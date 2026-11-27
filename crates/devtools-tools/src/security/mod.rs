//! 分类 7「安全网络」：#25–#28 纯计算内核。

pub mod hash_hmac;
pub mod ipv4_cidr;
pub mod jwt;
pub mod x509;

pub use hash_hmac::*;
pub use ipv4_cidr::*;
pub use jwt::*;
pub use x509::*;

use devtools_core::ToolId;

/// 本分类实现的工具集合。
pub const TOOL_IDS: [ToolId; 4] = [
    ToolId::JwtTool,
    ToolId::HashHmac,
    ToolId::X509Parser,
    ToolId::Ipv4CidrCalculator,
];
