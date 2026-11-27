//! Dedicated conversion, codec and generator controllers.

use std::sync::Arc;

use devtools_core::{CancelFlag, RawPayload, ToolError, ToolErrorKind, ToolId, ToolLaunchInput};
use devtools_tools::{
    conversion::{color, datetime, json_table, number_base},
    encode_decode::{base64, url},
    generator::{password, qr, uuid_ulid},
};
use gpui_kit::*;

use super::runtime::{Inputs, Slot, ToolResult, ToolView, WcagSummary, option_group, options_bar};
use super::{EditorLanguages, Model};
use crate::icons::{IconName, icon_el};
use crate::syntax::SyntaxLang;
use crate::theme::Palette;

#[derive(Clone)]
pub(super) enum ConverterOptions {
    Date {
        unit: usize,
        zone: usize,
    },
    Number {
        base: u8,
        custom: u8,
        bits: usize,
        uppercase: bool,
        prefixes: bool,
        group: bool,
    },
    Color,
    Table {
        format: usize,
        flatten: bool,
        bom: bool,
    },
    Base64 {
        decode: bool,
        data_uri: bool,
        url_safe: bool,
        padding: bool,
        lines: bool,
    },
    Url {
        mode: usize,
        component: bool,
        plus: bool,
        lines: bool,
    },
    Uuid {
        kind: usize,
        count: usize,
        uppercase: bool,
        hyphens: bool,
        format: usize,
    },
    Password {
        token: bool,
        length: usize,
        bytes: usize,
        encoding: usize,
        count: usize,
        lower: bool,
        upper: bool,
        digits: bool,
        symbols: bool,
        ambiguous: bool,
    },
    Qr {
        mode: usize,
        ec: usize,
        security: usize,
        hidden: bool,
    },
}

const ZONES: [&str; 5] = [
    "Local",
    "UTC",
    "Asia/Shanghai",
    "America/New_York",
    "Europe/London",
];

const COLOR_PRESETS: &[(&str, &str, u32)] = &[
    ("蓝", "#3b82f6", 0x3b82f6),
    ("绿", "#10b981", 0x10b981),
    ("琥珀", "#f59e0b", 0xf59e0b),
    ("红", "#ef4444", 0xef4444),
    ("紫", "#8b5cf6", 0x8b5cf6),
    ("深墨", "#0f172a", 0x0f172a),
];

fn io_error(error: std::io::Error) -> ToolError {
    ToolError::new(ToolErrorKind::Io, error.to_string())
}

pub(super) fn read_bytes(inputs: &Inputs, limit: usize) -> Result<Vec<u8>, ToolError> {
    match &inputs.payload {
        Some(RawPayload::Image(image)) => {
            if image.bytes.len() > limit {
                return Err(ToolError::new(
                    ToolErrorKind::SizeLimitExceeded,
                    "图片超过大小上限",
                ));
            }
            Ok(image.bytes.to_vec())
        }
        Some(RawPayload::Files(paths)) if !paths.is_empty() => read_file(&paths[0], limit),
        _ => Ok(inputs.text.as_bytes().to_vec()),
    }
}

pub(super) fn read_file(path: &std::path::Path, limit: usize) -> Result<Vec<u8>, ToolError> {
    use std::io::Read;
    let file = std::fs::File::open(path).map_err(io_error)?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() > limit {
        return Err(ToolError::new(
            ToolErrorKind::SizeLimitExceeded,
            format!("文件超过 {} MB 上限", limit / 1024 / 1024),
        ));
    }
    Ok(bytes)
}

impl ConverterOptions {
    pub fn new(tool: ToolId) -> Option<Self> {
        Some(match tool {
            ToolId::DateTimeConverter => Self::Date { unit: 0, zone: 0 },
            ToolId::NumberBaseConverter => Self::Number {
                base: 0,
                custom: 36,
                bits: 0,
                uppercase: true,
                prefixes: true,
                group: false,
            },
            ToolId::ColorConverter => Self::Color,
            ToolId::JsonToTable => Self::Table {
                format: 0,
                flatten: true,
                bom: false,
            },
            ToolId::Base64Codec => Self::Base64 {
                decode: false,
                data_uri: false,
                url_safe: false,
                padding: true,
                lines: false,
            },
            ToolId::UrlCodec => Self::Url {
                mode: 0,
                component: false,
                plus: false,
                lines: false,
            },
            ToolId::UuidUlidGenerator => Self::Uuid {
                kind: 0,
                count: 1,
                uppercase: false,
                hyphens: true,
                format: 0,
            },
            ToolId::PasswordTokenGenerator => Self::Password {
                token: false,
                length: 16,
                bytes: 32,
                encoding: 0,
                count: 1,
                lower: true,
                upper: true,
                digits: true,
                symbols: true,
                ambiguous: true,
            },
            ToolId::QrCodeGenerator => Self::Qr {
                mode: 0,
                ec: 1,
                security: 0,
                hidden: false,
            },
            _ => return None,
        })
    }

    pub fn syntax_languages(&self) -> EditorLanguages {
        match self {
            Self::Color => EditorLanguages {
                text: Some(SyntaxLang::Css),
                second: None,
                output: None,
            },
            Self::Table { format, .. } => EditorLanguages {
                text: Some(SyntaxLang::Json),
                second: None,
                output: (*format == 3).then_some(SyntaxLang::Markdown),
            },
            Self::Uuid { format, .. } => EditorLanguages {
                text: None,
                second: None,
                output: (*format == 1).then_some(SyntaxLang::Json),
            },
            _ => EditorLanguages::default(),
        }
    }

    pub fn inject(&mut self, text: &str, _: Option<&ToolLaunchInput>) {
        if let Self::Base64 {
            decode, data_uri, ..
        } = self
        {
            *data_uri = text.starts_with("data:");
            *decode = *data_uri
                || (text.len() >= 8
                    && text.trim().bytes().all(|b| {
                        b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'-' | b'_')
                    }));
        }
    }

    pub fn execute(&self, inputs: &Inputs, cancel: &CancelFlag) -> Result<ToolResult, ToolError> {
        let text = inputs.text.clone();
        Ok(match self {
            Self::Date { unit, zone } => {
                let out = datetime::convert(&datetime::DateTimeConverterRequest {
                    input: text,
                    timezone: ZONES[*zone].into(),
                    timestamp_unit: [
                        datetime::TimestampUnit::Auto,
                        datetime::TimestampUnit::Seconds,
                        datetime::TimestampUnit::Millis,
                        datetime::TimestampUnit::Micros,
                        datetime::TimestampUnit::Nanos,
                    ][*unit],
                })?;
                ToolResult::rows(vec![
                    ("本地时间".into(), out.local),
                    ("UTC / ISO 8601".into(), out.utc),
                    (format!("时区 ({})", out.timezone_name), out.timezone),
                    ("Unix 秒 (s)".into(), out.unix_seconds.to_string()),
                    ("Unix 毫秒 (ms)".into(), out.unix_millis.to_string()),
                    ("Unix 微秒 (μs)".into(), out.unix_micros.to_string()),
                    ("Unix 纳秒 (ns)".into(), out.unix_nanos.to_string()),
                    (
                        "星期 / ISO 周".into(),
                        format!("{} · {}", out.weekday, out.iso_week),
                    ),
                ])
            }
            Self::Number {
                base,
                custom,
                bits,
                uppercase,
                prefixes,
                group,
            } => {
                let out = number_base::convert(&number_base::NumberBaseConverterRequest {
                    input: text,
                    from_base: *base,
                    signed_mode: if *bits == 0 {
                        number_base::SignedMode::Mathematical
                    } else {
                        number_base::SignedMode::Bits
                    },
                    bit_width: [None, Some(8), Some(16), Some(32), Some(64)][*bits],
                    custom_base: Some(*custom),
                    uppercase: *uppercase,
                    prefixes: *prefixes,
                    group: *group,
                })?;
                let mut result = ToolResult::rows(vec![
                    ("二进制 (BIN · 2)".into(), out.binary),
                    ("八进制 (OCT · 8)".into(), out.octal),
                    ("十进制 (DEC · 10)".into(), out.decimal),
                    ("十六进制 (HEX · 16)".into(), out.hexadecimal),
                    (
                        format!("自定义 ({custom} 进制)"),
                        out.custom.unwrap_or_default(),
                    ),
                    ("有符号十进制".into(), out.signed_value),
                    ("有效位宽".into(), format!("{} bits", out.bits)),
                ]);
                result.footer = format!(
                    "源进制：{} · 有效位宽 {} bits",
                    if *base == 0 {
                        "自动识别".to_owned()
                    } else {
                        format!("{base} 进制")
                    },
                    out.bits
                );
                result
            }
            Self::Color => {
                let out = color::convert(&color::ColorConverterRequest { input: text })?;
                let [r, g, b] = out.rgb8;
                let mut result = ToolResult::rows(vec![
                    ("HEX".into(), out.hex),
                    ("RGB".into(), out.rgb),
                    ("HSL".into(), out.hsl),
                    ("HWB".into(), out.hwb),
                    ("OKLCH".into(), out.oklch),
                    ("CMYK".into(), out.cmyk),
                    ("不透明度 Alpha".into(), format!("{}%", out.alpha_percent)),
                ]);
                result.wcag = vec![
                    WcagSummary {
                        label: "#FFFFFF 浅底",
                        bg_hex: 0xffffff,
                        ratio: out.against_white.ratio,
                        aa: out.against_white.aa,
                        aaa: out.against_white.aaa,
                    },
                    WcagSummary {
                        label: "#000000 深底",
                        bg_hex: 0x000000,
                        ratio: out.against_black.ratio,
                        aa: out.against_black.aa,
                        aaa: out.against_black.aaa,
                    },
                ];
                result.swatch = Some((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b));
                result.footer = format!(
                    "RGB({r}, {g}, {b}) · 对比度 白 {:.2}:1 / 黑 {:.2}:1",
                    out.against_white.ratio, out.against_black.ratio
                );
                result
            }
            Self::Table {
                format,
                flatten,
                bom,
            } => {
                let out = json_table::convert(&json_table::JsonToTableRequest {
                    input: text,
                    flatten: *flatten,
                    separator: if inputs.query.is_empty() {
                        ".".into()
                    } else {
                        inputs.query.clone()
                    },
                    export_format: [
                        json_table::TableExportFormat::Csv,
                        json_table::TableExportFormat::Csv,
                        json_table::TableExportFormat::Tsv,
                        json_table::TableExportFormat::Markdown,
                    ][*format],
                    bom: *bom,
                })?;
                ToolResult {
                    output: out.export,
                    table: (*format == 0).then_some((out.preview.columns, out.preview.rows)),
                    footer: format!(
                        "{} 行 · {} 单元格{} · 复制导出保留全量",
                        out.total_rows,
                        out.total_cells,
                        if out.preview_truncated {
                            " · 超过 50k 单元格，预览已截断"
                        } else {
                            ""
                        }
                    ),
                    ..ToolResult::default()
                }
            }
            Self::Base64 {
                decode,
                data_uri,
                url_safe,
                padding,
                lines,
            } => {
                let payload = if !*decode && *data_uri && !inputs.query.is_empty() {
                    base64::Base64Payload::Bytes(read_file(
                        std::path::Path::new(&inputs.query),
                        base64::MAX_BYTES,
                    )?)
                } else if !*decode && !matches!(inputs.payload, Some(RawPayload::Text(_)) | None) {
                    base64::Base64Payload::Bytes(read_bytes(inputs, base64::MAX_BYTES)?)
                } else {
                    base64::Base64Payload::Text(text)
                };
                let out = base64::run(&base64::Base64CodecRequest {
                    mode: if *decode {
                        base64::Base64Mode::Decode
                    } else {
                        base64::Base64Mode::Encode
                    },
                    payload,
                    url_safe: *url_safe,
                    padding: *padding,
                    line_by_line: *lines,
                })?;
                let image = out
                    .image
                    .as_ref()
                    .and(out.bytes.as_ref())
                    .map(|bytes| Arc::from(bytes.as_slice()));
                let output = if *data_uri && !*decode {
                    out.data_url.or(out.text).unwrap_or_default()
                } else {
                    out.text.unwrap_or_else(|| {
                        out.bytes
                            .as_ref()
                            .map(|bytes| bytes.iter().map(|b| format!("{b:02x}")).collect())
                            .unwrap_or_default()
                    })
                };
                ToolResult {
                    output,
                    image,
                    image_format: out
                        .image
                        .as_ref()
                        .and_then(|i| ImageFormat::from_mime_type(&i.mime)),
                    footer: format!(
                        "{} → {} 字节{}",
                        out.input_bytes,
                        out.output_bytes,
                        out.image
                            .map(|i| format!(" · {}×{} {}", i.width, i.height, i.mime))
                            .unwrap_or_default()
                    ),
                    ..ToolResult::default()
                }
            }
            Self::Url {
                mode,
                component,
                plus,
                lines,
            } => {
                let source = if *mode == 0 && !inputs.query.is_empty() {
                    let (url, fragment) = text
                        .split_once('#')
                        .map_or((text.as_str(), None), |(url, fragment)| {
                            (url, Some(fragment))
                        });
                    let base = url.split_once('?').map_or(url, |(base, _)| base);
                    format!(
                        "{base}?{}{}",
                        inputs.query,
                        fragment.map(|f| format!("#{f}")).unwrap_or_default()
                    )
                } else {
                    text
                };
                let out = url::run(&url::UrlCodecRequest {
                    input: source,
                    mode: [
                        url::UrlMode::Parse,
                        url::UrlMode::Encode,
                        url::UrlMode::Decode,
                    ][*mode],
                    target: if *component {
                        url::UrlTarget::Component
                    } else {
                        url::UrlTarget::FullUrl
                    },
                    space_as_plus: *plus,
                    line_by_line: *lines,
                })?;
                let rows = out
                    .parts
                    .map(|p| {
                        vec![
                            ("Scheme".into(), p.scheme),
                            ("Username".into(), p.username),
                            ("Host".into(), p.host.unwrap_or_default()),
                            (
                                "Port".into(),
                                p.port.map(|v| v.to_string()).unwrap_or_default(),
                            ),
                            ("Path".into(), p.path),
                            ("Query".into(), p.query.unwrap_or_default()),
                            ("Fragment".into(), p.fragment.unwrap_or_default()),
                            ("Origin".into(), p.origin),
                        ]
                    })
                    .unwrap_or_default();
                ToolResult {
                    output: out.output,
                    rows,
                    table: (*mode == 0).then(|| {
                        (
                            vec!["Query Key".into(), "Value（保序，支持同名重复键）".into()],
                            out.params
                                .into_iter()
                                .map(|p| vec![p.key, p.value])
                                .collect(),
                        )
                    }),
                    ..ToolResult::default()
                }
            }
            Self::Uuid {
                kind,
                count,
                uppercase,
                hyphens,
                format,
            } => {
                let out = uuid_ulid::generate(
                    &uuid_ulid::UuidUlidRequest {
                        kind: [
                            uuid_ulid::IdKind::V4,
                            uuid_ulid::IdKind::V7,
                            uuid_ulid::IdKind::V1,
                            uuid_ulid::IdKind::Nil,
                            uuid_ulid::IdKind::Ulid,
                        ][*kind],
                        count: *count,
                        uppercase: *uppercase,
                        hyphens: *hyphens,
                        format: [
                            uuid_ulid::IdOutputFormat::Plain,
                            uuid_ulid::IdOutputFormat::Json,
                            uuid_ulid::IdOutputFormat::Quoted,
                        ][*format],
                        one_per_line: true,
                    },
                    Some(cancel),
                )?;
                let mut result = ToolResult::text(out.output);
                result.footer = format!("已生成 {count} 个标识符");
                if !inputs.text.trim().is_empty() {
                    let info = uuid_ulid::inspect(inputs.text.trim())?;
                    result.rows = vec![
                        ("批量生成结果".into(), result.output.clone()),
                        ("反解格式".into(), info.format),
                        (
                            "版本号".into(),
                            info.version
                                .map(|v| format!("v{v}"))
                                .unwrap_or_else(|| "—".into()),
                        ),
                        (
                            "嵌入时间戳".into(),
                            info.timestamp.unwrap_or_else(|| "无内嵌时间戳".into()),
                        ),
                        (
                            "是否全零 NIL".into(),
                            if info.is_nil { "是" } else { "否" }.into(),
                        ),
                    ];
                }
                result
            }
            Self::Password {
                token,
                length,
                bytes,
                encoding,
                count,
                lower,
                upper,
                digits,
                symbols,
                ambiguous,
            } => {
                let kind = if *token {
                    password::SecretKind::Token {
                        bytes: *bytes,
                        encoding: [
                            password::TokenEncoding::Hex,
                            password::TokenEncoding::Base64Url,
                            password::TokenEncoding::Base32,
                        ][*encoding],
                    }
                } else {
                    password::SecretKind::Password(password::PasswordOptions {
                        length: *length,
                        lowercase: *lower,
                        uppercase: *upper,
                        digits: *digits,
                        symbols: *symbols,
                        custom_symbols: (!inputs.query.is_empty()).then(|| inputs.query.clone()),
                        exclude_ambiguous: *ambiguous,
                    })
                };
                let out = password::run(
                    &password::PasswordTokenRequest {
                        kind,
                        count: *count,
                        output: password::SecretOutput::Plain,
                    },
                    Some(cancel),
                )?;
                ToolResult {
                    output: out.output,
                    strength: out.strength.first().map(|s| s.zxcvbn_score),
                    footer: out
                        .strength
                        .first()
                        .map(|s| {
                            format!(
                                "香农熵 {:.1} bits · zxcvbn {} / 4 · 估算破解耗时 {}",
                                s.shannon_bits, s.zxcvbn_score, s.crack_time
                            )
                        })
                        .unwrap_or_else(|| format!("加密安全随机 Token · {bytes} 字节")),
                    ..ToolResult::default()
                }
            }
            Self::Qr {
                mode,
                ec,
                security,
                hidden,
            } => {
                if *mode == 3
                    || matches!(
                        inputs.payload,
                        Some(RawPayload::Image(_)) | Some(RawPayload::Files(_))
                    )
                {
                    let bytes = if inputs.query.is_empty() {
                        read_bytes(inputs, qr::MAX_IMAGE_BYTES)?
                    } else {
                        read_file(std::path::Path::new(&inputs.query), qr::MAX_IMAGE_BYTES)?
                    };
                    let out =
                        qr::decode(&qr::QrDecodeRequest { image_bytes: bytes }, Some(cancel))?;
                    ToolResult {
                        output: out.text,
                        footer: format!("二维码解码成功 · 预处理阶段：{}", out.preprocessing),
                        ..ToolResult::default()
                    }
                } else {
                    let content = match mode {
                        1 => qr::QrContent::Url(text),
                        2 => qr::QrContent::Wifi(qr::WifiConfig {
                            ssid: text,
                            password: Some(inputs.secret.clone()),
                            security: [
                                qr::WifiSecurity::Wpa,
                                qr::WifiSecurity::Wep,
                                qr::WifiSecurity::None,
                            ][*security],
                            hidden: *hidden,
                        }),
                        _ => qr::QrContent::Text(text),
                    };
                    let out = qr::generate(&qr::QrGenerateRequest {
                        content,
                        ec: [
                            qr::QrErrorCorrection::L,
                            qr::QrErrorCorrection::M,
                            qr::QrErrorCorrection::Q,
                            qr::QrErrorCorrection::H,
                        ][*ec],
                        size: Some(256),
                    })?;
                    ToolResult {
                        output: out.svg,
                        image: Some(Arc::from(out.png)),
                        footer: format!(
                            "QR 版本 v{} · {}×{} 模块 · 下方为矢量 SVG 源码",
                            out.version, out.module_count, out.module_count
                        ),
                        ..ToolResult::default()
                    }
                }
            }
        })
    }

    pub fn inputs(&self, view: &ToolView, palette: Palette, _cx: &Context<ToolView>) -> AnyElement {
        let mut stack = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .size_full()
            .flex()
            .flex_col();

        match self {
            Self::Password { token, .. } => {
                if !token {
                    stack = stack.child(view.inline_field(
                        Slot::Query,
                        "自定义符号集（留空使用默认安全特殊符号）",
                        palette,
                    ));
                }
                stack = stack.child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .p_4()
                        .flex()
                        .flex_col()
                        .justify_center()
                        .gap_2()
                        .text_xs()
                        .text_color(rgb(palette.muted))
                        .child("密码与随机 Token 由操作系统密码学安全随机源（CSPRNG）实时生成。")
                        .child("调整上方长度、字符集或点击「重新生成」即可刷新；复制时默认写入剪贴板历史排除标记。"),
                );
            }
            Self::Uuid { .. } => {
                stack = stack
                    .child(
                        div()
                            .flex_shrink_0()
                            .px_3()
                            .py_1p5()
                            .bg(rgb(palette.panel_alt))
                            .border_b_1()
                            .border_color(rgb(palette.border))
                            .text_size(px(11.0))
                            .text_color(rgb(palette.muted))
                            .child("可选反解区：粘贴任意 UUID 或 ULID 可实时提取版本与内嵌时间戳"),
                    )
                    .child(view.full_editor(Slot::Text));
            }
            Self::Base64 {
                data_uri: true,
                decode: false,
                ..
            } => {
                stack = stack
                    .child(view.inline_field(
                        Slot::Query,
                        "本地图片 / 文件路径（可选，支持直接拖放文件到窗口）",
                        palette,
                    ))
                    .child(view.full_editor(Slot::Text));
            }
            Self::Qr { mode: 3, .. } => {
                stack = stack
                    .child(view.inline_field(
                        Slot::Query,
                        "待解码二维码图片路径（或拖放图片 / Ctrl+Shift+V 粘贴截图）",
                        palette,
                    ))
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .p_4()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_xs()
                            .text_color(rgb(palette.muted))
                            .child(
                                "支持 PNG / JPEG / WebP 二维码原图、反色与窄白边自动三级预处理解码",
                            ),
                    );
            }
            Self::Qr { mode: 2, .. } => {
                stack = stack
                    .child(view.inline_field(Slot::Secret, "WiFi 密码（仅驻内存）", palette))
                    .child(view.full_editor(Slot::Text));
            }
            Self::Url { mode: 0, .. } => {
                stack = stack
                    .child(view.inline_field(
                        Slot::Query,
                        "Query 覆写编辑（key=value&key=value，保序且保留同名键；留空直接解析下方 URL）",
                        palette,
                    ))
                    .child(view.full_editor(Slot::Text));
            }
            Self::Table { .. } => {
                stack = stack
                    .child(view.inline_field(
                        Slot::Query,
                        "嵌套对象拍平分隔符（默认为 .）",
                        palette,
                    ))
                    .child(view.full_editor(Slot::Text));
            }
            _ => {
                stack = stack.child(view.full_editor(Slot::Text));
            }
        }
        stack.into_any_element()
    }

    pub fn controls(
        &self,
        view: &ToolView,
        palette: Palette,
        cx: &Context<ToolView>,
    ) -> AnyElement {
        let mut groups: Vec<AnyElement> = Vec::new();

        match self {
            Self::Date { unit, zone } => {
                groups.push(option_group(
                    "时间戳单位",
                    palette,
                    view.segmented(
                        "dt-unit",
                        &["自动判定", "秒 (s)", "毫秒 (ms)", "微秒 (μs)", "纳秒 (ns)"],
                        *unit,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Converter(Self::Date { unit, .. }) = &mut view.model {
                                *unit = idx;
                            }
                        },
                    ),
                ));
                groups.push(option_group(
                    "目标时区",
                    palette,
                    view.segmented("dt-zone", &ZONES, *zone, palette, cx, |view, idx| {
                        if let Model::Converter(Self::Date { zone, .. }) = &mut view.model {
                            *zone = idx;
                        }
                    }),
                ));
                groups.push(view.action_pill(
                    "now",
                    Some(IconName::Clock),
                    "填入当前时间",
                    palette,
                    cx,
                    |view| {
                        view.inputs.text = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                            .to_string();
                    },
                ));
            }
            Self::Number {
                base,
                custom,
                bits,
                uppercase,
                prefixes,
                group,
            } => {
                let preset_bases: [u8; 5] = [0, 2, 8, 10, 16];
                let selected_preset = preset_bases
                    .iter()
                    .position(|b| b == base)
                    .unwrap_or(usize::MAX);
                groups.push(option_group(
                    "源进制",
                    palette,
                    view.segmented(
                        "num-base",
                        &["自动", "BIN", "OCT", "DEC", "HEX"],
                        selected_preset,
                        palette,
                        cx,
                        move |view, idx| {
                            if let Model::Converter(Self::Number { base, .. }) = &mut view.model {
                                *base = preset_bases[idx];
                            }
                        },
                    ),
                ));
                let custom_val = *custom;
                groups.push(option_group(
                    "自定义",
                    palette,
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .child(view.action_pill(
                            "custom-minus",
                            Some(IconName::Minus),
                            "",
                            palette,
                            cx,
                            |view| {
                                if let Model::Converter(Self::Number { custom, .. }) =
                                    &mut view.model
                                {
                                    *custom = custom.saturating_sub(1).clamp(2, 36);
                                }
                            },
                        ))
                        .child(
                            div()
                                .px_1p5()
                                .text_size(px(11.5))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(palette.accent))
                                .child(format!("{custom_val}进制")),
                        )
                        .child(view.action_pill(
                            "custom-plus",
                            Some(IconName::Plus),
                            "",
                            palette,
                            cx,
                            |view| {
                                if let Model::Converter(Self::Number { custom, .. }) =
                                    &mut view.model
                                {
                                    *custom = custom.saturating_add(1).clamp(2, 36);
                                }
                            },
                        )),
                ));
                groups.push(option_group(
                    "位宽",
                    palette,
                    view.segmented(
                        "num-bits",
                        &["任意精度", "8b", "16b", "32b", "64b"],
                        *bits,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Converter(Self::Number { bits, .. }) = &mut view.model {
                                *bits = idx;
                            }
                        },
                    ),
                ));
                groups.push(view.toggle_pill(
                    "num-upper",
                    "大写",
                    *uppercase,
                    palette,
                    cx,
                    |view| {
                        if let Model::Converter(Self::Number { uppercase, .. }) = &mut view.model {
                            *uppercase = !*uppercase;
                        }
                    },
                ));
                groups.push(view.toggle_pill(
                    "num-prefix",
                    "前缀",
                    *prefixes,
                    palette,
                    cx,
                    |view| {
                        if let Model::Converter(Self::Number { prefixes, .. }) = &mut view.model {
                            *prefixes = !*prefixes;
                        }
                    },
                ));
                groups.push(
                    view.toggle_pill("num-group", "分组", *group, palette, cx, |view| {
                        if let Model::Converter(Self::Number { group, .. }) = &mut view.model {
                            *group = !*group;
                        }
                    }),
                );
            }
            Self::Color => {
                groups.push(option_group("拾色", palette, view.color_picker()));
                let mut presets = div().flex().items_center().gap_1p5();
                for &(name, hex_str, hex_rgb) in COLOR_PRESETS {
                    presets = presets.child(
                        div()
                            .id(format!("color-preset-{name}"))
                            .h(px(24.0))
                            .px_2()
                            .rounded_md()
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .bg(rgb(palette.panel))
                            .border_1()
                            .border_color(rgb(palette.border))
                            .hover(move |s| s.bg(rgb(palette.hover)))
                            .text_size(px(11.0))
                            .text_color(rgb(palette.muted))
                            .child(
                                div()
                                    .w(px(11.0))
                                    .h(px(11.0))
                                    .rounded_full()
                                    .bg(rgb(hex_rgb)),
                            )
                            .child(name)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.inputs.text = hex_str.to_owned();
                                this.trigger_option_change(true, cx);
                            })),
                    );
                }
                groups.push(option_group("预设色板", palette, presets));
            }
            Self::Table {
                format,
                flatten,
                bom,
            } => {
                groups.push(option_group(
                    "视图与导出格式",
                    palette,
                    view.segmented(
                        "tbl-fmt",
                        &["交互表格", "CSV", "TSV", "Markdown"],
                        *format,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Converter(Self::Table { format, .. }) = &mut view.model {
                                *format = idx;
                            }
                        },
                    ),
                ));
                groups.push(view.toggle_pill(
                    "tbl-flatten",
                    "点号拍平嵌套对象",
                    *flatten,
                    palette,
                    cx,
                    |view| {
                        if let Model::Converter(Self::Table { flatten, .. }) = &mut view.model {
                            *flatten = !*flatten;
                        }
                    },
                ));
                groups.push(
                    view.toggle_pill("tbl-bom", "UTF-8 BOM", *bom, palette, cx, |view| {
                        if let Model::Converter(Self::Table { bom, .. }) = &mut view.model {
                            *bom = !*bom;
                        }
                    }),
                );
            }
            Self::Base64 {
                decode,
                data_uri,
                url_safe,
                padding,
                lines,
            } => {
                groups.push(option_group(
                    "方向",
                    palette,
                    view.segmented(
                        "b64-mode",
                        &["编码", "解码"],
                        usize::from(*decode),
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Converter(Self::Base64 { decode, .. }) = &mut view.model {
                                *decode = idx == 1;
                            }
                        },
                    ),
                ));
                groups.push(option_group(
                    "字符集",
                    palette,
                    view.segmented(
                        "b64-alphabet",
                        &["标准 (+/)", "URL-Safe (-_)"],
                        usize::from(*url_safe),
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Converter(Self::Base64 { url_safe, .. }) = &mut view.model
                            {
                                *url_safe = idx == 1;
                            }
                        },
                    ),
                ));
                groups.push(view.toggle_pill(
                    "b64-data-uri",
                    "Data URI",
                    *data_uri,
                    palette,
                    cx,
                    |view| {
                        if let Model::Converter(Self::Base64 { data_uri, .. }) = &mut view.model {
                            *data_uri = !*data_uri;
                        }
                    },
                ));
                groups.push(view.toggle_pill(
                    "b64-pad",
                    "= 填充",
                    *padding,
                    palette,
                    cx,
                    |view| {
                        if let Model::Converter(Self::Base64 { padding, .. }) = &mut view.model {
                            *padding = !*padding;
                        }
                    },
                ));
                groups.push(view.toggle_pill(
                    "b64-lines",
                    "逐行处理",
                    *lines,
                    palette,
                    cx,
                    |view| {
                        if let Model::Converter(Self::Base64 { lines, .. }) = &mut view.model {
                            *lines = !*lines;
                        }
                    },
                ));
            }
            Self::Url {
                mode,
                component,
                plus,
                lines,
            } => {
                groups.push(option_group(
                    "功能",
                    palette,
                    view.segmented(
                        "url-mode",
                        &["结构拆解", "百分号编码", "百分号解码"],
                        *mode,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Converter(Self::Url { mode, .. }) = &mut view.model {
                                *mode = idx;
                            }
                        },
                    ),
                ));
                if *mode != 0 {
                    groups.push(option_group(
                        "范围",
                        palette,
                        view.segmented(
                            "url-target",
                            &["完整 URL", "组件级"],
                            usize::from(*component),
                            palette,
                            cx,
                            |view, idx| {
                                if let Model::Converter(Self::Url { component, .. }) =
                                    &mut view.model
                                {
                                    *component = idx == 1;
                                }
                            },
                        ),
                    ));
                    groups.push(option_group(
                        "空格",
                        palette,
                        view.segmented(
                            "url-space",
                            &["%20", "+"],
                            usize::from(*plus),
                            palette,
                            cx,
                            |view, idx| {
                                if let Model::Converter(Self::Url { plus, .. }) = &mut view.model {
                                    *plus = idx == 1;
                                }
                            },
                        ),
                    ));
                    groups.push(view.toggle_pill(
                        "url-lines",
                        "逐行处理",
                        *lines,
                        palette,
                        cx,
                        |view| {
                            if let Model::Converter(Self::Url { lines, .. }) = &mut view.model {
                                *lines = !*lines;
                            }
                        },
                    ));
                }
            }
            Self::Uuid {
                kind,
                count,
                uppercase,
                hyphens,
                format,
            } => {
                groups.push(option_group(
                    "版本",
                    palette,
                    view.segmented(
                        "uuid-kind",
                        &["UUID v4", "UUID v7", "UUID v1", "NIL", "ULID"],
                        *kind,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Converter(Self::Uuid { kind, .. }) = &mut view.model {
                                *kind = idx;
                            }
                        },
                    ),
                ));
                let counts = [1usize, 5, 10, 50, 100];
                let count_idx = counts.iter().position(|c| c == count).unwrap_or(0);
                groups.push(option_group(
                    "数量",
                    palette,
                    view.segmented(
                        "uuid-count",
                        &["1", "5", "10", "50", "100"],
                        count_idx,
                        palette,
                        cx,
                        move |view, idx| {
                            if let Model::Converter(Self::Uuid { count, .. }) = &mut view.model {
                                *count = counts[idx];
                            }
                        },
                    ),
                ));
                groups.push(option_group(
                    "输出格式",
                    palette,
                    view.segmented(
                        "uuid-fmt",
                        &["每行一个", "JSON 数组", "带双引号"],
                        *format,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Converter(Self::Uuid { format, .. }) = &mut view.model {
                                *format = idx;
                            }
                        },
                    ),
                ));
                groups.push(view.toggle_pill(
                    "uuid-upper",
                    "大写",
                    *uppercase,
                    palette,
                    cx,
                    |view| {
                        if let Model::Converter(Self::Uuid { uppercase, .. }) = &mut view.model {
                            *uppercase = !*uppercase;
                        }
                    },
                ));
                groups.push(view.toggle_pill(
                    "uuid-hyphen",
                    "连字符",
                    *hyphens,
                    palette,
                    cx,
                    |view| {
                        if let Model::Converter(Self::Uuid { hyphens, .. }) = &mut view.model {
                            *hyphens = !*hyphens;
                        }
                    },
                ));
                groups.push(view.action_pill(
                    "regenerate",
                    Some(IconName::RefreshCw),
                    "重新生成",
                    palette,
                    cx,
                    |_| {},
                ));
            }
            Self::Password {
                token,
                length,
                bytes,
                encoding,
                count,
                lower,
                upper,
                digits,
                symbols,
                ambiguous,
            } => {
                groups.push(option_group(
                    "类型",
                    palette,
                    view.segmented(
                        "pw-kind",
                        &["随机密码", "密钥 Token"],
                        usize::from(*token),
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Converter(Self::Password { token, .. }) = &mut view.model
                            {
                                *token = idx == 1;
                            }
                        },
                    ),
                ));
                let counts = [1usize, 5, 10, 20, 50];
                let count_idx = counts.iter().position(|c| c == count).unwrap_or(0);
                groups.push(option_group(
                    "数量",
                    palette,
                    view.segmented(
                        "pw-count",
                        &["1", "5", "10", "20", "50"],
                        count_idx,
                        palette,
                        cx,
                        move |view, idx| {
                            if let Model::Converter(Self::Password { count, .. }) = &mut view.model
                            {
                                *count = counts[idx];
                            }
                        },
                    ),
                ));
                if *token {
                    let byte_opts = [16usize, 32, 64];
                    let byte_idx = byte_opts.iter().position(|b| b == bytes).unwrap_or(1);
                    groups.push(option_group(
                        "字节长度",
                        palette,
                        view.segmented(
                            "tok-bytes",
                            &["16 B", "32 B", "64 B"],
                            byte_idx,
                            palette,
                            cx,
                            move |view, idx| {
                                if let Model::Converter(Self::Password { bytes, .. }) =
                                    &mut view.model
                                {
                                    *bytes = byte_opts[idx];
                                }
                            },
                        ),
                    ));
                    groups.push(option_group(
                        "编码",
                        palette,
                        view.segmented(
                            "tok-enc",
                            &["Hex", "Base64URL", "Base32"],
                            *encoding,
                            palette,
                            cx,
                            |view, idx| {
                                if let Model::Converter(Self::Password { encoding, .. }) =
                                    &mut view.model
                                {
                                    *encoding = idx;
                                }
                            },
                        ),
                    ));
                } else {
                    let cur_len = *length;
                    groups.push(option_group(
                        "长度",
                        palette,
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(view.action_pill(
                                "pw-len-minus",
                                Some(IconName::Minus),
                                "",
                                palette,
                                cx,
                                |view| {
                                    if let Model::Converter(Self::Password { length, .. }) =
                                        &mut view.model
                                    {
                                        *length = length.saturating_sub(2).clamp(4, 128);
                                    }
                                },
                            ))
                            .child(
                                div()
                                    .px_2()
                                    .text_size(px(11.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(palette.accent))
                                    .child(format!("{cur_len} 位")),
                            )
                            .child(view.action_pill(
                                "pw-len-plus",
                                Some(IconName::Plus),
                                "",
                                palette,
                                cx,
                                |view| {
                                    if let Model::Converter(Self::Password { length, .. }) =
                                        &mut view.model
                                    {
                                        *length = length.saturating_add(2).clamp(4, 128);
                                    }
                                },
                            )),
                    ));
                    groups.push(
                        view.toggle_pill("pw-lower", "a-z", *lower, palette, cx, |v| {
                            if let Model::Converter(Self::Password { lower, .. }) = &mut v.model {
                                *lower = !*lower;
                            }
                        }),
                    );
                    groups.push(
                        view.toggle_pill("pw-upper", "A-Z", *upper, palette, cx, |v| {
                            if let Model::Converter(Self::Password { upper, .. }) = &mut v.model {
                                *upper = !*upper;
                            }
                        }),
                    );
                    groups.push(
                        view.toggle_pill("pw-digits", "0-9", *digits, palette, cx, |v| {
                            if let Model::Converter(Self::Password { digits, .. }) = &mut v.model {
                                *digits = !*digits;
                            }
                        }),
                    );
                    groups.push(view.toggle_pill(
                        "pw-sym",
                        "特殊符号",
                        *symbols,
                        palette,
                        cx,
                        |v| {
                            if let Model::Converter(Self::Password { symbols, .. }) = &mut v.model {
                                *symbols = !*symbols;
                            }
                        },
                    ));
                    groups.push(view.toggle_pill(
                        "pw-ambig",
                        "排除易混字符",
                        *ambiguous,
                        palette,
                        cx,
                        |v| {
                            if let Model::Converter(Self::Password { ambiguous, .. }) = &mut v.model
                            {
                                *ambiguous = !*ambiguous;
                            }
                        },
                    ));
                }
                groups.push(view.action_pill(
                    "regenerate",
                    Some(IconName::RefreshCw),
                    "重新生成",
                    palette,
                    cx,
                    |_| {},
                ));
            }
            Self::Qr {
                mode,
                ec,
                security,
                hidden,
            } => {
                groups.push(option_group(
                    "模式",
                    palette,
                    view.segmented(
                        "qr-mode",
                        &["文本", "URL", "WiFi 配网", "图片解码"],
                        *mode,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Converter(Self::Qr { mode, .. }) = &mut view.model {
                                *mode = idx;
                            }
                        },
                    ),
                ));
                if *mode != 3 {
                    groups.push(option_group(
                        "容错等级",
                        palette,
                        view.segmented(
                            "qr-ec",
                            &["L (7%)", "M (15%)", "Q (25%)", "H (30%)"],
                            *ec,
                            palette,
                            cx,
                            |view, idx| {
                                if let Model::Converter(Self::Qr { ec, .. }) = &mut view.model {
                                    *ec = idx;
                                }
                            },
                        ),
                    ));
                }
                if *mode == 2 {
                    groups.push(option_group(
                        "加密",
                        palette,
                        view.segmented(
                            "qr-wifi-sec",
                            &["WPA/WPA2", "WEP", "开放网络"],
                            *security,
                            palette,
                            cx,
                            |view, idx| {
                                if let Model::Converter(Self::Qr { security, .. }) = &mut view.model
                                {
                                    *security = idx;
                                }
                            },
                        ),
                    ));
                    groups.push(view.toggle_pill(
                        "qr-hidden",
                        "隐藏 SSID",
                        *hidden,
                        palette,
                        cx,
                        |view| {
                            if let Model::Converter(Self::Qr { hidden, .. }) = &mut view.model {
                                *hidden = !*hidden;
                            }
                        },
                    ));
                }
            }
        }

        let _ = icon_el;
        options_bar(palette, groups)
    }
}
