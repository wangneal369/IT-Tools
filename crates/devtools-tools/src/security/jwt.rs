//! #25 JWT 解析与签发（JwtTool）。
//!
//! 零密钥解析 header & claims + 时效徽章；离线验签与签发 HS256/384/512、
//! RS/PS/ES/EdDSA；所有密钥材料包装在 [`Zeroizing`] 中以确保内存清零。
//!
//! 本工具声明 `sensitive_output = true`（见 `devtools_core::ToolId::JwtTool`）。

use jsonwebtoken::dangerous::insecure_decode;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde_json::Value;
use zeroize::Zeroizing;

use crate::{ToolError, ToolErrorKind};

/// Claims 时效状态徽章。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenStatus {
    Valid,
    Expired,
    NotYetValid,
    NoExp,
}

impl TokenStatus {
    pub const fn label_zh(self) -> &'static str {
        match self {
            Self::Valid => "有效",
            Self::Expired => "已过期",
            Self::NotYetValid => "尚未生效",
            Self::NoExp => "无过期时间",
        }
    }
}

/// 零密钥解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedJwt {
    pub header: Value,
    pub claims: Value,
    pub status: TokenStatus,
    pub algorithm: String,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn status_from_claims(claims: &Value) -> TokenStatus {
    let now = now_secs();
    let num = |key: &str| claims.get(key).and_then(Value::as_i64);
    match num("nbf") {
        Some(nbf) if nbf > now => return TokenStatus::NotYetValid,
        _ => {}
    }
    match num("exp") {
        Some(exp) if exp < now => TokenStatus::Expired,
        Some(_) => TokenStatus::Valid,
        None => TokenStatus::NoExp,
    }
}

/// 零密钥解析 JWT 的 header 与 claims（不做任何签名校验）。
pub fn decode_without_verification(token: &str) -> Result<DecodedJwt, ToolError> {
    let data: jsonwebtoken::TokenData<Value> = insecure_decode(token)
        .map_err(|e| ToolError::new(ToolErrorKind::InvalidSyntax, format!("JWT 格式无效：{e}")))?;
    let header = serde_json::to_value(&data.header)
        .map_err(|e| ToolError::new(ToolErrorKind::Internal, format!("header 序列化失败：{e}")))?;
    let algorithm = header
        .get("alg")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    Ok(DecodedJwt {
        header,
        status: status_from_claims(&data.claims),
        claims: data.claims,
        algorithm,
    })
}

/// 离线验签。
///
/// `secret` 为 HMAC 共享密钥（HS*），或非 HMAC 算法的 PEM 公钥 / 密钥字节。
/// 参数以 [`Zeroizing`] 包装，确保使用后立即清零。
pub fn verify_jwt(
    token: &str,
    secret: &[u8],
    algorithm: Algorithm,
) -> Result<DecodedJwt, ToolError> {
    let secret = Zeroizing::new(secret.to_vec());
    let validation = Validation::new(algorithm);
    let key = build_decoding_key(algorithm, &secret)?;
    let data: jsonwebtoken::TokenData<Value> = decode::<Value>(token, &key, &validation)
        .map_err(|e| ToolError::new(ToolErrorKind::InvalidSyntax, format!("JWT 验签失败：{e}")))?;
    let header = serde_json::to_value(&data.header)
        .map_err(|e| ToolError::new(ToolErrorKind::Internal, format!("header 序列化失败：{e}")))?;
    Ok(DecodedJwt {
        header,
        status: status_from_claims(&data.claims),
        claims: data.claims,
        algorithm: format!("{algorithm:?}"),
    })
}

fn build_decoding_key(algorithm: Algorithm, secret: &[u8]) -> Result<DecodingKey, ToolError> {
    match algorithm {
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
            DecodingKey::from_secret(secret).map_err(map_key_error)
        }
        Algorithm::RS256
        | Algorithm::RS384
        | Algorithm::RS512
        | Algorithm::PS256
        | Algorithm::PS384
        | Algorithm::PS512 => DecodingKey::from_rsa_pem(secret).map_err(map_key_error),
        Algorithm::ES256 | Algorithm::ES384 => {
            DecodingKey::from_ec_pem(secret).map_err(map_key_error)
        }
        Algorithm::EdDSA => DecodingKey::from_ed_pem(secret).map_err(map_key_error),
        _ => Err(ToolError::new(
            ToolErrorKind::UnsupportedInput,
            format!("不支持的算法：{algorithm:?}"),
        )),
    }
}

fn build_encoding_key(algorithm: Algorithm, secret: &[u8]) -> Result<EncodingKey, ToolError> {
    match algorithm {
        Algorithm::HS256 | Algorithm::HS384 | Algorithm::HS512 => {
            EncodingKey::from_secret(secret).map_err(map_key_error)
        }
        Algorithm::RS256
        | Algorithm::RS384
        | Algorithm::RS512
        | Algorithm::PS256
        | Algorithm::PS384
        | Algorithm::PS512 => EncodingKey::from_rsa_pem(secret).map_err(map_key_error),
        Algorithm::ES256 | Algorithm::ES384 => {
            EncodingKey::from_ec_pem(secret).map_err(map_key_error)
        }
        Algorithm::EdDSA => EncodingKey::from_ed_pem(secret).map_err(map_key_error),
        _ => Err(ToolError::new(
            ToolErrorKind::UnsupportedInput,
            format!("不支持的算法：{algorithm:?}"),
        )),
    }
}

fn map_key_error(e: jsonwebtoken::errors::Error) -> ToolError {
    ToolError::new(ToolErrorKind::InvalidSyntax, format!("密钥格式无效：{e}"))
}

/// 使用 claims JSON 签发 JWT。
///
/// `secret` 在函数内被包装为 [`Zeroizing`]，签名完成后即清零。
pub fn sign_jwt(
    claims_json: &str,
    secret: &[u8],
    algorithm: Algorithm,
) -> Result<String, ToolError> {
    let claims: Value = serde_json::from_str(claims_json).map_err(|e| {
        ToolError::new(
            ToolErrorKind::InvalidSyntax,
            format!("Claims JSON 无效：{e}"),
        )
    })?;
    let secret = Zeroizing::new(secret.to_vec());
    let key = build_encoding_key(algorithm, &secret)?;
    let header = Header::new(algorithm);
    encode(&header, &claims, &key)
        .map_err(|e| ToolError::new(ToolErrorKind::Internal, format!("JWT 签发失败：{e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_secret_decode_and_status_badges() {
        // 用合法 HS256 头签发，再零密钥解析（不校验签名）。
        let token = sign_jwt(r#"{"sub":"123"}"#, b"k", Algorithm::HS256).unwrap();
        let decoded = decode_without_verification(&token).unwrap();
        assert_eq!(decoded.claims["sub"], "123");
        assert_eq!(decoded.status, TokenStatus::NoExp);
        assert_eq!(decoded.status.label_zh(), "无过期时间");

        let bad = decode_without_verification("not-a-jwt").unwrap_err();
        assert_eq!(bad.kind, ToolErrorKind::InvalidSyntax);
    }

    #[test]
    fn hs256_sign_verify_round_trip() {
        let claims = r#"{"sub":"user1","exp":4102444800,"iat":1700000000}"#;
        let secret = b"a-very-secret-key-for-tests";
        let token = sign_jwt(claims, secret, Algorithm::HS256).unwrap();
        let verified = verify_jwt(&token, secret, Algorithm::HS256).unwrap();
        assert_eq!(verified.claims["sub"], "user1");
        assert_eq!(verified.status, TokenStatus::Valid);
        let decoded = decode_without_verification(&token).unwrap();
        assert_eq!(decoded.algorithm, "HS256");
    }

    #[test]
    fn wrong_secret_or_algorithm_fails() {
        let token = sign_jwt(
            r#"{"sub":"x","exp":4102444800}"#,
            b"secret",
            Algorithm::HS256,
        )
        .unwrap();
        assert!(verify_jwt(&token, b"wrong", Algorithm::HS256).is_err());
        assert!(verify_jwt(&token, b"secret", Algorithm::HS384).is_err());
    }

    #[test]
    fn expired_and_not_yet_valid_badges() {
        let expired = sign_jwt(r#"{"exp":1,"sub":"x"}"#, b"s", Algorithm::HS256).unwrap();
        // exp 已过会被 Validation 拒绝；用零密钥解析检查徽章。
        assert_eq!(
            decode_without_verification(&expired).unwrap().status,
            TokenStatus::Expired
        );
        let future = sign_jwt(
            r#"{"nbf":4102444800,"exp":4102444801,"sub":"x"}"#,
            b"s",
            Algorithm::HS256,
        )
        .unwrap();
        assert_eq!(
            decode_without_verification(&future).unwrap().status,
            TokenStatus::NotYetValid
        );
    }
}
