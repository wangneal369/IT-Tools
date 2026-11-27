//! #6 Base64 编解码。
//!
//! 文本（标准 / URL-safe、`=` 填充开关、逐行）与二进制/图片（Data URI 与
//! 尺寸元数据）双向转换；单文件硬上限 20 MB。

use std::io::Cursor;

use base64::Engine as _;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};

use crate::util::{invalid_syntax, size_limit, unsupported};

use devtools_core::ToolError;

/// 单文件字节硬上限（20 MB）。
pub const MAX_BYTES: usize = 20 * 1024 * 1024;

/// 编解码方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base64Mode {
    Encode,
    Decode,
}

/// 输入载荷：文本或原始字节。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Base64Payload {
    Text(String),
    Bytes(Vec<u8>),
}

/// 单次编解码请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base64CodecRequest {
    pub mode: Base64Mode,
    pub payload: Base64Payload,
    /// 使用 URL-safe 字母表（`-` / `_`）。
    pub url_safe: bool,
    /// 输出是否保留 `=` 填充（解码时自动容错）。
    pub padding: bool,
    /// 逐行独立处理（空行原样保留）。
    pub line_by_line: bool,
}

/// 图片元数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInfo {
    pub width: u32,
    pub height: u32,
    /// 格式名，例如 `png`。
    pub format: String,
    pub mime: String,
}

/// 编解码结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base64CodecResponse {
    /// 文本结果（解码且为 UTF-8 文本时存在）。
    pub text: Option<String>,
    /// 二进制结果（解码时始终存在）。
    pub bytes: Option<Vec<u8>>,
    /// Data URI（编码时基于嗅探的 MIME 生成，解码 Data URI 时回显）。
    pub data_url: Option<String>,
    pub image: Option<ImageInfo>,
    pub input_bytes: usize,
    pub output_bytes: usize,
}

/// 执行 Base64 编解码。
pub fn run(req: &Base64CodecRequest) -> Result<Base64CodecResponse, ToolError> {
    match req.mode {
        Base64Mode::Encode => encode(req),
        Base64Mode::Decode => decode(req),
    }
}

fn engine(url_safe: bool, padding: bool) -> &'static base64::engine::GeneralPurpose {
    match (url_safe, padding) {
        (false, true) => &STANDARD,
        (false, false) => &STANDARD_NO_PAD,
        (true, true) => &URL_SAFE,
        (true, false) => &URL_SAFE_NO_PAD,
    }
}

fn encode(req: &Base64CodecRequest) -> Result<Base64CodecResponse, ToolError> {
    let engine = engine(req.url_safe, req.padding);
    match &req.payload {
        Base64Payload::Text(text) if req.line_by_line => {
            if text.len() > MAX_BYTES {
                return Err(size_limit("输入超过 20 MB 上限"));
            }
            let mut out = Vec::new();
            for line in text.lines() {
                out.push(engine.encode(line.as_bytes()));
            }
            let mut joined = out.join("\n");
            if text.ends_with('\n') {
                joined.push('\n');
            }
            Ok(Base64CodecResponse {
                output_bytes: joined.len(),
                input_bytes: text.len(),
                text: Some(joined),
                bytes: None,
                data_url: None,
                image: None,
            })
        }
        Base64Payload::Text(text) => {
            if text.len() > MAX_BYTES {
                return Err(size_limit("输入超过 20 MB 上限"));
            }
            encode_bytes(text.as_bytes(), req.url_safe, req.padding)
        }
        Base64Payload::Bytes(bytes) => encode_bytes(bytes, req.url_safe, req.padding),
    }
}

fn encode_bytes(
    bytes: &[u8],
    url_safe: bool,
    padding: bool,
) -> Result<Base64CodecResponse, ToolError> {
    if bytes.len() > MAX_BYTES {
        return Err(size_limit("输入超过 20 MB 上限"));
    }
    let encoded = engine(url_safe, padding).encode(bytes);
    let image = image_info(bytes);
    let mime = image
        .as_ref()
        .map(|info| info.mime.clone())
        .unwrap_or_else(|| "text/plain".to_string());
    let data_url = format!("data:{mime};base64,{encoded}");
    Ok(Base64CodecResponse {
        output_bytes: encoded.len(),
        input_bytes: bytes.len(),
        text: Some(encoded),
        bytes: None,
        data_url: Some(data_url),
        image,
    })
}

fn decode(req: &Base64CodecRequest) -> Result<Base64CodecResponse, ToolError> {
    let text = match &req.payload {
        Base64Payload::Text(text) => text,
        Base64Payload::Bytes(_) => {
            return Err(unsupported("Base64 解码输入必须是文本")
                .with_hint("请以文本模式粘贴 Base64 或 Data URI"));
        }
    };

    if req.line_by_line {
        return decode_lines(text);
    }

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(invalid_syntax("输入为空"));
    }
    if trimmed.starts_with("data:") {
        return decode_data_uri(trimmed);
    }
    if trimmed.len() / 4 * 3 > MAX_BYTES || trimmed.len() > MAX_BYTES * 4 / 3 + 4 {
        return Err(size_limit("Base64 输入超过 20 MB 上限"));
    }
    let bytes = decode_to_bytes(trimmed)?;
    if bytes.len() > MAX_BYTES {
        return Err(size_limit("解码结果超过 20 MB 上限"));
    }
    let text_out = String::from_utf8(bytes.clone()).ok();
    let image = image_info(&bytes);
    let data_url = image.as_ref().map(|info| {
        format!(
            "data:{};base64,{}",
            info.mime,
            engine(false, true).encode(&bytes)
        )
    });
    Ok(Base64CodecResponse {
        input_bytes: text.len(),
        output_bytes: bytes.len(),
        text: text_out,
        bytes: Some(bytes),
        data_url,
        image,
    })
}

fn decode_data_uri(input: &str) -> Result<Base64CodecResponse, ToolError> {
    let data_url = data_url::DataUrl::process(input)
        .map_err(|err| invalid_syntax(format!("Data URI 解析失败：{err}")))?;
    let (bytes, _fragment) = data_url
        .decode_to_vec()
        .map_err(|err| invalid_syntax(format!("Data URI 解码失败：{err}")))?;
    if bytes.len() > MAX_BYTES {
        return Err(size_limit("Data URI 解码结果超过 20 MB 上限"));
    }
    let mime = data_url.mime_type().to_string();
    let image = image_info(&bytes).map(|mut info| {
        info.mime = mime.clone();
        info
    });
    let text_out = String::from_utf8(bytes.clone()).ok();
    let data_url = image.as_ref().map(|info| {
        format!(
            "data:{};base64,{}",
            info.mime,
            engine(false, true).encode(&bytes)
        )
    });
    Ok(Base64CodecResponse {
        input_bytes: input.len(),
        output_bytes: bytes.len(),
        text: text_out,
        bytes: Some(bytes),
        data_url,
        image,
    })
}

fn decode_lines(text: &str) -> Result<Base64CodecResponse, ToolError> {
    let mut decoded_lines: Vec<String> = Vec::new();
    let mut total = 0usize;
    for (index, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            decoded_lines.push(String::new());
            continue;
        }
        let bytes = decode_to_bytes(line.trim())
            .map_err(|err| invalid_syntax(format!("第 {} 行：{}", index + 1, err.message)))?;
        total += bytes.len();
        if total > MAX_BYTES {
            return Err(size_limit("逐行解码结果超过 20 MB 上限"));
        }
        decoded_lines.push(
            String::from_utf8(bytes).map_err(|_| {
                invalid_syntax(format!("第 {} 行解码结果不是 UTF-8 文本", index + 1))
            })?,
        );
    }
    let mut joined = decoded_lines.join("\n");
    if text.ends_with('\n') {
        joined.push('\n');
    }
    Ok(Base64CodecResponse {
        input_bytes: text.len(),
        output_bytes: joined.len(),
        text: Some(joined),
        bytes: None,
        data_url: None,
        image: None,
    })
}

pub(crate) fn decode_to_bytes(input: &str) -> Result<Vec<u8>, ToolError> {
    let compact: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return Ok(Vec::new());
    }
    let url_safe = compact.contains('-') || compact.contains('_');
    let padded = compact.ends_with('=');
    let candidates: Vec<&base64::engine::GeneralPurpose> = if url_safe {
        if padded {
            vec![&URL_SAFE, &URL_SAFE_NO_PAD]
        } else {
            vec![&URL_SAFE_NO_PAD, &URL_SAFE]
        }
    } else if padded {
        vec![&STANDARD, &STANDARD_NO_PAD]
    } else {
        vec![&STANDARD_NO_PAD, &STANDARD]
    };
    let mut last_error = None;
    for engine in candidates {
        match engine.decode(compact.as_bytes()) {
            Ok(bytes) => return Ok(bytes),
            Err(err) => last_error = Some(err),
        }
    }
    Err(invalid_syntax(format!(
        "Base64 解码失败：{}",
        last_error.map(|err| err.to_string()).unwrap_or_default()
    )))
}

fn image_info(bytes: &[u8]) -> Option<ImageInfo> {
    let format = image::guess_format(bytes).ok()?;
    let mime = match format {
        image::ImageFormat::Png => "image/png",
        image::ImageFormat::Jpeg => "image/jpeg",
        image::ImageFormat::Gif => "image/gif",
        image::ImageFormat::WebP => "image/webp",
        image::ImageFormat::Bmp => "image/bmp",
        image::ImageFormat::Ico => "image/x-icon",
        _ => return None,
    };
    let reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    let (width, height) = reader.into_dimensions().ok()?;
    Some(ImageInfo {
        width,
        height,
        format: format
            .extensions_str()
            .first()
            .copied()
            .unwrap_or("bin")
            .to_string(),
        mime: mime.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use devtools_core::ToolErrorKind;

    fn text_request(text: &str, mode: Base64Mode) -> Base64CodecRequest {
        Base64CodecRequest {
            mode,
            payload: Base64Payload::Text(text.to_string()),
            url_safe: false,
            padding: true,
            line_by_line: false,
        }
    }

    fn png_bytes() -> Vec<u8> {
        let image = image::RgbaImage::from_pixel(2, 3, image::Rgba([255, 0, 0, 255]));
        let mut out = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    #[test]
    fn text_round_trip() {
        let encoded = run(&text_request("hello", Base64Mode::Encode)).unwrap();
        assert_eq!(encoded.text.as_deref(), Some("aGVsbG8="));
        let decoded = run(&text_request("aGVsbG8=", Base64Mode::Decode)).unwrap();
        assert_eq!(decoded.text.as_deref(), Some("hello"));
    }

    #[test]
    fn padding_toggle() {
        let mut req = text_request("hello", Base64Mode::Encode);
        req.padding = false;
        let encoded = run(&req).unwrap();
        assert_eq!(encoded.text.as_deref(), Some("aGVsbG8"));
        // 解码时缺省填充也能容错。
        let decoded = run(&text_request("aGVsbG8", Base64Mode::Decode)).unwrap();
        assert_eq!(decoded.text.as_deref(), Some("hello"));
    }

    #[test]
    fn url_safe_alphabet() {
        let bytes = vec![0xfb, 0xff];
        let standard = run(&Base64CodecRequest {
            mode: Base64Mode::Encode,
            payload: Base64Payload::Bytes(bytes.clone()),
            url_safe: false,
            padding: true,
            line_by_line: false,
        })
        .unwrap();
        assert_eq!(standard.text.as_deref(), Some("+/8="));
        let safe = run(&Base64CodecRequest {
            mode: Base64Mode::Encode,
            payload: Base64Payload::Bytes(bytes),
            url_safe: true,
            padding: true,
            line_by_line: false,
        })
        .unwrap();
        assert_eq!(safe.text.as_deref(), Some("-_8="));
        let decoded = run(&text_request("-_8=", Base64Mode::Decode)).unwrap();
        assert_eq!(decoded.text, None);
        assert_eq!(decoded.bytes, Some(vec![0xfb, 0xff]));
    }

    #[test]
    fn line_by_line_encodes_each_line() {
        let mut req = text_request("a\nbc\n", Base64Mode::Encode);
        req.line_by_line = true;
        let encoded = run(&req).unwrap();
        assert_eq!(encoded.text.as_deref(), Some("YQ==\nYmM=\n"));
        let mut req = text_request("YQ==\nYmM=", Base64Mode::Decode);
        req.line_by_line = true;
        assert_eq!(run(&req).unwrap().text.as_deref(), Some("a\nbc"));
    }

    #[test]
    fn image_bytes_expose_dimensions_and_data_uri() {
        let bytes = png_bytes();
        let resp = run(&Base64CodecRequest {
            mode: Base64Mode::Encode,
            payload: Base64Payload::Bytes(bytes),
            url_safe: false,
            padding: true,
            line_by_line: false,
        })
        .unwrap();
        let info = resp.image.expect("image info");
        assert_eq!((info.width, info.height), (2, 3));
        assert_eq!(info.mime, "image/png");
        assert!(
            resp.data_url
                .as_deref()
                .unwrap()
                .starts_with("data:image/png;base64,")
        );
    }

    #[test]
    fn decode_data_uri_restores_bytes() {
        let bytes = png_bytes();
        let encoded = engine(false, true).encode(&bytes);
        let uri = format!("data:image/png;base64,{encoded}");
        let resp = run(&text_request(&uri, Base64Mode::Decode)).unwrap();
        assert_eq!(resp.bytes.as_deref(), Some(bytes.as_slice()));
        assert_eq!(resp.image.as_ref().unwrap().width, 2);
    }

    #[test]
    fn invalid_base64_is_rejected() {
        let err = run(&text_request("!!!", Base64Mode::Decode)).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
    }

    #[test]
    fn oversized_input_is_rejected_before_work() {
        let err = run(&Base64CodecRequest {
            mode: Base64Mode::Encode,
            payload: Base64Payload::Bytes(vec![0u8; MAX_BYTES + 1]),
            url_safe: false,
            padding: true,
            line_by_line: false,
        })
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::SizeLimitExceeded);
    }
}
