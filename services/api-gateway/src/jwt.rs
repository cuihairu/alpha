//! 纯 HMAC-SHA256 JWT 实现（HS256 only）。
//!
//! 替换 jsonwebtoken 以摆脱 p256/rsa/elliptic-curve/crypto-bigint 生态
//! （VULN-ALPHA-005/006）。本项目认证面只用 HS256 对称密钥与 oct JWKS，
//! 故无需 EC/RSA；接口形状对齐 jsonwebtoken 以最小化 auth.rs 改动面。

use base64::Engine as _;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

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
pub fn encode<T: Serialize>(
    header: &Header,
    claims: &T,
    key: &EncodingKey,
) -> Result<String, ()> {
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
pub fn decode(token: &str, key: &DecodingKey, validation: &Validation) -> Result<serde_json::Value, ()> {
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
    if expected != sig {
        return Err(());
    }

    let claims_bytes = b64url_decode(claims_b64)?;
    let value: serde_json::Value = serde_json::from_slice(&claims_bytes).map_err(|_| ())?;
    if validation.validate_exp {
        let exp = value
            .get("exp")
            .and_then(|v| v.as_i64())
            .ok_or(())?;
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
        let token_aud = value.get("aud").and_then(|v| v.as_str()).ok_or(())?;
        if !aud.iter().any(|a| a == token_aud) {
            return Err(());
        }
    }
    Ok(value)
}
