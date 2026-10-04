//! Instrument 契约（docs/architecture-review.md §3.2：数据契约地基）
//!
//! A 股同一标的的多形态符号（600519 / SH600519 / 600519.SH / 600519.SSE /
//! 贵州茅台）必须统一消歧为全仓唯一引用键：
//!
//! ```text
//! instrument_id = "{market}.{exchange}.{symbol}"
//!               = "cn.sse.600519" / "cn.szse.000001"
//! ```
//!
//! 交易所前缀显式入 ID——`000001` 在 SSE 是上证指数、在 SZSE 是平安银行，
//! 裸数字符号天然跨市场歧义，任何层级引用必须携带 exchange。
//!
//! 本模块是**纯契约层**（serde 可序列化、无 IO、wasm-clean）；种子注册表与
//! 查询端点在 data-engine（数据面）。

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use std::fmt;

/// 交易所
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Exchange {
    #[serde(rename = "SSE")]
    Sse,
    #[serde(rename = "SZSE")]
    Szse,
}

/// 市场（当前仅 A 股；港股/美股接入时扩展）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Market {
    #[serde(rename = "cn")]
    Cn,
}

/// 证券类别
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstrumentType {
    #[serde(rename = "equity")]
    Equity,
    #[serde(rename = "index")]
    Index,
    #[serde(rename = "fund")]
    Fund,
    #[serde(rename = "bond")]
    Bond,
    #[serde(rename = "etf")]
    Etf,
    #[serde(rename = "other")]
    Other,
}

/// 上市状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum InstrumentStatus {
    #[serde(rename = "active")]
    Active,
    #[serde(rename = "suspended")]
    Suspended,
    #[serde(rename = "delisted")]
    Delisted,
}

/// 证券主数据（全仓统一 instrument_id 引用）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Instrument {
    /// 唯一引用键：`cn.sse.600519`（解析见 [`instrument_id`]）
    pub instrument_id: String,
    #[serde(rename = "type")]
    pub instrument_type: InstrumentType,
    pub exchange: Exchange,
    pub market: Market,
    /// 裸数字符号（6 位，不含交易所前缀/后缀）
    pub symbol: String,
    pub name: String,
    pub currency: String,
    pub status: InstrumentStatus,
    pub listed_at: Option<NaiveDate>,
    pub delisted_at: Option<NaiveDate>,
}

impl Instrument {
    pub fn new(
        exchange: Exchange,
        symbol: impl Into<String>,
        name: impl Into<String>,
        instrument_type: InstrumentType,
    ) -> Self {
        let symbol = symbol.into();
        Self {
            instrument_id: instrument_id(Market::Cn, exchange, &symbol),
            instrument_type,
            exchange,
            market: Market::Cn,
            symbol,
            name: name.into(),
            currency: "CNY".to_string(),
            status: InstrumentStatus::Active,
            listed_at: None,
            delisted_at: None,
        }
    }
}

impl fmt::Display for Market {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Market::Cn => write!(f, "cn"),
        }
    }
}

impl fmt::Display for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Exchange::Sse => write!(f, "sse"),
            Exchange::Szse => write!(f, "szse"),
        }
    }
}

impl Exchange {
    /// 解析交易所 token（大小写不敏感；支持全名 SSE/SZSE 与短码 SH/SZ）
    pub fn parse_token(raw: &str) -> Result<Self, String> {
        match raw.trim().to_ascii_uppercase().as_str() {
            "SSE" | "SH" => Ok(Exchange::Sse),
            "SZSE" | "SZ" => Ok(Exchange::Szse),
            other => Err(format!("未知交易所「{other}」（可选 SSE/SH、SZSE/SZ）")),
        }
    }
}

/// 构造全仓唯一键。market 当前仅 cn，exchange 显式携带消歧
pub fn instrument_id(market: Market, exchange: Exchange, symbol: &str) -> String {
    format!("{}.{}.{}", market, exchange, symbol)
}

/// 解析 instrument_id 回 (market, exchange, symbol)
pub fn parse_instrument_id(id: &str) -> Result<(Market, Exchange, String), String> {
    let parts: Vec<&str> = id.trim().split('.').collect();
    let [market, exchange, symbol] = parts.as_slice() else {
        return Err(format!(
            "instrument_id 「{id}」须为市场.交易所.符号 3 段（如 cn.sse.600519）"
        ));
    };
    let market = match market.to_ascii_lowercase().as_str() {
        "cn" => Market::Cn,
        other => return Err(format!("未知市场「{other}」（当前仅 cn）")),
    };
    let exchange = Exchange::parse_token(exchange)?;
    // instrument_id 是规范键（小写交易所 + 裸数字），此处只做格式校验，不做
    // 代码段-交易所边界校验：那属于 parse_symbol（人手输入面）。作为系统内
    // 引用键，cn.sse.000001（上证指数）与 cn.szse.000001（平安银行）都合法；
    // 边界校验会把 cn.sse.999999 这类「格式合法但代码段与交易所段何谓」的
    // 键误判为请求格式错误——查询未命中（404）才是正确语义。
    let symbol = symbol.trim().to_ascii_uppercase();
    if symbol.len() != 6 || !symbol.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!(
            "instrument_id 「{id}」符号段必须为 6 位纯数字（'{symbol}'）"
        ));
    }
    Ok((market, exchange, symbol))
}

/// 多形态 A 股符号 → (交易所, 裸数字符号)。
///
/// 接受形态：`600519` / `SH600519` / `600519.SH` / `600519.SSE` /
/// `szse.000001` 等（前缀/后缀大小写不敏感，前后可带空白）。
/// 裸数字（无前缀后缀）时按代码段推断交易所：
/// - `60xxxx` / `688xxx`（科创板）/ `51xxxx`（沪市基金）→ SSE
/// - `00xxxx` / `30xxxx`（创业板）/ `15xxxx`（深市基金）→ SZSE
///
/// 推断不出或带显式交易所时以显式为准并校验数字段为 6 位。
pub fn parse_symbol(raw: &str) -> Result<(Exchange, String), String> {
    let input = raw.trim();
    if input.is_empty() {
        return Err("symbol 不能为空".to_string());
    }
    if !input
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
    {
        return Err(format!("symbol 「{raw}」含非法字符（仅字母/数字/./_）"));
    }

    // 剥后缀：600519.SH / 600519.SSE（亦容忍 sh.600519 前缀在第二轮处理）
    let upper = input.to_ascii_uppercase();
    let (body, suffix_exchange): (&str, Option<Exchange>) = match upper.rsplit_once('.') {
        Some((b, tok)) if tok.chars().all(|c| c.is_ascii_alphabetic()) => {
            match tok {
                "SH" | "SSE" => (b, Some(Exchange::Sse)),
                "SZ" | "SZSE" => (b, Some(Exchange::Szse)),
                _ => (input, None), // 后缀非交易所 token，原样降级
            }
        }
        _ => (input, None),
    };

    // 剥前缀：SH600519 / szse.000001（小写 sh.600519 经统一大写后在此命中）
    let (digits, prefix_exchange) = match body.split_once('.') {
        Some((tok, rest))
            if tok.chars().all(|c| c.is_ascii_alphabetic())
                && rest.chars().all(|c| c.is_ascii_digit()) =>
        {
            (rest, Some(Exchange::parse_token(tok)?))
        }
        _ => {
            let toks: Vec<&str> = body.split('.').collect();
            if toks.len() > 2 {
                return Err(format!(
                    "symbol 「{raw}」段数过多（支持 交易所.符号 / 符号.交易所 / 裸数字）"
                ));
            }
            // 无点贴连前缀：SH600519 / sz000001
            let alpha_run = body.chars().take_while(|c| c.is_ascii_alphabetic()).count();
            if alpha_run > 0 && alpha_run < body.len() {
                let (tok, rest) = body.split_at(alpha_run);
                (rest, Some(Exchange::parse_token(tok)?))
            } else {
                (body, None)
            }
        }
    };

    let digits = digits.to_ascii_uppercase();
    if digits.len() != 6 || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!(
            "symbol 「{raw}」的数字段必须为 6 位纯数字（'{digits}'）"
        ));
    }

    // 显式交易所（前缀优先，后缀次之）→ 校验代码段与类型边界；否则按段推断
    let exchange = match (prefix_exchange, suffix_exchange) {
        (Some(e), _) | (None, Some(e)) => {
            validate_exchange_for_symbol(e, &digits)?;
            e
        }
        (None, None) => infer_exchange(&digits)?,
    };
    Ok((exchange, digits))
}

/// 代码段与交易所的硬约束。
///
/// 注意 00 段**两边都合法**：000001.SH 是上证指数、000001.SZ 是平安银行——
/// 这正是 instrument_id 必须带 exchange 的原因。矛盾的是结构性不受对方
/// 交易所接纳的段（上交所不收 30 段、深交所不收 60 段）。
fn validate_exchange_for_symbol(exchange: Exchange, digits: &str) -> Result<(), String> {
    let ok = match exchange {
        Exchange::Sse => {
            digits.starts_with("60")
                || digits.starts_with("68")
                || digits.starts_with("00") // 上证体系指数（000001 上证指数等）
                || digits.starts_with("51")
                || digits.starts_with("58")
                || digits.starts_with("11")
                || digits.starts_with("90") // 沪市 B 股
        }
        Exchange::Szse => {
            digits.starts_with("00")
                || digits.starts_with("30")
                || digits.starts_with("15")
                || digits.starts_with("12")
                || digits.starts_with("39") // 深证体系指数（399001 深证成指等）
                || digits.starts_with("20") // 深市 B 股
        }
    };
    if !ok {
        return Err(format!(
            "符号 {} 与交易所 {exchange} 矛盾（上交所段：60/68/00指数/51/58/11/90；深交所段：00/30/15/12/39/20）",
            digits
        ));
    }
    Ok(())
}

fn infer_exchange(digits: &str) -> Result<Exchange, String> {
    if digits.starts_with("60") || digits.starts_with("68") {
        Ok(Exchange::Sse)
    } else if digits.starts_with("00") || digits.starts_with("30") {
        Ok(Exchange::Szse)
    } else if digits.starts_with("51") || digits.starts_with("58") || digits.starts_with("11") {
        Ok(Exchange::Sse)
    } else if digits.starts_with("15") || digits.starts_with("12") {
        Ok(Exchange::Szse)
    } else {
        Err(format!(
            "symbol {digits} 无法推断交易所——请显式携带（如 sh.{digits} / {digits}.sz）"
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_symbols_with_inference() {
        assert_eq!(parse_symbol("600519"), Ok((Exchange::Sse, "600519".into())));
        assert_eq!(
            parse_symbol("000001"),
            Ok((Exchange::Szse, "000001".into())),
            "00 段深市"
        );
        assert_eq!(
            parse_symbol("688009"),
            Ok((Exchange::Sse, "688009".into())),
            "科创板"
        );
        assert_eq!(
            parse_symbol("300750"),
            Ok((Exchange::Szse, "300750".into())),
            "创业板"
        );
        assert_eq!(
            parse_symbol("510300"),
            Ok((Exchange::Sse, "510300".into())),
            "沪市基金"
        );
        assert_eq!(
            parse_symbol("159915"),
            Ok((Exchange::Szse, "159915".into())),
            "深市基金"
        );
    }

    #[test]
    fn parses_prefix_suffix_forms_case_insensitively() {
        for raw in [
            "SH600519",
            "600519.SH",
            "600519.SSE",
            "sh.600519",
            "sse.600519",
            " 600519.SH ",
        ] {
            assert_eq!(
                parse_symbol(raw),
                Ok((Exchange::Sse, "600519".into())),
                "{raw}"
            );
        }
        for raw in ["SZ000001", "000001.SZ", "000001.SZSE"] {
            assert_eq!(
                parse_symbol(raw),
                Ok((Exchange::Szse, "000001".into())),
                "{raw}"
            );
        }
    }

    #[test]
    fn explicit_exchange_beats_inference_and_disambiguates() {
        // 000001：裸数字 = 深市平安银行；显式 SSE = 上证指数（000001.SH）
        assert_eq!(
            parse_symbol("000001"),
            Ok((Exchange::Szse, "000001".into()))
        );
        assert_eq!(
            parse_symbol("000001.SH"),
            Ok((Exchange::Sse, "000001".into()))
        );
        assert_eq!(
            parse_symbol("SH.000001"),
            Ok((Exchange::Sse, "000001".into()))
        );
        assert_eq!(
            parse_symbol("SH000001"),
            Ok((Exchange::Sse, "000001".into()))
        );
        // 显式交易所与代码段结构性矛盾要报错（60 段不上深交所、30 段不上上交所）
        assert!(parse_symbol("600519.SZ").is_err());
        assert!(parse_symbol("SZ600519").is_err());
        assert!(parse_symbol("300750.SH").is_err());
    }

    #[test]
    fn rejects_malformed_symbols() {
        for bad in [
            "",
            "abc",
            "12345",
            "1234567",
            "60051x",
            "6005x9",
            "600 519",
            "a.600519.b",
        ] {
            assert!(parse_symbol(bad).is_err(), "{bad:?} 应拒绝");
        }
        assert!(
            parse_symbol("SSH600519").is_err(),
            "前缀含交易所全名带第三字母"
        );
    }

    #[test]
    fn instrument_id_is_canonical_and_roundtrips() {
        let id = instrument_id(Market::Cn, Exchange::Sse, "600519");
        assert_eq!(id, "cn.sse.600519");
        assert_eq!(
            parse_instrument_id("cn.sse.600519"),
            Ok((Market::Cn, Exchange::Sse, "600519".into()))
        );
        // 000001 双市场各自独立键，且 cn.sse.000001（上证指数）可回解析
        assert_ne!(
            instrument_id(Market::Cn, Exchange::Sse, "000001"),
            instrument_id(Market::Cn, Exchange::Szse, "000001")
        );
        assert_eq!(
            parse_instrument_id("cn.sse.000001"),
            Ok((Market::Cn, Exchange::Sse, "000001".into())),
            "上证指数键"
        );
        // 规范键只做格式校验：格式合法的键均可解析（是否真实存在由注册表裁决）
        assert_eq!(
            parse_instrument_id("cn.szse.600519"),
            Ok((Market::Cn, Exchange::Szse, "600519".into()))
        );
        assert!(parse_instrument_id("cn.600519").is_err(), "缺交易所段");
        assert!(parse_instrument_id("us.sse.600519").is_err(), "未知市场");
        assert!(
            parse_instrument_id("cn.sse.60051x").is_err(),
            "非纯数字符号段"
        );
    }

    #[test]
    fn instrument_serializes_to_canonical_json() {
        let moutai = Instrument::new(Exchange::Sse, "600519", "贵州茅台", InstrumentType::Equity);
        let json = serde_json::to_value(&moutai).expect("序列化");
        assert_eq!(json["instrument_id"], "cn.sse.600519");
        assert_eq!(json["type"], "equity");
        assert_eq!(json["exchange"], "SSE");
        assert_eq!(json["currency"], "CNY");
        // 反向
        let back: Instrument = serde_json::from_value(json).expect("反序列化");
        assert_eq!(back, moutai);
        // 未知字段拒绝（契约严格性）
        let mut extra = serde_json::to_value(&moutai).expect("序列化");
        extra["ghost"] = serde_json::Value::Bool(true);
        assert!(serde_json::from_value::<Instrument>(extra).is_err());
    }
}
