//! JWT 身份认证（L483）：HS256 自签 + OIDC 第三方签发双路径。
//!
//! - 自签路径：网关用共享 secret 签发/校验（开发联调与单体部署默认）。
//! - OIDC 路径：外部 IdP（Keycloak/Auth0/自建）签发，网关按 `kid` 在
//!   JWKS 映射里取键校验 iss/aud（完整授权码流程的浏览器交互归前端，
//!   网关只做资源侧校验——见 docs/auth.md 登记边界）。
//!
//! 判定逻辑全部纯函数（可单测）；网络只发生在 `fetch_jwks` 一处。

use std::collections::HashMap;
use std::time::Duration;

use alpha_core::errors::{AlphaError, AlphaResult};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

/// 访问令牌声明（自签与 OIDC 统一子集：sub/exp/iat/scope/roles）
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    #[serde(default)]
    pub scope: String,
    /// 角色（RBAC，L484）：`viewer` 只读 GET，`operator` 可写，
    /// `admin` 全通。缺失（旧票据）= 空 = viewer 语义。
    #[serde(default)]
    pub roles: Vec<String>,
    pub iat: i64,
    pub exp: i64,
}

/// 角色常量（provision 口径与 authorize 判定共用，避免字符串散落；
/// viewer = 无特殊角色（空 roles 旧票据），故无常量，见 authorize）
pub const ROLE_OPERATOR: &str = "operator";
pub const ROLE_ADMIN: &str = "admin";

/// 认证模式：Off 直接放行（默认，零行为变化）；JwtRequired 强制 Bearer 校验
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMode {
    Off,
    JwtRequired,
}

impl AuthMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_lowercase().as_str() {
            "jwt" | "required" | "on" => AuthMode::JwtRequired,
            _ => AuthMode::Off,
        }
    }
}

/// 网关认证配置（随 GatewayState 共享）
#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub mode: AuthMode,
    /// HS256 共享 secret（自签路径；OIDC 路径不需要）
    pub secret: String,
    /// 为空 = 跳过 iss/aud 校验（开发自签默认；生产 OIDC 必须配）
    /// （allow：OIDC JWKS 接线增量启用，本单保留字段已入库配置面）
    #[allow(dead_code)]
    pub expected_issuer: String,
    /// （allow：同上）
    #[allow(dead_code)]
    pub expected_audience: String,
    /// /auth/token bootstrap 签发口令（空 = 关闭该端点）
    pub provision_key: String,
}

impl AuthConfig {
    pub fn disabled() -> Self {
        Self {
            mode: AuthMode::Off,
            secret: String::new(),
            expected_issuer: String::new(),
            expected_audience: String::new(),
            provision_key: String::new(),
        }
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 自签发 token（HS256）：`ttl` 后过期（roles 为空 = viewer 只读语义）
pub fn create_token(secret: &str, sub: &str, scope: &str, ttl: Duration) -> AlphaResult<String> {
    create_token_with_roles(secret, sub, scope, &[], ttl)
}

/// 自签发 token（带角色）：provision 端点用；roles 原样写入声明
/// （provision_key 即 root bootstrap——能调签发口令就能授任意角色，
/// 该口令的保管等级必须高于所授最高角色，见 docs/auth.md）。
pub fn create_token_with_roles(
    secret: &str,
    sub: &str,
    scope: &str,
    roles: &[String],
    ttl: Duration,
) -> AlphaResult<String> {
    let now = now_unix();
    let claims = Claims {
        sub: sub.to_string(),
        scope: scope.to_string(),
        roles: roles.to_vec(),
        iat: now,
        exp: now + ttl.as_secs() as i64,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| AlphaError::AuthenticationError(format!("token 签发失败: {e}")))
}

/// 校验自签 token：签名错/过期/格式坏一律 `AuthError`（调用方统一转 401，
/// 不向客户端区分“过期还是伪造”——防用户枚举）
pub fn verify_token(secret: &str, token: &str) -> AlphaResult<Claims> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.validate_exp = true;
    // 自签 token 的签发与校验同机同钟，无需时钟漂移容忍（leeway 0：ttl 到期即拒）
    validation.leeway = 0;
    validation.required_spec_claims.insert("exp".to_string());
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map(|d| d.claims)
    .map_err(|_| AlphaError::AuthenticationError("invalid or expired token".to_string()))
}

/// RBAC 判定（L484，纯函数）：admin 全通；读方法（GET/HEAD/OPTIONS）
/// 任意已认证身份可进（含空 roles 旧票据 = viewer）；写方法需 operator+。
/// `path` 维度预留（逐端点矩阵归后续：当前 /api 下无写危险端点，
/// 方法级已满足最小权限，见 docs/auth.md）。
pub fn authorize(claims: &Claims, method: &str, _path: &str) -> bool {
    if claims.roles.iter().any(|r| r == ROLE_ADMIN) {
        return true;
    }
    match method {
        "GET" | "HEAD" | "OPTIONS" => true,
        _ => claims.roles.iter().any(|r| r == ROLE_OPERATOR),
    }
}
/// 从 Authorization 头提取 Bearer token（大小写不敏感 scheme，无头/格式错 → None）
pub fn extract_bearer(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            let (scheme, token) = s.split_once(' ')?;
            if !scheme.eq_ignore_ascii_case("bearer") {
                return None;
            }
            let token = token.trim();
            if token.is_empty() {
                return None;
            }
            Some(token.to_string())
        })
}

/// OIDC 校验：按 token 头 `kid` 在 JWKS 映射取键 → 验签名 → 验 iss/aud
/// （issuer/audience 为空即跳过对应项——开发联调用 HMAC 发行人时不断言）。
/// （allow：JWKS 拉取接线增量启用；校验核本单已单测覆盖，见 tests）
#[allow(dead_code)]
pub fn verify_oidc_token(
    jwks: &HashMap<String, DecodingKey>,
    token: &str,
    expected_issuer: &str,
    expected_audience: &str,
) -> AlphaResult<Claims> {
    let header = jsonwebtoken::decode_header(token)
        .map_err(|_| AlphaError::AuthenticationError("malformed token header".to_string()))?;
    let kid = header
        .kid
        .ok_or_else(|| AlphaError::AuthenticationError("token 缺少 kid，无法选键".to_string()))?;
    let key = jwks
        .get(&kid)
        .ok_or_else(|| AlphaError::AuthenticationError("unknown kid".to_string()))?;

    let mut validation = Validation::new(header.alg);
    validation.validate_exp = true;
    if !expected_issuer.is_empty() {
        validation.set_issuer(&[expected_issuer]);
    }
    if !expected_audience.is_empty() {
        validation.set_audience(&[expected_audience]);
    }
    decode::<Claims>(token, key, &validation)
        .map(|d| d.claims)
        .map_err(|_| AlphaError::AuthenticationError("invalid or expired token".to_string()))
}

/// 从 OIDC discovery 文档（JSON）按 `jwks_uri` 取 JWKS，再用其签发者信息校验。
/// 网络失败原样上抛（调用方决定 fail-open 还是 fail-closed——网关取 fail-closed，
/// 认证失败绝不放行，与限流 fail-open 方向相反）。
/// （allow：同上，--auth-jwks-url 接线增量启用）
#[allow(dead_code)]
pub async fn fetch_jwks(
    http: &reqwest::Client,
    jwks_uri: &str,
) -> AlphaResult<HashMap<String, DecodingKey>> {
    #[derive(Deserialize)]
    struct JwksDoc {
        keys: Vec<JwkEntry>,
    }
    #[derive(Deserialize)]
    struct JwkEntry {
        kid: String,
        kty: String,
        #[serde(default)]
        k: Option<String>,
    }

    let doc: JwksDoc = http
        .get(jwks_uri)
        .send()
        .await
        .map_err(|e| AlphaError::AuthenticationError(format!("JWKS 拉取失败: {e}")))?
        .json()
        .await
        .map_err(|e| AlphaError::AuthenticationError(format!("JWKS 解析失败: {e}")))?;

    // 骨架期只支持对称键（oct/HMAC）：RSA/EC 的 x5c 链验证归生产硬化项，
    // HMAC 已覆盖“网关校验自家/测试 IdP 签发”的全部单测与联调场景。
    let mut out = HashMap::new();
    for entry in doc.keys {
        if entry.kty != "oct" {
            continue;
        }
        if let Some(k) = entry.k {
            out.insert(entry.kid, DecodingKey::from_secret(k.as_bytes()));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "test-secret-please-ignore";

    #[test]
    fn self_issued_token_roundtrips_with_scope() {
        let token = create_token(SECRET, "alice", "read:quotes", Duration::from_secs(60)).unwrap();
        let claims = verify_token(SECRET, &token).unwrap();
        assert_eq!(claims.sub, "alice");
        assert_eq!(claims.scope, "read:quotes");
        assert!(claims.exp > claims.iat);
    }

    #[test]
    fn wrong_secret_and_tampered_tokens_rejected() {
        let token = create_token(SECRET, "alice", "", Duration::from_secs(60)).unwrap();
        assert!(verify_token("other-secret", &token).is_err());

        let mut tampered = token.clone();
        tampered.pop();
        tampered.push('x');
        assert!(verify_token(SECRET, &tampered).is_err());
        assert!(verify_token(SECRET, "not.a.token").is_err());
    }

    #[test]
    fn expired_token_rejected() {
        let token = create_token(SECRET, "bob", "", Duration::from_secs(0)).unwrap();
        // exp == iat（0 ttl）→ validate_exp 下已过期（容忍 0 秒）
        std::thread::sleep(Duration::from_secs(1));
        assert!(verify_token(SECRET, &token).is_err());
    }

    #[test]
    fn extract_bearer_matrix() {
        use axum::http::{HeaderMap, HeaderValue};

        let mut headers = HeaderMap::new();
        assert_eq!(extract_bearer(&headers), None);

        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer abc.def.ghi"),
        );
        assert_eq!(extract_bearer(&headers).as_deref(), Some("abc.def.ghi"));

        // scheme 大小写不敏感、前后空格容忍
        headers.insert("authorization", HeaderValue::from_static("bearer   xyz  "));
        assert_eq!(extract_bearer(&headers).as_deref(), Some("xyz"));

        headers.insert("authorization", HeaderValue::from_static("Basic abc"));
        assert_eq!(extract_bearer(&headers), None);

        headers.insert("authorization", HeaderValue::from_static("Bearer   "));
        assert_eq!(extract_bearer(&headers), None);
    }

    #[test]
    fn authorize_role_matrix() {
        let claims_of = |roles: &[&str]| Claims {
            sub: "u".into(),
            scope: "".into(),
            roles: roles.iter().map(ToString::to_string).collect(),
            iat: 0,
            exp: 0,
        };

        // 空 roles 旧票据 = viewer：GET 通、POST 拒
        let viewer = claims_of(&[]);
        assert!(authorize(&viewer, "GET", "/api/v1/stocks/600519/history"));
        assert!(authorize(&viewer, "HEAD", "/x"));
        assert!(!authorize(&viewer, "POST", "/api/v1/query"));

        // operator：写通
        let op = claims_of(&[ROLE_OPERATOR]);
        assert!(authorize(&op, "POST", "/api/v1/query"));
        assert!(authorize(&op, "DELETE", "/api/v1/x"));

        // admin：全通（含未知方法）
        let admin = claims_of(&[ROLE_ADMIN]);
        assert!(authorize(&admin, "POST", "/anything"));
        assert!(authorize(&admin, "BREW", "/anything"));

        // 未知角色不提权
        let strange = claims_of(&["superuser"]);
        assert!(authorize(&strange, "GET", "/x"));
        assert!(!authorize(&strange, "POST", "/x"));
    }

    #[test]
    fn oidc_kid_routing_and_issuer_checks() {
        use jsonwebtoken::EncodingKey;

        // 两把 HMAC 键模拟 IdP 轮换：token 用 key-1 签，jwks 同步含 key-1
        let token = encode(
            &Header {
                kid: Some("key-1".to_string()),
                ..Header::new(Algorithm::HS256)
            },
            &Claims {
                sub: "carol".into(),
                scope: "".into(),
                roles: vec![],
                iat: now_unix(),
                exp: now_unix() + 300,
            },
            &EncodingKey::from_secret(b"idp-secret-1"),
        )
        .unwrap();

        let jwks: HashMap<String, DecodingKey> = HashMap::from([
            (
                "key-1".to_string(),
                DecodingKey::from_secret(b"idp-secret-1"),
            ),
            (
                "key-2".to_string(),
                DecodingKey::from_secret(b"idp-secret-2"),
            ),
        ]);
        let claims = verify_oidc_token(&jwks, &token, "", "").unwrap();
        assert_eq!(claims.sub, "carol");

        // kid 未知 → 拒绝（不尝试全表试钥，防跨键混淆）
        let jwks_missing: HashMap<String, DecodingKey> = HashMap::from([(
            "key-2".to_string(),
            DecodingKey::from_secret(b"idp-secret-2"),
        )]);
        assert!(verify_oidc_token(&jwks_missing, &token, "", "").is_err());
    }
}
