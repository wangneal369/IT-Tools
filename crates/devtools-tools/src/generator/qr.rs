//! #16 二维码生成与解码。
//!
//! 生成：文本 / URL + WiFi 模板、L/M/Q/H 纠错、SVG 字符串与 PNG 字节。
//! 解码：`rqrr` 三步预处理（原图 → 反色 → 白色静区边框），逐级重试。

use std::io::Cursor;

use qrcode::render::svg;
use qrcode::{EcLevel, QrCode};

use crate::util::{check_cancel, invalid_syntax, size_limit, unsupported};

use devtools_core::{CancelFlag, ToolError};

/// 解码输入上限（10 MB）。
pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// 纠错级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QrErrorCorrection {
    L,
    M,
    Q,
    H,
}

impl QrErrorCorrection {
    const fn level(self) -> EcLevel {
        match self {
            QrErrorCorrection::L => EcLevel::L,
            QrErrorCorrection::M => EcLevel::M,
            QrErrorCorrection::Q => EcLevel::Q,
            QrErrorCorrection::H => EcLevel::H,
        }
    }
}

/// WiFi 加密方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiSecurity {
    Wpa,
    Wep,
    None,
}

/// WiFi 配置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiConfig {
    pub ssid: String,
    pub password: Option<String>,
    pub security: WifiSecurity,
    pub hidden: bool,
}

/// 生成内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QrContent {
    Text(String),
    Url(String),
    Wifi(WifiConfig),
}

/// 生成请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrGenerateRequest {
    pub content: QrContent,
    pub ec: QrErrorCorrection,
    /// 目标最小边长（像素），默认 256。
    pub size: Option<u32>,
}

/// 生成结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrGenerateResponse {
    /// 实际编码载荷（WiFi 模板已展开）。
    pub payload: String,
    pub svg: String,
    pub png: Vec<u8>,
    /// 二维码版本 1–40。
    pub version: u8,
    /// 每边模块数（含定位图形，不含静区）。
    pub module_count: u32,
}

/// 解码请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrDecodeRequest {
    pub image_bytes: Vec<u8>,
}

/// 解码结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QrDecodeResponse {
    pub text: String,
    /// 成功的预处理步骤：`original` / `inverted` / `quiet-zone`。
    pub preprocessing: &'static str,
}

/// 生成二维码。
pub fn generate(req: &QrGenerateRequest) -> Result<QrGenerateResponse, ToolError> {
    let payload = match &req.content {
        QrContent::Text(text) => text.clone(),
        QrContent::Url(url) => url.clone(),
        QrContent::Wifi(config) => wifi_payload(config),
    };
    if payload.is_empty() {
        return Err(invalid_syntax("二维码内容不能为空"));
    }
    let size = req.size.unwrap_or(256);
    if !(64..=2048).contains(&size) {
        return Err(unsupported("二维码边长必须在 64–2048 像素之间"));
    }
    let code = QrCode::with_error_correction_level(payload.as_bytes(), req.ec.level())
        .map_err(|err| unsupported(format!("二维码生成失败（内容过长？）：{err}")))?;

    let svg = code
        .render::<svg::Color>()
        .min_dimensions(size, size)
        .build();
    let buffer = code
        .render::<::image::Luma<u8>>()
        .min_dimensions(size, size)
        .build();
    let mut cursor = Cursor::new(Vec::new());
    ::image::DynamicImage::ImageLuma8(buffer)
        .write_to(&mut cursor, ::image::ImageFormat::Png)
        .map_err(|err| invalid_syntax(format!("PNG 编码失败：{err}")))?;

    let module_count = (code.to_colors().len() as f64).sqrt().round() as u32;
    let version = if module_count >= 21 {
        ((module_count - 17) / 4) as u8
    } else {
        0
    };
    Ok(QrGenerateResponse {
        payload,
        svg,
        png: cursor.into_inner(),
        version,
        module_count,
    })
}

/// 解码二维码图片（PNG / JPEG / GIF / WebP / BMP）。
pub fn decode(
    req: &QrDecodeRequest,
    cancel: Option<&CancelFlag>,
) -> Result<QrDecodeResponse, ToolError> {
    check_cancel(cancel)?;
    if req.image_bytes.len() > MAX_IMAGE_BYTES {
        return Err(size_limit("图片超过 10 MB 上限"));
    }
    if req.image_bytes.is_empty() {
        return Err(invalid_syntax("图片数据为空"));
    }
    let image = ::image::load_from_memory(&req.image_bytes)
        .map_err(|err| invalid_syntax(format!("无法读取图片：{err}")))?;
    let gray = image.to_luma8();

    check_cancel(cancel)?;
    if let Some(text) = try_decode(gray.clone()) {
        return Ok(QrDecodeResponse {
            text,
            preprocessing: "original",
        });
    }
    check_cancel(cancel)?;
    let inverted = ::image::GrayImage::from_fn(gray.width(), gray.height(), |x, y| {
        let value = gray.get_pixel(x, y)[0];
        ::image::Luma([255 - value])
    });
    if let Some(text) = try_decode(inverted) {
        return Ok(QrDecodeResponse {
            text,
            preprocessing: "inverted",
        });
    }
    check_cancel(cancel)?;
    let bordered = add_white_border(&gray, 32);
    if let Some(text) = try_decode(bordered) {
        return Ok(QrDecodeResponse {
            text,
            preprocessing: "quiet-zone",
        });
    }
    Err(invalid_syntax("未能识别二维码")
        .with_hint("请确保图片清晰、对比度足够，并尽量保留四周留白"))
}

/// 生成标准 WiFi 二维码载荷，并按规范转义 `\ ; , : "`。
pub fn wifi_payload(config: &WifiConfig) -> String {
    let security = match config.security {
        WifiSecurity::Wpa => "WPA",
        WifiSecurity::Wep => "WEP",
        WifiSecurity::None => "nopass",
    };
    let mut payload = format!("WIFI:T:{security};S:{};", escape_wifi(&config.ssid));
    if let Some(password) = &config.password
        && config.security != WifiSecurity::None
    {
        payload.push_str(&format!("P:{};", escape_wifi(password)));
    }
    if config.hidden {
        payload.push_str("H:true;");
    }
    payload.push(';');
    payload
}

fn escape_wifi(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '\\' | ';' | ',' | ':' | '"') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn try_decode(image: ::image::GrayImage) -> Option<String> {
    let mut prepared = rqrr::PreparedImage::prepare(image);
    for grid in prepared.detect_grids() {
        if let Ok((_meta, text)) = grid.decode() {
            return Some(text);
        }
    }
    None
}

fn add_white_border(image: &::image::GrayImage, border: u32) -> ::image::GrayImage {
    let width = image.width() + border * 2;
    let height = image.height() + border * 2;
    let mut padded = ::image::GrayImage::from_pixel(width, height, ::image::Luma([255]));
    ::image::imageops::replace(&mut padded, image, i64::from(border), i64::from(border));
    padded
}

#[cfg(test)]
mod tests {
    use super::*;
    use devtools_core::ToolErrorKind;

    fn generate_png(content: QrContent) -> QrGenerateResponse {
        generate(&QrGenerateRequest {
            content,
            ec: QrErrorCorrection::M,
            size: Some(256),
        })
        .unwrap()
    }

    fn decode_png(png: Vec<u8>) -> Result<QrDecodeResponse, ToolError> {
        decode(&QrDecodeRequest { image_bytes: png }, None)
    }

    #[test]
    fn text_generates_svg_and_png() {
        let resp = generate_png(QrContent::Text("https://example.com".to_string()));
        assert!(resp.svg.contains("<svg"), "{}", resp.svg);
        assert_eq!(&resp.png[..4], b"\x89PNG");
        assert!((1..=40).contains(&resp.version));
        assert!(resp.module_count >= 21);
        assert_eq!(resp.payload, "https://example.com");
    }

    #[test]
    fn encode_decode_round_trip() {
        let text = "hello 二维码 123";
        let resp = generate_png(QrContent::Text(text.to_string()));
        let decoded = decode_png(resp.png).unwrap();
        assert_eq!(decoded.text, text);
        assert_eq!(decoded.preprocessing, "original");
    }

    #[test]
    fn url_content_round_trips() {
        let url = "https://github.com/wangneal369/IT-Tools";
        let resp = generate_png(QrContent::Url(url.to_string()));
        assert_eq!(decode_png(resp.png).unwrap().text, url);
    }

    #[test]
    fn inverted_image_is_decoded_after_preprocessing() {
        let resp = generate_png(QrContent::Text("invert me".to_string()));
        let image = ::image::load_from_memory(&resp.png).unwrap().to_luma8();
        let inverted = ::image::GrayImage::from_fn(image.width(), image.height(), |x, y| {
            ::image::Luma([255 - image.get_pixel(x, y)[0]])
        });
        let mut cursor = Cursor::new(Vec::new());
        ::image::DynamicImage::ImageLuma8(inverted)
            .write_to(&mut cursor, ::image::ImageFormat::Png)
            .unwrap();
        let decoded = decode_png(cursor.into_inner()).unwrap();
        assert_eq!(decoded.text, "invert me");
        assert!(decoded.preprocessing != "original");
    }

    #[test]
    fn all_error_correction_levels_generate() {
        for ec in [
            QrErrorCorrection::L,
            QrErrorCorrection::M,
            QrErrorCorrection::Q,
            QrErrorCorrection::H,
        ] {
            let resp = generate(&QrGenerateRequest {
                content: QrContent::Text("ec level".to_string()),
                ec,
                size: Some(128),
            })
            .unwrap();
            assert_eq!(decode_png(resp.png).unwrap().text, "ec level");
        }
    }

    #[test]
    fn wifi_payload_matches_standard() {
        let config = WifiConfig {
            ssid: "My;Net".to_string(),
            password: Some("p@ss:word".to_string()),
            security: WifiSecurity::Wpa,
            hidden: true,
        };
        assert_eq!(
            wifi_payload(&config),
            "WIFI:T:WPA;S:My\\;Net;P:p@ss\\:word;H:true;;"
        );
        let open = WifiConfig {
            ssid: "Open".to_string(),
            password: None,
            security: WifiSecurity::None,
            hidden: false,
        };
        assert_eq!(wifi_payload(&open), "WIFI:T:nopass;S:Open;;");
    }

    #[test]
    fn wifi_qr_round_trips() {
        let config = WifiConfig {
            ssid: "Home".to_string(),
            password: Some("secret123".to_string()),
            security: WifiSecurity::Wpa,
            hidden: false,
        };
        let payload = wifi_payload(&config);
        let resp = generate_png(QrContent::Wifi(config));
        assert_eq!(resp.payload, payload);
        assert_eq!(decode_png(resp.png).unwrap().text, payload);
    }

    #[test]
    fn empty_content_is_rejected() {
        let err = generate(&QrGenerateRequest {
            content: QrContent::Text(String::new()),
            ec: QrErrorCorrection::M,
            size: None,
        })
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
    }

    #[test]
    fn invalid_size_is_rejected() {
        let err = generate(&QrGenerateRequest {
            content: QrContent::Text("x".to_string()),
            ec: QrErrorCorrection::M,
            size: Some(10),
        })
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
    }

    #[test]
    fn undecodable_image_is_rejected() {
        let image = ::image::GrayImage::from_pixel(64, 64, ::image::Luma([255]));
        let mut cursor = Cursor::new(Vec::new());
        ::image::DynamicImage::ImageLuma8(image)
            .write_to(&mut cursor, ::image::ImageFormat::Png)
            .unwrap();
        let err = decode_png(cursor.into_inner()).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
    }

    #[test]
    fn oversized_image_is_rejected() {
        let err = decode(
            &QrDecodeRequest {
                image_bytes: vec![0u8; MAX_IMAGE_BYTES + 1],
            },
            None,
        )
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::SizeLimitExceeded);
    }
}
