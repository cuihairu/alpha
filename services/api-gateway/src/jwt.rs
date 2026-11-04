//! 纯 HMAC-SHA256 JWT 实现（HS256 only）。
//!
//! 替换 jsonwebtoken 以摆脱 p256/rsa/elliptic-curve/crypto-bigint 生态
//! （VULN-ALPHA-005/006）。本项目认证面只用 HS256 对称密钥与 oct JWKS，
//! 故无需 EC/RSA；接口形状对齐 jsonwebtoken 以最小化 auth.rs 改动面。

use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;

pub type HmacSha256 = Hmac<Sha256>;

pub fn b64url_encode(data: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

pub fn b64url_decode(data: &str) -> Result<Vec<u8>, ()> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(data)
        .map_err(|_| ())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Algorithm {
    HS256,
}

#[derive(Debug, Clone)]
pub struct Header {
    pub alg: Algorithm,
    pub kid: Option<String>,
}

impl Header {
    pub fn new(alg: Algorithm) -> Self {
        Self { alg, kid: None }
    }
}

#[derive(Debug, Clone)]
pub struct EncodingKey {
    secret: Vec<u8>,
}

impl EncodingKey {
    pub fn from_secret(secret: &[u8]) -> Self {
        Self {
            secret: secret.to_vec(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DecodingKey {
    secret: Vec<u8>,
}

impl DecodingKey {
    pub fn from_secret(secret: &[u8]) -> Self {
        Self {
            secret: secret.to_vec(),
        }
    }
}

#[derive(Debug)]
pub struct Validation {
    pub alg: Algorithm,
    pub validate_exp: bool,
    pub leeway: u64,
    pub required_spec_claims: Vec<String>,
    pub iss: Option<Vec<String>>,
    pub aud: Option<Vec<String>>,
}

impl Validation {
    pub fn new(alg: Algorithm) -> Self {
        Self {
            alg,
            validate_exp: false,
            leeway: 0,
            required_spec_claims: Vec::new(),
            iss: None,
            aud: None,
        }
    }

    pub fn set_issuer(&mut self, issuers: &[&str]) {
        self.iss = Some(issuers.iter().map(|s| s.to_string()).collect());
    }

    pub fn set_audience(&mut self, auds: &[&str]) {
        self.aud = Some(auds.iter().map(|s| s.to_string()).collect());
    }
}

fn hmac_sign(secret: &[u8], data: &[u8]) -> Result<Vec<u8>, ()> {
    let mut mac = HmacSha256::new_from_slice(secret).map_err(|_| ())?;
    mac.update(data);
    Ok(mac.finalize().into_bytes().to_vec())
}

#[derive(Deserialize)]
struct HeaderPayload {
    alg: String,
    #[serde(default)]
    kid: Option<String>,
}

/// 解码 JWT 头（不验签，仅取 alg/kid）。
pub fn decode_header(token: &str) -> Result<Header, ()> {
    let mut parts = token.split('.');
    let h = parts.next().ok_or(())?;
    let hd: HeaderPayload = serde_json::from_slice(&b64url_decode(h)?).map_err(|_| ())?;
    let alg = match hd.alg.as_str() {
        "HS256" => Algorithm::HS256,
        _ => return Err(()),
    };
    Ok(Header { alg, kid: hd.kid })
}

/// 签发 JWT（HS256）。
pub fn encode<T: Serialize>(header: &Header, claims: &T, key: &EncodingKey) -> Result<String, ()> {
    let mut h = serde_json::json!({
        "alg": "HS256",
        "typ": "JWT",
    });
    if let Some(kid) = &header.kid {
        h["kid"] = serde_json::Value::String(kid.clone());
    }
    let c = serde_json::to_vec(claims).map_err(|_| ())?;
    let header_b64 = b64url_encode(h.to_string().as_bytes());
    let claims_b64 = b64url_encode(&c);
    let signing_input = format!("{header_b64}.{claims_b64}");
    let sig = hmac_sign(&key.secret, signing_input.as_bytes())?;
    Ok(format!("{signing_input}.{}", b64url_encode(&sig)))
}

/// 校验并解码 JWT（HS256），验证 exp/iss/aud。
pub fn decode(
    token: &str,
    key: &DecodingKey,
    validation: &Validation,
) -> Result<serde_json::Value, ()> {
    let mut parts = token.split('.');
    let header_b64 = parts.next().ok_or(())?;
    let claims_b64 = parts.next().ok_or(())?;
    let sig_b64 = parts.next().ok_or(())?;
    if parts.next().is_some() {
        return Err(());
    }

    let header = decode_header(token)?;
    if header.alg != validation.alg {
        return Err(());
    }

    let signing_input = format!("{header_b64}.{claims_b64}");
    let expected = hmac_sign(&key.secret, signing_input.as_bytes())?;
    let sig = b64url_decode(sig_b64)?;
    // 恒定时间比较：不因签名前缀匹配长度而泄露响应时序（jsonwebtoken 同款语义）
    if !bool::from(expected.ct_eq(&sig)) {
        return Err(());
    }

    let claims_bytes = b64url_decode(claims_b64)?;
    let value: serde_json::Value = serde_json::from_slice(&claims_bytes).map_err(|_| ())?;
    // required_spec_claims：声明即强制存在，与 validate_exp 开关正交
    // （jsonwebtoken 语义一致，杜绝「validate_exp=false 时静默不校验」的死配置）
    for claim in &validation.required_spec_claims {
        if value.get(claim).is_none() {
            return Err(());
        }
    }
    if validation.validate_exp {
        let exp = value.get("exp").and_then(|v| v.as_i64()).ok_or(())?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        if now > exp + validation.leeway as i64 {
            return Err(());
        }
    }
    if let Some(iss) = &validation.iss {
        let token_iss = value.get("iss").and_then(|v| v.as_str()).ok_or(())?;
        if !iss.iter().any(|i| i == token_iss) {
            return Err(());
        }
    }
    if let Some(aud) = &validation.aud {
        // RFC 7519 §4.1.3：aud 可为字符串或字符串数组；任一成员命中即通过。
        let token_auds = match value.get("aud") {
            Some(v) => match v {
                serde_json::Value::String(s) => vec![s.as_str()],
                serde_json::Value::Array(arr) => {
                    let mut out = Vec::with_capacity(arr.len());
                    for item in arr {
                        match item.as_str() {
                            Some(s) => out.push(s),
                            None => return Err(()),
                        }
                    }
                    out
                }
                _ => return Err(()),
            },
            None => return Err(()),
        };
        if !aud.iter().any(|a| token_auds.iter().any(|t| a == t)) {
            return Err(());
        }
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &[u8] = b"test-secret-please-ignore";

    fn enc<T: Serialize>(claims: &T) -> String {
        encode(
            &Header::new(Algorithm::HS256),
            claims,
            &EncodingKey::from_secret(SECRET),
        )
        .unwrap()
    }

    fn val() -> Validation {
        let mut v = Validation::new(Algorithm::HS256);
        v.validate_exp = false;
        v.required_spec_claims.push("exp".to_string());
        v
    }

    fn dec(token: &str) -> Result<serde_json::Value, ()> {
        decode(token, &DecodingKey::from_secret(SECRET), &val())
    }

    #[test]
    fn alg_none_rejected() {
        // 头声称 alg=none，签名仍用 HS256 密钥计算——decode 必须因算法不符拒绝
        let h = b64url_encode(b"{\"alg\":\"none\",\"typ\":\"JWT\"}");
        let c = b64url_encode(b"{\"sub\":\"u\",\"exp\":9999999999}");
        let sig = b64url_encode(&hmac_sign(SECRET, format!("{h}.{c}").as_bytes()).unwrap());
        assert!(decode(
            &format!("{h}.{c}.{sig}"),
            &DecodingKey::from_secret(SECRET),
            &val()
        )
        .is_err());
    }

    #[test]
    fn extra_segment_rejected() {
        let t = enc(&serde_json::json!({"sub":"u","exp":9999999999_i64}));
        assert!(dec(&format!("{t}.extra")).is_err());
    }

    #[test]
    fn empty_signature_rejected() {
        let t = enc(&serde_json::json!({"sub":"u","exp":9999999999_i64}));
        let base = t.rsplit_once('.').unwrap().0;
        assert!(dec(&format!("{base}.")).is_err());
    }

    #[test]
    fn aud_array_accepted() {
        let mut v = val();
        v.set_audience(&["my-client"]);
        let t = enc(&serde_json::json!({"aud":["other","my-client"],"exp":9999999999_i64}));
        assert!(decode(t.as_str(), &DecodingKey::from_secret(SECRET), &v).is_ok());
    }

    #[test]
    fn missing_exp_rejected_by_required_spec_claims() {
        assert!(!val().validate_exp);
        let t = enc(&serde_json::json!({"sub":"u"}));
        assert!(dec(&t).is_err());
        let ok = enc(&serde_json::json!({"sub":"u","exp":9999999999_i64}));
        assert!(dec(&ok).is_ok());
    }

    #[test]
    fn signature_validation_accepts_valid_and_rejects_tampered() {
        let t = enc(&serde_json::json!({"sub":"u","exp":9999999999_i64}));
        assert!(dec(&t).is_ok());
        let (signing_input, sig_b64) = t.rsplit_once('.').unwrap();
        let mut signature = b64url_decode(sig_b64).unwrap();
        signature[0] ^= 1;
        let tampered = format!("{signing_input}.{}", b64url_encode(&signature));
        assert!(dec(&tampered).is_err());
    }
}
