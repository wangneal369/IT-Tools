//! 分类 2：编解码（工具 #6–#10）。

pub mod base64;
pub mod gzip;
pub mod html_unicode;
pub mod string_escape;
pub mod url;

pub use base64::{Base64CodecRequest, Base64CodecResponse, Base64Mode, Base64Payload, ImageInfo};
pub use gzip::{GzipAction, GzipCompressRequest, GzipCompressResponse, GzipTransport};
pub use html_unicode::{
    HtmlEntityUnicodeRequest, HtmlEntityUnicodeResponse, HtmlUnicodeMode, UnicodeEscapeStyle,
};
pub use string_escape::{EscapeDirection, EscapeStyle, StringEscapeRequest, StringEscapeResponse};
pub use url::{QueryParam, UrlCodecRequest, UrlCodecResponse, UrlMode, UrlParts, UrlTarget};

use devtools_core::ToolId;

/// 本分类实现的工具集合。
pub const TOOL_IDS: [ToolId; 5] = [
    ToolId::Base64Codec,
    ToolId::UrlCodec,
    ToolId::HtmlEntityUnicode,
    ToolId::StringEscape,
    ToolId::GzipCompress,
];
