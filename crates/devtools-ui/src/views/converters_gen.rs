//! Dedicated conversion, codec and generator controllers.

use devtools_core::{CancelFlag, RawPayload, ToolError, ToolErrorKind, ToolId, ToolLaunchInput};
use devtools_tools::{
    conversion::{color, datetime, json_table, number_base},
    encode_decode::{base64, url},
    generator::{password, qr, uuid_ulid},
};
use gpui_kit::*;

use super::Model;
use super::runtime::{Inputs, Slot, ToolResult, ToolView};
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
                uppercase: false,
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
                    ("Local".into(), out.local),
                    ("UTC / ISO 8601".into(), out.utc),
                    (out.timezone_name, out.timezone),
                    ("Unix 秒".into(), out.unix_seconds.to_string()),
                    ("Unix 毫秒".into(), out.unix_millis.to_string()),
                    ("Unix 微秒".into(), out.unix_micros.to_string()),
                    ("Unix 纳秒".into(), out.unix_nanos.to_string()),
                    (
                        "星期 / ISO 周".into(),
                        format!("{} / {}", out.weekday, out.iso_week),
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
                ToolResult::rows(vec![
                    ("BIN · 2".into(), out.binary),
                    ("OCT · 8".into(), out.octal),
                    ("DEC · 10".into(), out.decimal),
                    ("HEX · 16".into(), out.hexadecimal),
                    (format!("自定义 · {custom}"), out.custom.unwrap_or_default()),
                    ("有符号值".into(), out.signed_value),
                    ("有效位数".into(), out.bits.to_string()),
                ])
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
                    ("Alpha".into(), format!("{}%", out.alpha_percent)),
                ]);
                for (background, contrast) in
                    [("#FFF", out.against_white), ("#000", out.against_black)]
                {
                    result.rows.push((
                        format!("WCAG {background}"),
                        format!(
                            "{:.2}:1 · AA {} · AAA {}",
                            contrast.ratio,
                            if contrast.aa { "✓" } else { "×" },
                            if contrast.aaa { "✓" } else { "×" }
                        ),
                    ));
                }
                result.swatch = Some((u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b));
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
                            vec!["Query Key".into(), "Value（保序，允许同名）".into()],
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
                if !inputs.text.trim().is_empty() {
                    let info = uuid_ulid::inspect(inputs.text.trim())?;
                    result.rows = vec![
                        ("生成结果".into(), result.output.clone()),
                        ("Inspector".into(), info.format),
                        (
                            "Version".into(),
                            info.version.map(|v| v.to_string()).unwrap_or_default(),
                        ),
                        (
                            "Timestamp".into(),
                            info.timestamp.unwrap_or_else(|| "无时间戳".into()),
                        ),
                        ("NIL".into(), info.is_nil.to_string()),
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
                                "香农熵 {:.1} bits · zxcvbn {} / 4 · {}",
                                s.shannon_bits, s.zxcvbn_score, s.crack_time
                            )
                        })
                        .unwrap_or_else(|| format!("敏感 Token · {bytes} bytes")),
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
                        footer: format!("解码成功 · {}", out.preprocessing),
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
                            "QR v{} · {} 模块 · SVG 全量复制",
                            out.version, out.module_count
                        ),
                        ..ToolResult::default()
                    }
                }
            }
        })
    }

    pub fn inputs(&self, view: &ToolView) -> AnyElement {
        let mut inputs = div().flex().flex_col().gap_3();
        match self {
            Self::Password { token, .. } => {
                if !token {
                    inputs = inputs.child(view.field(Slot::Query, "自定义符号（留空使用默认）"));
                }
            }
            Self::Uuid { .. } => {
                inputs =
                    inputs.child(view.field(Slot::Text, "Inspector · 粘贴 UUID / ULID（可选）"))
            }
            Self::Base64 {
                data_uri: true,
                decode: false,
                ..
            } => {
                inputs = inputs
                    .child(view.field(Slot::Query, "文件路径（可选；支持拖放 / 智能粘贴图片）"))
                    .child(view.field(Slot::Text, "文本（未选择文件时）"));
            }
            Self::Qr { mode: 3, .. } => {
                inputs =
                    inputs.child(view.field(Slot::Query, "待解码图片文件路径（或智能粘贴图片）"))
            }
            Self::Qr { mode: 2, .. } => {
                inputs = inputs
                    .child(view.field(Slot::Text, "WiFi SSID"))
                    .child(view.field(Slot::Secret, "WiFi 密码"))
            }
            Self::Url { mode: 0, .. } => {
                inputs = inputs
                    .child(view.field(
                        Slot::Query,
                        "Query 编辑（key=value&key=value，保留重复键；留空使用 URL）",
                    ))
                    .child(view.field(Slot::Text, "URL"));
            }
            Self::Table { .. } => {
                inputs = inputs
                    .child(view.field(Slot::Query, "拍平分隔符（默认 .）"))
                    .child(view.field(Slot::Text, "JSON 输入"))
            }
            _ => inputs = inputs.child(view.field(Slot::Text, "输入（支持智能粘贴 / 工具接力）")),
        }
        inputs.into_any_element()
    }

    pub fn controls(
        &self,
        view: &ToolView,
        palette: Palette,
        cx: &Context<ToolView>,
    ) -> AnyElement {
        let mut bar = div().flex().flex_wrap().gap_2().p_2();
        macro_rules! choices {
            ($variant:ident, $field:ident, $value:expr, $labels:expr) => {
                for (index, label) in $labels.into_iter().enumerate() {
                    bar = bar.child(view.chip(
                        &format!("{}-{index}", stringify!($field)),
                        label,
                        *$value == index,
                        palette,
                        cx,
                        move |view| {
                            if let Model::Converter(Self::$variant { $field, .. }) = &mut view.model
                            {
                                *$field = index;
                            }
                        },
                    ));
                }
            };
        }
        macro_rules! toggle {
            ($variant:ident, $field:ident, $value:expr, $label:expr) => {
                bar = bar.child(view.chip(
                    stringify!($field),
                    $label,
                    *$value,
                    palette,
                    cx,
                    |view| {
                        if let Model::Converter(Self::$variant { $field, .. }) = &mut view.model {
                            *$field = !*$field;
                        }
                    },
                ));
            };
        }
        macro_rules! numeric {
            ($variant:ident,$field:ident,$value:expr,$values:expr,$label:expr) => {
                for value in $values {
                    bar = bar.child(view.chip(
                        &format!("{}-{value}", stringify!($field)),
                        format!("{} {value}", $label),
                        *$value == value,
                        palette,
                        cx,
                        move |view| {
                            if let Model::Converter(Self::$variant { $field, .. }) = &mut view.model
                            {
                                *$field = value;
                            }
                        },
                    ));
                }
            };
        }
        match self {
            Self::Date { unit, zone } => {
                choices!(Date, unit, unit, ["自动单位", "秒", "毫秒", "微秒", "纳秒"]);
                choices!(Date, zone, zone, ZONES);
                bar = bar.child(view.chip("now", "Now", false, palette, cx, |view| {
                    view.inputs.text = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                        .to_string();
                }));
            }
            Self::Number {
                base,
                custom,
                bits,
                uppercase,
                prefixes,
                group,
            } => {
                numeric!(Number, base, base, [0, 2, 8, 10, 16], "源");
                for delta in [-1i16, 1] {
                    bar = bar.child(view.chip(
                        &format!("base-step-{delta}"),
                        format!("源进制 {}", if delta < 0 { "−" } else { "+" }),
                        false,
                        palette,
                        cx,
                        move |view| {
                            if let Model::Converter(Self::Number { base, .. }) = &mut view.model {
                                *base = (i16::from(*base) + delta).clamp(2, 36) as u8;
                            }
                        },
                    ));
                }
                bar = bar.child(
                    div()
                        .text_xs()
                        .child(format!("源 {base} · 额外 {custom} 进制")),
                );
                for delta in [-1i16, 1] {
                    bar = bar.child(view.chip(
                        &format!("custom-step-{delta}"),
                        format!("额外 {}", if delta < 0 { "−" } else { "+" }),
                        false,
                        palette,
                        cx,
                        move |view| {
                            if let Model::Converter(Self::Number { custom, .. }) = &mut view.model {
                                *custom = (i16::from(*custom) + delta).clamp(2, 36) as u8;
                            }
                        },
                    ));
                }
                choices!(
                    Number,
                    bits,
                    bits,
                    ["数学有符号", "补码 8", "补码 16", "补码 32", "补码 64"]
                );
                toggle!(Number, uppercase, uppercase, "大写");
                toggle!(Number, prefixes, prefixes, "前缀");
                toggle!(Number, group, group, "分组");
            }
            Self::Color => {
                bar = bar.child(view.color_picker());
                bar = bar.child(
                    div()
                        .text_xs()
                        .child("CSS Color 4 · 点击结果行复制 · WCAG AA / AAA"),
                );
            }
            Self::Table {
                format,
                flatten,
                bom,
            } => {
                choices!(Table, format, format, ["Table", "CSV", "TSV", "Markdown"]);
                toggle!(Table, flatten, flatten, "拍平对象");
                toggle!(Table, bom, bom, "UTF-8 BOM");
            }
            Self::Base64 {
                decode,
                data_uri,
                url_safe,
                padding,
                lines,
            } => {
                toggle!(Base64, decode, decode, "解码（关闭为编码）");
                toggle!(Base64, data_uri, data_uri, "Data URI");
                toggle!(Base64, url_safe, url_safe, "URL-safe");
                toggle!(Base64, padding, padding, "= 填充");
                toggle!(Base64, lines, lines, "逐行");
            }
            Self::Url {
                mode,
                component,
                plus,
                lines,
            } => {
                choices!(Url, mode, mode, ["Parse", "Encode", "Decode"]);
                toggle!(Url, component, component, "组件级");
                toggle!(Url, plus, plus, "空格 +（关闭为 %20）");
                toggle!(Url, lines, lines, "逐行");
            }
            Self::Uuid {
                kind,
                count,
                uppercase,
                hyphens,
                format,
            } => {
                choices!(Uuid, kind, kind, ["v4", "v7", "v1", "NIL", "ULID"]);
                numeric!(Uuid, count, count, [1, 5, 10, 50, 100], "批量");
                choices!(Uuid, format, format, ["行", "JSON", "引号"]);
                toggle!(Uuid, uppercase, uppercase, "大写");
                toggle!(Uuid, hyphens, hyphens, "连字符");
                bar = bar.child(view.chip("regenerate", "重新生成", false, palette, cx, |_| {}));
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
                toggle!(Password, token, token, "Token（关闭为密码）");
                numeric!(Password, count, count, [1, 5, 10, 50, 100], "批量");
                if *token {
                    numeric!(Password, bytes, bytes, [16, 32, 64], "字节");
                    choices!(Password, encoding, encoding, ["Hex", "B64URL", "Base32"]);
                } else {
                    for delta in [-1isize, 1] {
                        bar = bar.child(view.chip(
                            &format!("length-{delta}"),
                            format!("长度 {}", if delta < 0 { "−" } else { "+" }),
                            false,
                            palette,
                            cx,
                            move |view| {
                                if let Model::Converter(Self::Password { length, .. }) =
                                    &mut view.model
                                {
                                    *length = length.saturating_add_signed(delta).clamp(4, 128);
                                }
                            },
                        ));
                    }
                    bar = bar.child(div().text_xs().child(format!("{length} 字符")));
                    toggle!(Password, lower, lower, "a-z");
                    toggle!(Password, upper, upper, "A-Z");
                    toggle!(Password, digits, digits, "0-9");
                    toggle!(Password, symbols, symbols, "符号");
                    toggle!(Password, ambiguous, ambiguous, "排除易混字符");
                }
                bar = bar.child(view.chip("regenerate", "重新生成", false, palette, cx, |_| {}));
            }
            Self::Qr {
                mode,
                ec,
                security,
                hidden,
            } => {
                choices!(Qr, mode, mode, ["文本", "URL", "WiFi", "解码"]);
                choices!(Qr, ec, ec, ["ECC L", "ECC M", "ECC Q", "ECC H"]);
                if *mode == 2 {
                    choices!(Qr, security, security, ["WPA", "WEP", "开放"]);
                    toggle!(Qr, hidden, hidden, "隐藏网络");
                }
            }
        }
        bar.into_any_element()
    }
}

use std::sync::Arc;
