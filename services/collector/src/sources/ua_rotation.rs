//! 请求头轮换池（architecture.md 抗封策略的 UA/Headers 半边）
//!
//! 此前四个采集源对同一目标站恒发同一 UA（默认串截断自 Chrome 91，
//! 属 2021 年老指纹）、固定 Accept-Language——单值请求头是最廉价的
//! 采集指纹。本模块提供 UA 与 Accept-Language 两个池，源在构建每个
//! 请求时随机取值：
//!
//! - `config.user_agent` 显式给定 → UA 不轮换（定向伪装优先，兼容
//!   旧配置语义）；None（现为默认）→ 池内轮换
//! - Accept-Language 始终轮换（原静态值进池）
//! - 池为空（手工构造）→ 回退文档化默认值，不发空头
//!
//! 各源的 Referer 等源内语义头仍走各自 `build_headers` 静态池，不入
//! 本模块。调度半边（重试退避抖动 / 派发抖动）在 source_scheduler.rs。

use rand::Rng;

/// 池空时的兜底 UA（现代 Chrome，非截断串）
const FALLBACK_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

/// 池空时的兜底 Accept-Language
const FALLBACK_LANGUAGE: &str = "zh-CN,zh;q=0.9,en;q=0.8";

/// UA / Accept-Language 轮换池（不可变，`Send + Sync`，各源实例共享）
#[derive(Debug, Clone)]
pub struct HeaderRotator {
    uas: Vec<String>,
    languages: Vec<String>,
}

impl Default for HeaderRotator {
    fn default() -> Self {
        Self {
            uas: DEFAULT_USER_AGENTS.iter().map(|s| s.to_string()).collect(),
            languages: DEFAULT_LANGUAGES.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// 默认 UA 池：2024 年主流浏览器 stable 指纹（Windows/macOS 双平台）
pub const DEFAULT_USER_AGENTS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64; rv:126.0) Gecko/20100101 Firefox/126.0",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15",
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36 Edg/124.0.0.0",
];

/// 默认 Accept-Language 池：简中环境常见取值序
pub const DEFAULT_LANGUAGES: &[&str] = &[
    "zh-CN,zh;q=0.9,en;q=0.8",
    "zh-CN,zh;q=0.9",
    "zh-CN,zh;q=0.9,en-US;q=0.8,en;q=0.7",
];

impl HeaderRotator {
    /// 自定义池（测试注入 / 运维收紧面）；空池回退兜底值
    pub fn new(uas: Vec<String>, languages: Vec<String>) -> Self {
        Self { uas, languages }
    }

    /// 取本次请求的 User-Agent：`config_ua` 显式给定则原样返回
    /// （不轮换），否则池内随机（池空回退 [`FALLBACK_UA`]）
    pub fn user_agent(&self, config_ua: Option<&str>) -> String {
        match config_ua {
            Some(ua) => ua.to_string(),
            None => self.pick(&self.uas, FALLBACK_UA),
        }
    }

    /// 取本次请求的 Accept-Language（池空回退 [`FALLBACK_LANGUAGE`]）
    pub fn accept_language(&self) -> String {
        self.pick(&self.languages, FALLBACK_LANGUAGE)
    }

    fn pick(&self, pool: &[String], fallback: &str) -> String {
        if pool.is_empty() {
            return fallback.to_string();
        }
        let idx = rand::thread_rng().gen_range(0..pool.len());
        pool[idx].clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn explicit_config_ua_wins_and_never_rotates() {
        let rotator = HeaderRotator::default();
        for _ in 0..32 {
            assert_eq!(
                rotator.user_agent(Some("CustomAgent/1.0")),
                "CustomAgent/1.0"
            );
        }
    }

    #[test]
    fn default_pool_rotates_across_values() {
        let rotator = HeaderRotator::default();
        let seen: HashSet<String> = (0..64).map(|_| rotator.user_agent(None)).collect();
        assert!(seen.len() >= 2, "64 次取样应覆盖 ≥2 种 UA，实际 {seen:?}");
        let all_values: HashSet<String> =
            DEFAULT_USER_AGENTS.iter().map(|s| s.to_string()).collect();
        assert!(
            seen.is_subset(&all_values),
            "取样值必须全部来自池内，混入 {seen:?}"
        );
    }

    #[test]
    fn empty_pool_falls_back_instead_of_empty_header() {
        let rotator = HeaderRotator::new(Vec::new(), Vec::new());
        assert_eq!(rotator.user_agent(None), FALLBACK_UA);
        assert_eq!(rotator.accept_language(), FALLBACK_LANGUAGE);
    }

    #[test]
    fn custom_pool_is_respected() {
        let rotator = HeaderRotator::new(
            vec!["A/1".to_string(), "B/2".to_string()],
            vec!["en-US".to_string()],
        );
        let seen: HashSet<String> = (0..32).map(|_| rotator.user_agent(None)).collect();
        assert_eq!(seen, HashSet::from(["A/1".to_string(), "B/2".to_string()]));
        assert_eq!(rotator.accept_language(), "en-US");
    }
}
