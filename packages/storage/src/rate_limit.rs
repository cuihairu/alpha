//! 分布式限流（L446）：Redis 固定窗口计数——INCR + EXPIRE 经 Lua 原子执行，
//! 多网关实例共享同一配额。窗口判定与键拼装纯函数化（无 Redis 也可单测）。

use std::time::Duration;

use alpha_core::errors::{AlphaError, AlphaResult};

/// 限流判定结果：`remaining` 为本窗口剩余配额（拒绝时恒 0），
/// `reset_after_secs` 为窗口重置剩余秒（固定窗口即窗口长度）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateDecision {
    pub allowed: bool,
    pub remaining: u32,
    pub reset_after_secs: u64,
}

/// 窗口判定（纯函数）：`count` 为 INCR 之后的当前计数——
/// 允许条件 `count <= limit`（第 limit 个请求仍放行）。
pub fn decide_window(count: i64, limit: u32, window_secs: u64) -> RateDecision {
    let allowed = count <= limit as i64;
    let remaining = if allowed {
        limit.saturating_sub(u32::try_from(count).unwrap_or(0))
    } else {
        0
    };
    RateDecision {
        allowed,
        remaining,
        reset_after_secs: window_secs,
    }
}

/// 限流键 = 前缀 + 主体 + 窗口索引（同主体不同窗口互不干扰）
pub fn window_key(prefix: &str, subject: &str, window_index: u64) -> String {
    format!("{prefix}{subject}:{window_index}")
}

/// 当前窗口索引（纯函数）：epoch 秒整除窗口长度
pub fn window_index(epoch_secs: u64, window_secs: u64) -> u64 {
    epoch_secs / window_secs
}

/// INCR + 首次 EXPIRE 原子化（两命令分开发请求之间存在计数无过期键的窗口）
const INCR_EXPIRE_LUA: &str = "
local count = redis.call('INCR', KEYS[1])
if count == 1 then
  redis.call('EXPIRE', KEYS[1], ARGV[1])
end
return count
";

#[derive(Clone)]
pub struct RedisRateLimiter {
    conn: redis::aio::ConnectionManager,
    prefix: String,
}

impl RedisRateLimiter {
    pub async fn connect(connection_string: &str, prefix: &str) -> AlphaResult<Self> {
        let client = redis::Client::open(connection_string)
            .map_err(|e| AlphaError::ConfigurationError(format!("invalid redis URL: {e}")))?;
        let conn = client
            .get_connection_manager()
            .await
            .map_err(|e| AlphaError::StorageError(format!("redis connect failed: {e}")))?;
        Ok(Self {
            conn,
            prefix: prefix.to_string(),
        })
    }

    /// 消费一个配额：主体（如客户端标识）在当前窗口内计数 +1 并判定。
    pub async fn check(
        &self,
        subject: &str,
        limit: u32,
        window: Duration,
    ) -> AlphaResult<RateDecision> {
        let epoch_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let key = window_key(
            &self.prefix,
            subject,
            window_index(epoch_secs, window.as_secs()),
        );

        let mut conn = self.conn.clone();
        let count: i64 = redis::Script::new(INCR_EXPIRE_LUA)
            .key(&key)
            .arg(window.as_secs())
            .invoke_async(&mut conn)
            .await
            .map_err(|e| AlphaError::StorageError(format!("rate limit INCR failed: {e}")))?;

        Ok(decide_window(count, limit, window.as_secs()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_window_matrix() {
        // 窗口内第 1 / limit 个请求放行，第 limit+1 个拒绝且 remaining 归 0
        assert!(decide_window(1, 3, 60).allowed);
        assert_eq!(decide_window(1, 3, 60).remaining, 2);
        assert!(decide_window(3, 3, 60).allowed);
        assert_eq!(decide_window(3, 3, 60).remaining, 0);
        let denied = decide_window(4, 3, 60);
        assert!(!denied.allowed);
        assert_eq!(denied.remaining, 0);
        assert_eq!(denied.reset_after_secs, 60);
    }

    #[test]
    fn window_key_and_index_are_pure() {
        assert_eq!(
            window_key("alpha:rl:", "1.2.3.4", 17),
            "alpha:rl:1.2.3.4:17"
        );
        assert_eq!(window_index(0, 60), 0);
        assert_eq!(window_index(59, 60), 0);
        assert_eq!(window_index(60, 60), 1);
        assert_eq!(window_index(125, 60), 2);
    }

    fn redis_url() -> Option<String> {
        std::env::var("REDIS_TEST_URL").ok()
    }

    /// 集成（REDIS_TEST_URL 门控）：limit 内放行 / 超限拒绝 / 主体隔离 / 窗口滚动重置
    #[tokio::test]
    async fn fixed_window_enforcement_and_isolation() -> AlphaResult<()> {
        let Some(url) = redis_url() else {
            return Ok(());
        };
        let prefix = format!("alpha:test:rl:{}:", uuid::Uuid::new_v4());
        let limiter = RedisRateLimiter::connect(&url, &prefix).await?;

        // 同主体 limit=2：两次放行、第三次拒绝
        let d1 = limiter.check("client-a", 2, Duration::from_secs(1)).await?;
        assert!(d1.allowed && d1.remaining == 1);
        let d2 = limiter.check("client-a", 2, Duration::from_secs(1)).await?;
        assert!(d2.allowed && d2.remaining == 0);
        let d3 = limiter.check("client-a", 2, Duration::from_secs(1)).await?;
        assert!(!d3.allowed);

        // 主体隔离：client-b 配额不受 client-a 影响
        let d4 = limiter.check("client-b", 2, Duration::from_secs(1)).await?;
        assert!(d4.allowed);

        // 窗口滚动（1s 窗口 + 睡过边界）：新窗口重新放行
        tokio::time::sleep(Duration::from_millis(1100)).await;
        let d5 = limiter.check("client-a", 2, Duration::from_secs(1)).await?;
        assert!(d5.allowed);

        Ok(())
    }
}
