//! #10 GZip 压缩与解压。
//!
//! Base64 / Hex 传输、压缩级别 1–9、压缩率统计；解压严格执行 GZip Bomb
//! 硬上限：解压后 32 MB 或 100× 膨胀比，任一命中即中止并返回
//! `ToolError::SizeLimitExceeded`。

use std::io::{Read, Write};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use flate2::Compression;
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;

use crate::util::{check_cancel, invalid_syntax, size_limit, unsupported};

use devtools_core::{CancelFlag, ToolError};

/// 解压后硬上限（32 MB）。
pub const MAX_UNCOMPRESSED_BYTES: usize = 32 * 1024 * 1024;
/// 最大膨胀比。
pub const MAX_EXPANSION_RATIO: usize = 100;
/// 压缩输入上限（32 MB 文本）。
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
/// 待解压的压缩字节上限（64 MB，覆盖不可压缩的 32 MB 载荷）。
pub const MAX_COMPRESSED_BYTES: usize = 64 * 1024 * 1024;

/// 操作方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GzipAction {
    Compress,
    Decompress,
}

/// 传输编码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GzipTransport {
    Base64,
    Hex,
}

/// 单次请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GzipCompressRequest {
    pub action: GzipAction,
    pub input: String,
    pub transport: GzipTransport,
    /// 压缩级别 1–9（仅压缩生效）。
    pub level: u32,
}

/// 结果。
#[derive(Debug, Clone, PartialEq)]
pub struct GzipCompressResponse {
    pub output: String,
    /// 原始压缩前 / 解压前字节数。
    pub input_bytes: usize,
    /// 压缩后 / 解压后字节数。
    pub output_bytes: usize,
    /// 压缩率百分比（输出 / 输入 × 100）。
    pub ratio: f64,
    /// 解压场景下的解压后字节数。
    pub expanded_bytes: Option<u64>,
}

/// 执行压缩 / 解压。
pub fn run(
    req: &GzipCompressRequest,
    cancel: Option<&CancelFlag>,
) -> Result<GzipCompressResponse, ToolError> {
    match req.action {
        GzipAction::Compress => compress(req, cancel),
        GzipAction::Decompress => decompress(req, cancel),
    }
}

fn compress(
    req: &GzipCompressRequest,
    cancel: Option<&CancelFlag>,
) -> Result<GzipCompressResponse, ToolError> {
    if !(1..=9).contains(&req.level) {
        return Err(unsupported(format!(
            "压缩级别必须在 1–9 之间，实际为 {}",
            req.level
        )));
    }
    check_cancel(cancel)?;
    let input = req.input.as_bytes();
    if input.len() > MAX_INPUT_BYTES {
        return Err(size_limit("压缩输入超过 32 MB 上限"));
    }
    let mut encoder = GzEncoder::new(Vec::new(), Compression::new(req.level));
    encoder
        .write_all(input)
        .map_err(|err| invalid_syntax(format!("GZip 压缩失败：{err}")))?;
    check_cancel(cancel)?;
    let compressed = encoder
        .finish()
        .map_err(|err| invalid_syntax(format!("GZip 压缩失败：{err}")))?;
    let output = match req.transport {
        GzipTransport::Base64 => STANDARD.encode(&compressed),
        GzipTransport::Hex => encode_hex(&compressed),
    };
    Ok(GzipCompressResponse {
        output,
        input_bytes: input.len(),
        output_bytes: compressed.len(),
        ratio: percent(compressed.len(), input.len()),
        expanded_bytes: None,
    })
}

fn decompress(
    req: &GzipCompressRequest,
    cancel: Option<&CancelFlag>,
) -> Result<GzipCompressResponse, ToolError> {
    check_cancel(cancel)?;
    if req.input.len() > MAX_COMPRESSED_BYTES * 4 / 3 + 4 {
        return Err(size_limit("压缩输入超过传输上限"));
    }
    let compressed = match req.transport {
        GzipTransport::Base64 => crate::encode_decode::base64::decode_to_bytes(req.input.trim())?,
        GzipTransport::Hex => decode_hex(req.input.trim())?,
    };
    if compressed.is_empty() {
        return Err(invalid_syntax("压缩输入为空"));
    }
    if compressed.len() > MAX_COMPRESSED_BYTES {
        return Err(size_limit("压缩输入超过 64 MB 上限"));
    }

    let mut decoder = GzDecoder::new(compressed.as_slice());
    let mut buffer = [0u8; 64 * 1024];
    let mut output = Vec::new();
    let mut total = 0usize;
    loop {
        check_cancel(cancel)?;
        let read = decoder
            .read(&mut buffer)
            .map_err(|err| invalid_syntax(format!("GZip 解压失败：{err}")))?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read);
        if total > MAX_UNCOMPRESSED_BYTES {
            return Err(size_limit(format!(
                "解压后超过 {MAX_UNCOMPRESSED_BYTES} 字节硬上限（疑似 GZip Bomb）"
            ))
            .with_hint("请确认数据来源可信，或分段处理"));
        }
        if total > compressed.len().saturating_mul(MAX_EXPANSION_RATIO) {
            return Err(size_limit(format!(
                "解压膨胀比超过 {MAX_EXPANSION_RATIO}× 硬上限（疑似 GZip Bomb）"
            ))
            .with_hint("请确认数据来源可信"));
        }
        output.extend_from_slice(&buffer[..read]);
    }

    let text = String::from_utf8(output).map_err(|_| {
        unsupported("解压结果不是有效的 UTF-8 文本").with_hint("二进制内容请使用 Base64 工具处理")
    })?;
    Ok(GzipCompressResponse {
        output: text,
        input_bytes: compressed.len(),
        output_bytes: total,
        ratio: percent(total, compressed.len()),
        expanded_bytes: Some(total as u64),
    })
}

fn percent(part: usize, whole: usize) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    ((part as f64 / whole as f64) * 10000.0).round() / 100.0
}

fn encode_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn decode_hex(input: &str) -> Result<Vec<u8>, ToolError> {
    let cleaned: String = input
        .trim()
        .trim_start_matches("0x")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if !cleaned.len().is_multiple_of(2) {
        return Err(invalid_syntax("Hex 输入长度必须为偶数"));
    }
    let mut out = Vec::with_capacity(cleaned.len() / 2);
    for pair in cleaned.as_bytes().chunks(2) {
        let text = std::str::from_utf8(pair).map_err(|_| invalid_syntax("Hex 输入不是合法文本"))?;
        let byte = u8::from_str_radix(text, 16)
            .map_err(|_| invalid_syntax(format!("非法 Hex 字符：{text}")))?;
        out.push(byte);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use devtools_core::ToolErrorKind;

    fn request(
        action: GzipAction,
        input: &str,
        transport: GzipTransport,
        level: u32,
    ) -> GzipCompressRequest {
        GzipCompressRequest {
            action,
            input: input.to_string(),
            transport,
            level,
        }
    }

    #[test]
    fn text_round_trip_base64() {
        let original = "hello 中文 gzip ".repeat(20);
        let compressed = run(
            &request(GzipAction::Compress, &original, GzipTransport::Base64, 6),
            None,
        )
        .unwrap();
        assert!(compressed.ratio < 100.0);
        let restored = run(
            &request(
                GzipAction::Decompress,
                &compressed.output,
                GzipTransport::Base64,
                6,
            ),
            None,
        )
        .unwrap();
        assert_eq!(restored.output, original);
        assert_eq!(restored.expanded_bytes, Some(original.len() as u64));
    }

    #[test]
    fn hex_transport_round_trip() {
        let original = "hex transport test";
        let compressed = run(
            &request(GzipAction::Compress, original, GzipTransport::Hex, 9),
            None,
        )
        .unwrap();
        assert!(compressed.output.chars().all(|c| c.is_ascii_hexdigit()));
        let restored = run(
            &request(
                GzipAction::Decompress,
                &compressed.output,
                GzipTransport::Hex,
                9,
            ),
            None,
        )
        .unwrap();
        assert_eq!(restored.output, original);
    }

    #[test]
    fn invalid_level_is_rejected() {
        let err = run(
            &request(GzipAction::Compress, "x", GzipTransport::Base64, 0),
            None,
        )
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
        let err = run(
            &request(GzipAction::Compress, "x", GzipTransport::Base64, 10),
            None,
        )
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
    }

    #[test]
    fn invalid_base64_is_rejected() {
        let err = run(
            &request(GzipAction::Decompress, "!!!", GzipTransport::Base64, 6),
            None,
        )
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
    }

    #[test]
    fn high_ratio_bomb_is_blocked_by_expansion_limit() {
        // 200 KB 全零压缩到数百字节，膨胀比远超 100×。
        let payload = "0".repeat(200 * 1024);
        let compressed = run(
            &request(GzipAction::Compress, &payload, GzipTransport::Base64, 9),
            None,
        )
        .unwrap();
        let err = run(
            &request(
                GzipAction::Decompress,
                &compressed.output,
                GzipTransport::Base64,
                9,
            ),
            None,
        )
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::SizeLimitExceeded);
        assert!(err.message.contains("膨胀比"), "{}", err.message);
        assert!(err.hint.is_some());
    }

    #[test]
    fn uncompressed_size_bomb_is_blocked_by_32mb_limit() {
        // 33 MB 的 deflate “stored”（不压缩）块：膨胀比约 1×，只有 32 MB 上限能拦住。
        let gzip = stored_gzip(MAX_UNCOMPRESSED_BYTES + 1024 * 1024);
        let input = STANDARD.encode(&gzip);
        let err = run(
            &request(GzipAction::Decompress, &input, GzipTransport::Base64, 6),
            None,
        )
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::SizeLimitExceeded);
        assert!(err.message.contains("32"), "{}", err.message);
    }

    #[test]
    fn cancelled_flag_aborts_before_work() {
        let flag = CancelFlag::new();
        flag.cancel();
        let err = run(
            &request(GzipAction::Compress, "data", GzipTransport::Base64, 6),
            Some(&flag),
        )
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Cancelled);
    }

    /// 构造仅含 stored 块的 gzip 流（无需压缩即可测试解压上限）。
    fn stored_gzip(total: usize) -> Vec<u8> {
        use flate2::Crc;
        let mut crc = Crc::new();
        let zeros = vec![0u8; 64 * 1024];
        let mut out = vec![0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff];
        let mut remaining = total;
        while remaining > 0 {
            let len = remaining.min(0xFFFF);
            out.push(0x00); // BFINAL=0, BTYPE=stored
            out.extend_from_slice(&(len as u16).to_le_bytes());
            out.extend_from_slice(&(!(len as u16)).to_le_bytes());
            let chunk = &zeros[..len];
            out.extend_from_slice(chunk);
            crc.update(chunk);
            remaining -= len;
        }
        // 最后一个块置 BFINAL：简单起见重扫不可行，改为在末尾追加一个 final 空 stored 块。
        out.extend_from_slice(&[0x01, 0x00, 0x00, 0xff, 0xff]);
        out.extend_from_slice(&crc.sum().to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out
    }

    #[test]
    fn ratio_statistics_are_reported() {
        let original = "a".repeat(10_000);
        let compressed = run(
            &request(GzipAction::Compress, &original, GzipTransport::Base64, 9),
            None,
        )
        .unwrap();
        assert!(compressed.ratio > 0.0 && compressed.ratio < 10.0);
        assert_eq!(compressed.input_bytes, 10_000);
        assert!(compressed.output_bytes > 0);
    }
}
