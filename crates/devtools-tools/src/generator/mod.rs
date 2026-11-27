//! 分类 4：生成器（工具 #14–#16）。

pub mod password;
pub mod qr;
pub mod uuid_ulid;

pub use password::{
    PasswordOptions, PasswordTokenRequest, PasswordTokenResponse, SecretKind, SecretOutput,
    StrengthReport, TokenEncoding,
};
pub use qr::{
    QrContent, QrDecodeRequest, QrDecodeResponse, QrErrorCorrection, QrGenerateRequest,
    QrGenerateResponse, WifiConfig, WifiSecurity,
};
pub use uuid_ulid::{IdInfo, IdKind, IdOutputFormat, UuidUlidRequest, UuidUlidResponse};

use devtools_core::ToolId;

/// 本分类实现的工具集合。
pub const TOOL_IDS: [ToolId; 3] = [
    ToolId::UuidUlidGenerator,
    ToolId::PasswordTokenGenerator,
    ToolId::QrCodeGenerator,
];
