//! 零拷贝数据处理与内存池管理（L451）。
//!
//! 高频行情路径的两个分配优化面：
//! * [`BufferPool`]：`Vec<u8>` 缓冲对象池——acquire/release 复用已分配缓冲，
//!   消除反复堆分配（配命中率计数验证复用确实发生）；
//! * [`QuoteFrame`]：32 字节定长行情帧的**零拷贝**解码视图——symbol 直接借用
//!   输入缓冲（`&'a str`），标量字段按位读出，全程零堆分配；
//!   [`encode_frame_into`] 反向写帧时优先写入池化缓冲，两块组合成
//!   「解码借用 / 编码复用」的完整闭环。
//!
//! 线程模型沿 platform.rs 惯例：`std::sync::Mutex` + 锁中毒 map_err。

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use crate::errors::{AlphaError, AlphaResult};

// ---------------------------------------------------------------------------
// 缓冲对象池
// ---------------------------------------------------------------------------

/// `Vec<u8>` 缓冲池：release 的缓冲回收复用，acquire 优先从池取。
///
/// 回收策略（防池膨胀）：
/// * 单缓冲容量超过 `max_capacity` → 直接丢弃不回收（大缓冲低频，不值得占池）；
/// * 池内空闲数达到 `max_idle` → 多余缓冲丢弃。
#[derive(Debug)]
pub struct BufferPool {
    max_capacity: usize,
    max_idle: usize,
    idle: Mutex<Vec<Vec<u8>>>,
    acquisitions: AtomicU64,
    reuses: AtomicU64,
}

impl BufferPool {
    pub fn new(max_capacity: usize, max_idle: usize) -> Self {
        Self {
            max_capacity,
            max_idle,
            idle: Mutex::new(Vec::new()),
            acquisitions: AtomicU64::new(0),
            reuses: AtomicU64::new(0),
        }
    }

    fn lock(&self) -> AlphaResult<std::sync::MutexGuard<'_, Vec<Vec<u8>>>> {
        self.idle
            .lock()
            .map_err(|e| AlphaError::InternalError(format!("buffer pool lock poisoned: {e}")))
    }

    /// 取一个长度恰为 `len`（内容清零）的缓冲：池空或池内缓冲容量不足时新分配。
    pub fn acquire(&self, len: usize) -> AlphaResult<PooledBuffer<'_>> {
        self.acquisitions.fetch_add(1, Ordering::Relaxed);
        let reused = {
            let mut idle = self.lock()?;
            idle.pop()
        };
        let mut buf = match reused {
            Some(buf) if buf.capacity() >= len => {
                self.reuses.fetch_add(1, Ordering::Relaxed);
                buf
            }
            _ => Vec::with_capacity(len),
        };
        buf.clear();
        buf.resize(len, 0);
        Ok(PooledBuffer { buf, pool: self })
    }

    fn release(&self, buf: &mut Vec<u8>) {
        if buf.capacity() > self.max_capacity {
            return;
        }
        buf.clear();
        let mut idle = match self.lock() {
            Ok(guard) => guard,
            Err(_) => return, // 中毒池宁可漏回收不可 panic in drop
        };
        if idle.len() < self.max_idle {
            idle.push(std::mem::take(buf));
        }
    }

    pub fn acquisitions(&self) -> u64 {
        self.acquisitions.load(Ordering::Relaxed)
    }

    pub fn reuses(&self) -> u64 {
        self.reuses.load(Ordering::Relaxed)
    }

    /// 复用率（reuses / acquisitions；0 次获取时为 0.0）
    pub fn reuse_ratio(&self) -> f64 {
        let total = self.acquisitions();
        if total == 0 {
            0.0
        } else {
            self.reuses() as f64 / total as f64
        }
    }

    pub fn idle_buffers(&self) -> AlphaResult<usize> {
        Ok(self.lock()?.len())
    }
}

/// 池化缓冲：`Deref`/`DerefMut` 到 `Vec<u8>`，Drop 时自动归还池
/// （容量超限或池满由池侧丢弃）。
pub struct PooledBuffer<'pool> {
    buf: Vec<u8>,
    pool: &'pool BufferPool,
}

impl std::ops::Deref for PooledBuffer<'_> {
    type Target = Vec<u8>;

    fn deref(&self) -> &Self::Target {
        &self.buf
    }
}

impl std::ops::DerefMut for PooledBuffer<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.buf
    }
}

impl Drop for PooledBuffer<'_> {
    fn drop(&mut self) {
        self.pool.release(&mut self.buf);
    }
}

// ---------------------------------------------------------------------------
// 32 字节定长行情帧（零拷贝解码 / 池化编码）
// ---------------------------------------------------------------------------

/// 帧长：symbol[6] + pad[2] + price f64 + volume f64 + timestamp i64
pub const QUOTE_FRAME_LEN: usize = 32;

/// 定长行情帧的零拷贝解码视图：symbol 借用输入缓冲，标量按位读出
/// （LE 字节序；跨平台无对齐要求——`from_le_bytes` 逐字节构造）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuoteFrame<'a> {
    /// 六位代码（尾部 NUL 补齐，解码时剥离）
    pub symbol: &'a str,
    pub price: f64,
    pub volume: f64,
    pub timestamp_ms: i64,
}

/// 零拷贝解码：只校验长度与 symbol ASCII，不复制任何业务数据。
pub fn decode_frame(frame: &[u8]) -> AlphaResult<QuoteFrame<'_>> {
    if frame.len() != QUOTE_FRAME_LEN {
        return Err(AlphaError::InvalidInput(format!(
            "行情帧长度应为 {QUOTE_FRAME_LEN} 字节，实际 {}",
            frame.len()
        )));
    }
    let symbol_bytes = &frame[..6];
    let symbol = core::str::from_utf8(symbol_bytes)
        .map_err(|_| AlphaError::InvalidInput("行情帧 symbol 非合法 UTF-8".to_string()))?;
    let symbol = symbol.trim_end_matches('\0');
    if symbol.is_empty() {
        return Err(AlphaError::InvalidInput("行情帧 symbol 为空".to_string()));
    }

    Ok(QuoteFrame {
        symbol,
        price: f64::from_le_bytes(frame[8..16].try_into().expect("长度已校验")),
        volume: f64::from_le_bytes(frame[16..24].try_into().expect("长度已校验")),
        timestamp_ms: i64::from_le_bytes(frame[24..32].try_into().expect("长度已校验")),
    })
}

/// 编码入给定缓冲（`buf` 重置为 32 字节帧）：配合 [`BufferPool::acquire`]
/// 复用缓冲，消除高频编码路径的堆分配。
pub fn encode_frame_into(frame: &QuoteFrame<'_>, buf: &mut Vec<u8>) -> AlphaResult<()> {
    let symbol = frame.symbol.as_bytes();
    if symbol.is_empty() || symbol.len() > 6 {
        return Err(AlphaError::InvalidInput(format!(
            "symbol 应为 1..=6 字节，实际 {}",
            symbol.len()
        )));
    }

    buf.clear();
    buf.resize(QUOTE_FRAME_LEN, 0);
    buf[..symbol.len()].copy_from_slice(symbol);
    // pad[6..8] 保持 0
    buf[8..16].copy_from_slice(&frame.price.to_le_bytes());
    buf[16..24].copy_from_slice(&frame.volume.to_le_bytes());
    buf[24..32].copy_from_slice(&frame.timestamp_ms.to_le_bytes());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pool_reuses_released_buffers_and_tracks_ratio() {
        let pool = BufferPool::new(1024, 4);
        assert_eq!(pool.reuse_ratio(), 0.0, "未获取过时复用率为 0");

        {
            let mut buf = pool.acquire(64).unwrap();
            buf[..4].copy_from_slice(b"ABCD");
        } // drop → 归还
        assert_eq!(pool.idle_buffers().unwrap(), 1);

        {
            let buf = pool.acquire(32).unwrap();
            assert_eq!(buf.len(), 32);
            assert!(buf.iter().all(|b| *b == 0), "复用缓冲内容必须清零");
        }
        assert_eq!(pool.acquisitions(), 2);
        assert_eq!(pool.reuses(), 1);
        assert!((pool.reuse_ratio() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn pool_drops_oversized_and_overflowing_buffers() {
        let pool = BufferPool::new(64, 2);
        // 超 max_capacity 的缓冲不回收
        {
            let big = pool.acquire(128).unwrap();
            assert!(big.capacity() >= 128);
        }
        assert_eq!(pool.idle_buffers().unwrap(), 0);

        // 同时持有 4 个再归还：池满（max_idle=2）后多余缓冲丢弃
        let held: Vec<_> = (0..4).map(|_| pool.acquire(16).unwrap()).collect();
        drop(held);
        assert_eq!(pool.idle_buffers().unwrap(), 2);
    }

    #[test]
    fn frame_encode_decode_roundtrip_preserves_all_fields() {
        let frame = QuoteFrame {
            symbol: "600519",
            price: 147.25, // 2^-2 精确二进制小数
            volume: 2048.0,
            timestamp_ms: 1_790_870_400_000,
        };
        let pool = BufferPool::new(QUOTE_FRAME_LEN * 2, 4);
        let mut buf = pool.acquire(QUOTE_FRAME_LEN).unwrap();
        encode_frame_into(&frame, &mut buf).unwrap();

        assert_eq!(buf.len(), QUOTE_FRAME_LEN);
        // pad 字节为 0；symbol 六字节无补齐空间
        assert_eq!(&buf[6..8], &[0, 0]);
        assert_eq!(&buf[..6], b"600519");

        let decoded = decode_frame(&buf).unwrap();
        assert_eq!(decoded.symbol, "600519");
        assert_eq!(decoded.price, 147.25);
        assert_eq!(decoded.volume, 2048.0);
        assert_eq!(decoded.timestamp_ms, 1_790_870_400_000);
    }

    #[test]
    fn frame_decode_rejects_bad_length_symbol_and_empty() {
        let pool = BufferPool::new(64, 2);
        let mut buf = pool.acquire(QUOTE_FRAME_LEN).unwrap();
        encode_frame_into(
            &QuoteFrame {
                symbol: "600519",
                price: 1.0,
                volume: 1.0,
                timestamp_ms: 1,
            },
            &mut buf,
        )
        .unwrap();

        // 长度不对
        assert!(decode_frame(&buf[..31]).is_err());
        assert!(decode_frame(&[]).is_err());
        // symbol 非 UTF-8（0xFF 在 [..6] 内）
        let mut bad_symbol = buf.clone();
        bad_symbol[0] = 0xFF;
        assert!(decode_frame(&bad_symbol).is_err());
        // symbol 全 NUL → 剥离后为空
        let mut empty_symbol = buf.clone();
        empty_symbol[..6].fill(0);
        assert!(decode_frame(&empty_symbol).is_err());
        // 编码侧：symbol 超长拒绝
        let mut sink = Vec::new();
        assert!(encode_frame_into(
            &QuoteFrame {
                symbol: "6005191",
                price: 1.0,
                volume: 1.0,
                timestamp_ms: 1,
            },
            &mut sink
        )
        .is_err());
    }
}
