//! wasm 边界零拷贝价格缓冲区（TODO「实现零拷贝内存管理与 Arrow 数据格式优化」落地件）。
//!
//! 模式：在 wasm 线性内存中分配一次（[`SharedF64Buffer::alloc`]），JS 经
//! `Float64Array::view_mut_raw` 视图直写（数据 ingress 零拷贝），计算侧经
//! [`SharedF64Buffer::as_slice`] 直读，可跨多次调用复用后显式释放。
//! 消除的是旧路径「每次调用 `to_vec()` 的 JS→wasm 一次 memcpy + 堆分配」。
//!
//! 生命周期契约（调用方必须遵守，wasm 无法校验外来指针）：
//! * 缓冲区自分配起地址恒定（定长、禁止任何扩容操作）；
//! * JS 视图在**任何后续 wasm 线性内存扩容**（grow / 大块分配）后失效——
//!   「JS 写视图」与「下一次 wasm 导出调用」之间不得插入其它会扩容的调用；
//!   Rust 侧切片不受 grow 影响（线性内存基址不变）；
//! * 同一 `(ptr, len)` 只能释放一次，双 free / 伪造指针 = UB；
//! * ptr 必须来自本模块 [`SharedF64Buffer::alloc`]（`Vec<f64>` 裸指针，f64 8 字节对齐）。
//!
//! JS 侧自动释放兜底（FinalizationRegistry）属调用方增强，本层只提供显式契约；
//! 全局缓冲池（按容量分桶复用）为后续扩展缝，当前由调用方自行复用。

/// 长度上限（1GiB / 8 字节元）：防误传超长在 wasm32 堆上直接 OOM trap
pub const MAX_BUFFER_LEN: usize = 1 << 27;

/// 定长 f64 缓冲区：分配后地址恒定，所有权在 Rust 侧持有期间受 Drop 保护
pub struct SharedF64Buffer {
    ptr: *mut f64,
    len: usize,
    /// 释放时回传给分配器的容量；`from_raw` 路径按 alloc 精确分配口径取 len
    capacity: usize,
}

impl SharedF64Buffer {
    /// 分配零填充缓冲区；`len` 为 f64 元素个数
    pub fn alloc(len: usize) -> Self {
        let mut vec: Vec<f64> = vec![0.0; len];
        // vec![elem; len] 按精确 layout 零填充分配，capacity == len 是
        // from_raw 回传口径 (ptr, len) 能完备重建所有权的前提
        assert_eq!(vec.capacity(), len, "vec![0.0; len] 须精确分配");
        let buf = Self {
            ptr: vec.as_mut_ptr(),
            len: vec.len(),
            capacity: vec.capacity(),
        };
        std::mem::forget(vec);
        buf
    }

    /// 依裸指针重建所有权（`freePriceBuffer` 的释放路径）。
    ///
    /// # Safety
    /// `(ptr, len)` 必须来自本模块 `alloc` 且尚未释放过，见模块文档契约。
    pub unsafe fn from_raw(ptr: *mut f64, len: usize) -> Self {
        Self {
            ptr,
            len,
            capacity: len,
        }
    }

    pub fn as_ptr(&self) -> *const f64 {
        self.ptr
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 计算侧直读视图。
    ///
    /// # Safety
    /// 类型层面 `&self` 已保证与 `as_mut_slice` 互斥；指针有效性由构造契约保证。
    pub fn as_slice(&self) -> &[f64] {
        // SAFETY: ptr 来自 alloc/from_raw 契约，len 与分配时一致
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }

    /// JS ingress 填充用的可写视图
    pub fn as_mut_slice(&mut self) -> &mut [f64] {
        // SAFETY: 同 as_slice，且独占借用保证无别名
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }

    /// 收回为 `Vec<f64>`（内容与容量原样归还，零拷贝）；此后原指针不可再用
    pub fn into_vec(self) -> Vec<f64> {
        let out = unsafe { Vec::from_raw_parts(self.ptr, self.len, self.capacity) };
        std::mem::forget(self);
        out
    }
}

impl Drop for SharedF64Buffer {
    fn drop(&mut self) {
        // SAFETY: (ptr, len, capacity) 三元组与分配时一致（定长缓冲）
        unsafe {
            drop(Vec::from_raw_parts(self.ptr, self.len, self.capacity));
        }
    }
}

impl std::fmt::Debug for SharedF64Buffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedF64Buffer")
            .field("ptr", &self.ptr)
            .field("len", &self.len)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    /// 确定性内容指纹：验证释放路径内容无损
    fn fingerprint(data: &[f64]) -> u64 {
        let mut hasher = DefaultHasher::new();
        for &v in data {
            v.to_bits().hash(&mut hasher);
        }
        hasher.finish()
    }

    #[test]
    fn alloc_zero_fills_and_roundtrips() {
        let mut buf = SharedF64Buffer::alloc(1000);
        assert_eq!(buf.len(), 1000);
        assert!(!buf.is_empty());
        assert!(buf.as_slice().iter().all(|&v| v == 0.0), "零填充");

        let source: Vec<f64> = (0..1000)
            .map(|i| (i as f64).sin() * 100.0 + 100.0)
            .collect();
        buf.as_mut_slice().copy_from_slice(&source);
        assert_eq!(buf.as_slice(), &source[..]);
    }

    #[test]
    fn raw_ptr_view_matches_slice_content() {
        // 验证 ptr 计算路径使用的 from_raw_parts 重建与 as_slice 内容一致
        let mut buf = SharedF64Buffer::alloc(512);
        buf.as_mut_slice()
            .copy_from_slice(&(0..512).map(|i| i as f64).collect::<Vec<_>>());
        let ptr = buf.as_ptr();
        let len = buf.len();
        // SAFETY: ptr/len 来自存活的 buf，读取期间 buf 未释放
        let reconstructed = unsafe { std::slice::from_raw_parts(ptr, len) };
        assert_eq!(reconstructed, buf.as_slice());
    }

    #[test]
    fn into_vec_reclaims_content_exactly() {
        let mut buf = SharedF64Buffer::alloc(256);
        buf.as_mut_slice()
            .copy_from_slice(&(7..263).map(|i| i as f64 * 1.5).collect::<Vec<_>>());
        let expected = fingerprint(buf.as_slice());
        let ptr = buf.as_ptr();
        let reclaimed = buf.into_vec();
        assert_eq!(reclaimed.len(), 256);
        assert_eq!(fingerprint(&reclaimed), expected);
        // into_vec 后原指针失效由 forget + 单一所有权保证（Double-free 由 Drop 不再触发）
        let _ = ptr;
    }

    #[test]
    fn from_raw_reclaims_allocation() {
        // 模拟 freePriceBuffer 完整路径：alloc → forget 交出所有权 → ptr 回传 → from_raw 释放
        let mut buf = SharedF64Buffer::alloc(128);
        buf.as_mut_slice()
            .copy_from_slice(&(0..128).map(|i| i as f64 + 0.25).collect::<Vec<_>>());
        let expected = fingerprint(buf.as_slice());
        let ptr = buf.as_ptr() as *mut f64;
        let len = buf.len();
        std::mem::forget(buf);
        // SAFETY: (ptr, len) 来自上方 forget 前的 alloc，且为首次释放
        let echo = unsafe { SharedF64Buffer::from_raw(ptr, len) };
        assert_eq!(fingerprint(echo.as_slice()), expected);
        drop(echo);
    }

    #[test]
    fn alignment_is_f64_aligned() {
        for len in [1usize, 3, 7, 64, 4096] {
            let buf = SharedF64Buffer::alloc(len);
            assert_eq!(
                buf.as_ptr() as usize % std::mem::align_of::<f64>(),
                0,
                "len={len} 分配须满足 f64 对齐（JS TypedArray byteOffset 前提）"
            );
        }
    }

    #[test]
    fn zero_len_is_valid_empty_buffer() {
        let buf = SharedF64Buffer::alloc(0);
        assert!(buf.is_empty());
        assert!(buf.as_slice().is_empty());
        drop(buf.into_vec());
    }
}
