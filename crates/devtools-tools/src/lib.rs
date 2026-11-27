//! 28 工具纯计算内核（ADR-0004）。
//!
//! 本 crate 为零 GPUI / 零 `tokio` 依赖的纯同步/流式工具内核集合，按 7 分类
//! 分模块、以 Cargo feature 裁剪。所有工具均为纯函数：输入强类型请求、输出强类型
//! 响应，长耗时或批量操作额外接受共享 [`CancelFlag`] 协作取消与进度回调，运行于
//! GPUI `BackgroundExecutor`。

pub use devtools_core;
pub use devtools_core::error::{CancelFlag, ToolError, ToolErrorKind};

/// 长耗时操作（当前为文件流式哈希）的进度回调参数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// 已处理字节数。
    pub processed: u64,
    /// 总字节数；未知时为 `None`。
    pub total: Option<u64>,
}

impl Progress {
    pub const fn new(processed: u64, total: Option<u64>) -> Self {
        Self { processed, total }
    }

    /// 完成比例（0.0–1.0）；总长未知或为零时为 0.0。
    pub fn fraction(&self) -> f64 {
        match self.total {
            Some(total) if total > 0 => (self.processed as f64 / total as f64).clamp(0.0, 1.0),
            _ => 0.0,
        }
    }
}

/// 进度回调签名：返回 `false` 表示请求中止。
pub type ProgressFn<'a> = dyn FnMut(Progress) -> bool + 'a;

#[cfg(any(
    feature = "conversion",
    feature = "encode-decode",
    feature = "format",
    feature = "generator"
))]
mod util;

#[cfg(feature = "conversion")]
pub mod conversion;
#[cfg(feature = "debug")]
pub mod debug;
#[cfg(feature = "encode-decode")]
pub mod encode_decode;
#[cfg(feature = "format")]
pub mod format;
#[cfg(feature = "generator")]
pub mod generator;
#[cfg(feature = "security")]
pub mod security;
#[cfg(feature = "text")]
pub mod text;

use devtools_core::{ToolCapabilities, ToolId, UiTrack};

/// 快速预览中双输入工具（文本对比 / JSON 对比 / 列表对比）的左右分隔符。
pub const QUICK_INPUT_SEPARATOR: &str = "\n---\n";

/// 当前 feature 组合下已编译进本 crate 的工具集合（按分类声明顺序）。
pub fn implemented_tools() -> Vec<ToolId> {
    let mut tools = Vec::new();
    #[cfg(feature = "conversion")]
    tools.extend_from_slice(&conversion::TOOL_IDS);
    #[cfg(feature = "encode-decode")]
    tools.extend_from_slice(&encode_decode::TOOL_IDS);
    #[cfg(feature = "format")]
    tools.extend_from_slice(&format::TOOL_IDS);
    #[cfg(feature = "generator")]
    tools.extend_from_slice(&generator::TOOL_IDS);
    #[cfg(feature = "text")]
    tools.extend_from_slice(&text::TOOL_IDS);
    #[cfg(feature = "debug")]
    tools.extend_from_slice(&debug::TOOL_IDS);
    #[cfg(feature = "security")]
    tools.extend_from_slice(&security::TOOL_IDS);
    tools
}

/// 该工具内核是否已编译进本 crate。
pub fn is_implemented(id: ToolId) -> bool {
    implemented_tools().contains(&id)
}

/// 已实现工具的能力声明；未实现时返回 `None`。
pub fn capabilities(id: ToolId) -> Option<ToolCapabilities> {
    is_implemented(id).then(|| id.default_capabilities())
}

/// 全部已实现工具是否都满足 V1 的 100% 离线契约。
pub fn all_offline() -> bool {
    implemented_tools()
        .iter()
        .all(|id| id.default_capabilities().offline)
}

fn not_compiled(tool: ToolId) -> ToolError {
    ToolError::new(
        ToolErrorKind::UnsupportedInput,
        format!("工具 {} 未在当前 feature 组合中编译", tool.as_str()),
    )
}

/// 按 UX 默认参数执行 7 个 [`UiTrack::StandardTransform`] 工具中的任意一个。
///
/// 默认管道：输入 → 解析 / 编码 → 文本输出。非 StandardTransform 工具返回
/// [`ToolErrorKind::UnsupportedInput`]。
pub fn standard_transform(tool: ToolId, input: &str) -> Result<String, ToolError> {
    match tool {
        #[cfg(feature = "conversion")]
        ToolId::DataFormatConverter => {
            use conversion::data_format::{
                DataFormat, DataFormatConverterRequest, IndentStyle, convert,
            };
            let from = detect_data_format(input);
            let to = if from == DataFormat::Json {
                DataFormat::Yaml
            } else {
                DataFormat::Json
            };
            let req = DataFormatConverterRequest {
                input: input.to_owned(),
                from,
                to,
                indent: IndentStyle::Two,
                sort_keys: false,
            };
            Ok(convert(&req)?.output)
        }
        #[cfg(feature = "encode-decode")]
        ToolId::HtmlEntityUnicode => {
            use encode_decode::html_unicode::{
                HtmlEntityUnicodeRequest, HtmlUnicodeMode, UnicodeEscapeStyle, run,
            };
            let req = HtmlEntityUnicodeRequest {
                input: input.to_owned(),
                mode: HtmlUnicodeMode::Encode,
                style: UnicodeEscapeStyle::HtmlEntity,
                non_ascii_only: false,
            };
            Ok(run(&req)?.output)
        }
        #[cfg(feature = "encode-decode")]
        ToolId::StringEscape => {
            use encode_decode::string_escape::{
                EscapeDirection, EscapeStyle, StringEscapeRequest, run,
            };
            let req = StringEscapeRequest {
                input: input.to_owned(),
                style: EscapeStyle::Json,
                direction: EscapeDirection::Escape,
                line_by_line: false,
            };
            Ok(run(&req)?.output)
        }
        #[cfg(feature = "encode-decode")]
        ToolId::GzipCompress => {
            use encode_decode::gzip::{GzipAction, GzipCompressRequest, GzipTransport, run};
            let req = GzipCompressRequest {
                action: GzipAction::Compress,
                input: input.to_owned(),
                transport: GzipTransport::Base64,
                level: 6,
            };
            Ok(run(&req, None)?.output)
        }
        #[cfg(feature = "format")]
        ToolId::JsonFormatter => {
            use format::json::{JsonFormatterRequest, JsonIndent, run};
            let req = JsonFormatterRequest {
                input: input.to_owned(),
                pretty: true,
                indent: JsonIndent::Two,
                sort_keys: false,
            };
            Ok(run(&req)?.output)
        }
        #[cfg(feature = "format")]
        ToolId::SqlFormatter => {
            use format::sql::{SqlFormatterRequest, SqlIndent, SqlKeywordCase, SqlLayout, run};
            let req = SqlFormatterRequest {
                input: input.to_owned(),
                keyword_case: SqlKeywordCase::Upper,
                layout: SqlLayout::Expanded,
                indent: SqlIndent::Two,
            };
            Ok(run(&req)?.output)
        }
        #[cfg(feature = "format")]
        ToolId::XmlHtmlFormatter => {
            use format::markup::{MarkupIndent, MarkupLanguage, XmlHtmlFormatterRequest, run};
            let req = XmlHtmlFormatterRequest {
                input: input.to_owned(),
                language: MarkupLanguage::Auto,
                pretty: true,
                indent: MarkupIndent::Two,
            };
            Ok(run(&req)?.output)
        }
        other => Err(ToolError::new(
            ToolErrorKind::UnsupportedInput,
            format!("工具 {} 不属于 StandardTransform 文本模板", other.as_str()),
        )),
    }
}

/// 任意工具的一次性文本预览执行。
///
/// StandardTransform 工具直接复用 [`standard_transform`]；其余 21 个自定义视图
/// 工具以合理默认参数执行并返回文本化结果，供 UI 快速预览与测试使用。
/// 双输入工具（TextDiff / JsonDiff / ListCompare）以 [`QUICK_INPUT_SEPARATOR`]
/// 分隔左右两部分；RegexTester 以首行为模式、其余为测试文本；JsonPathTester
/// 以首行为路径、其余为 JSON 文档。
pub fn run_quick_transform(tool: ToolId, input: &str) -> Result<String, ToolError> {
    if !is_implemented(tool) {
        return Err(not_compiled(tool));
    }
    if tool.ui_track() == UiTrack::StandardTransform {
        return standard_transform(tool, input);
    }
    match tool {
        #[cfg(feature = "conversion")]
        ToolId::DateTimeConverter => {
            use conversion::datetime::{DateTimeConverterRequest, TimestampUnit, convert};
            let req = DateTimeConverterRequest {
                input: input.to_owned(),
                timezone: "UTC".to_owned(),
                timestamp_unit: TimestampUnit::Auto,
            };
            let out = convert(&req)?;
            Ok(format!(
                "UTC: {}\nUnix(s): {}\nUnix(ms): {}\nWeekday: {}\nISO week: {}",
                out.utc, out.unix_seconds, out.unix_millis, out.weekday, out.iso_week
            ))
        }
        #[cfg(feature = "conversion")]
        ToolId::NumberBaseConverter => {
            use conversion::number_base::{NumberBaseConverterRequest, SignedMode, convert};
            let req = NumberBaseConverterRequest {
                input: input.to_owned(),
                from_base: 0,
                signed_mode: SignedMode::Mathematical,
                bit_width: None,
                custom_base: None,
                uppercase: false,
                prefixes: true,
                group: false,
            };
            Ok(convert(&req)?.decimal)
        }
        #[cfg(feature = "conversion")]
        ToolId::ColorConverter => {
            use conversion::color::{ColorConverterRequest, convert};
            let req = ColorConverterRequest {
                input: input.to_owned(),
            };
            Ok(convert(&req)?.hex)
        }
        #[cfg(feature = "conversion")]
        ToolId::JsonToTable => {
            use conversion::json_table::{JsonToTableRequest, TableExportFormat, convert};
            let req = JsonToTableRequest {
                input: input.to_owned(),
                flatten: true,
                separator: ".".to_owned(),
                export_format: TableExportFormat::Csv,
                bom: true,
            };
            Ok(convert(&req)?.export)
        }
        #[cfg(feature = "encode-decode")]
        ToolId::Base64Codec => {
            use encode_decode::base64::{Base64CodecRequest, Base64Mode, Base64Payload, run};
            let mode = if looks_like_base64(input) {
                Base64Mode::Decode
            } else {
                Base64Mode::Encode
            };
            let req = Base64CodecRequest {
                mode,
                payload: Base64Payload::Text(input.to_owned()),
                url_safe: false,
                padding: true,
                line_by_line: false,
            };
            let out = run(&req)?;
            Ok(out.text.or(out.data_url).unwrap_or_else(|| {
                String::from_utf8_lossy(out.bytes.as_deref().unwrap_or_default()).into_owned()
            }))
        }
        #[cfg(feature = "encode-decode")]
        ToolId::UrlCodec => {
            use encode_decode::url::{UrlCodecRequest, UrlMode, UrlTarget, run};
            let req = UrlCodecRequest {
                input: input.to_owned(),
                mode: UrlMode::Parse,
                target: UrlTarget::FullUrl,
                space_as_plus: false,
                line_by_line: false,
            };
            Ok(run(&req)?.output)
        }
        #[cfg(feature = "generator")]
        ToolId::UuidUlidGenerator => {
            use generator::uuid_ulid::{IdKind, IdOutputFormat, UuidUlidRequest, generate};
            let req = UuidUlidRequest {
                kind: IdKind::V4,
                count: 1,
                uppercase: false,
                hyphens: true,
                format: IdOutputFormat::Plain,
                one_per_line: true,
            };
            Ok(generate(&req, None)?.output)
        }
        #[cfg(feature = "generator")]
        ToolId::PasswordTokenGenerator => {
            use generator::password::{
                PasswordOptions, PasswordTokenRequest, SecretKind, SecretOutput, run,
            };
            let req = PasswordTokenRequest {
                kind: SecretKind::Password(PasswordOptions::default()),
                count: 1,
                output: SecretOutput::Plain,
            };
            Ok(run(&req, None)?.output)
        }
        #[cfg(feature = "generator")]
        ToolId::QrCodeGenerator => {
            use generator::qr::{QrContent, QrErrorCorrection, QrGenerateRequest, generate};
            let req = QrGenerateRequest {
                content: QrContent::Text(input.to_owned()),
                ec: QrErrorCorrection::M,
                size: None,
            };
            Ok(generate(&req)?.svg)
        }
        #[cfg(feature = "text")]
        ToolId::TextDiff => {
            use text::{TextDiffOptions, export_unified_diff};
            let (old, new) = split_quick_input(input)?;
            export_unified_diff(old, new, &TextDiffOptions::default())
        }
        #[cfg(feature = "text")]
        ToolId::JsonDiff => {
            use text::{JsonDiffOptions, diff_json};
            let (left, right) = split_quick_input(input)?;
            Ok(diff_json(left, right, &JsonDiffOptions::default())?.report)
        }
        #[cfg(feature = "text")]
        ToolId::TextAnalysis => {
            let stats = text::analyze_text(input);
            Ok(format!(
                "graphemes: {}\nbytes: {}\nwords: {}\nlines: {}\nparagraphs: {}",
                stats.graphemes, stats.bytes, stats.words, stats.lines, stats.paragraphs
            ))
        }
        #[cfg(feature = "text")]
        ToolId::ListCompare => {
            use text::{ListCompareOptions, compare_lists};
            let (a, b) = split_quick_input(input)?;
            let result = compare_lists(a, b, &ListCompareOptions::default());
            Ok(format!(
                "only_a: {}\nonly_b: {}\nintersection: {}\nunion: {}",
                result.only_a.join(", "),
                result.only_b.join(", "),
                result.intersection.join(", "),
                result.union.join(", ")
            ))
        }
        #[cfg(feature = "debug")]
        ToolId::RegexTester => {
            use debug::{RegexFlags, test_regex};
            let (pattern, subject) = input
                .split_once('\n')
                .map_or((input, ""), |(pattern, rest)| (pattern, rest));
            let flags = RegexFlags {
                g: true,
                ..RegexFlags::default()
            };
            let result = test_regex(
                pattern,
                subject,
                &flags,
                None,
                false,
                false,
                &CancelFlag::new(),
            )?;
            let mut out = String::new();
            for m in &result.matches {
                out.push_str(&format!("{}..{}: {}\n", m.start, m.end, m.text));
            }
            Ok(out)
        }
        #[cfg(feature = "debug")]
        ToolId::JsonPathTester => {
            let (path, json) = input.split_once('\n').ok_or_else(|| {
                ToolError::new(
                    ToolErrorKind::UnsupportedInput,
                    "JSONPath 预览需要首行为路径、其余为 JSON 文档",
                )
            })?;
            let result = debug::query_json_path(json, path)?;
            let values: Vec<&serde_json::Value> = result.matches.iter().map(|m| &m.value).collect();
            serde_json::to_string_pretty(&values).map_err(|err| {
                ToolError::new(
                    ToolErrorKind::Internal,
                    format!("JSONPath 结果序列化失败：{err}"),
                )
            })
        }
        #[cfg(feature = "debug")]
        ToolId::CronParser => {
            use debug::{TriggerCount, parse_cron};
            let result = parse_cron(input.trim(), "UTC", TriggerCount::Ten)?;
            let mut out = format!("{}\n", result.description_zh);
            for trigger in &result.next_triggers {
                out.push_str(trigger);
                out.push('\n');
            }
            Ok(out)
        }
        #[cfg(feature = "debug")]
        ToolId::ChmodCalculator => {
            use debug::Permissions;
            let raw = input.trim();
            let perms = if !raw.is_empty() && raw.bytes().all(|b| b.is_ascii_digit()) {
                let value = u16::from_str_radix(raw, 8).map_err(|err| {
                    ToolError::new(
                        ToolErrorKind::InvalidSyntax,
                        format!("八进制权限无效：{err}"),
                    )
                })?;
                Permissions::from_octal(value)?
            } else {
                Permissions::from_symbolic(raw)?
            };
            Ok(format!(
                "{} {} {}",
                perms.to_octal(),
                perms.to_symbolic(),
                perms.to_command("path")
            ))
        }
        #[cfg(feature = "security")]
        ToolId::JwtTool => {
            let decoded = security::decode_without_verification(input.trim())?;
            let header = serde_json::to_string_pretty(&decoded.header).map_err(serialize_error)?;
            let claims = serde_json::to_string_pretty(&decoded.claims).map_err(serialize_error)?;
            Ok(format!(
                "algorithm: {}\nstatus: {}\nheader:\n{}\nclaims:\n{}",
                decoded.algorithm,
                decoded.status.label_zh(),
                header,
                claims
            ))
        }
        #[cfg(feature = "security")]
        ToolId::HashHmac => {
            use security::{HashAlgorithm, hash_text};
            Ok(hash_text(input.as_bytes(), HashAlgorithm::Sha256, None).hex)
        }
        #[cfg(feature = "security")]
        ToolId::X509Parser => {
            let result = security::parse_x509_input(input.as_bytes(), None)?;
            let mut out = String::new();
            for cert in &result.certificates {
                out.push_str(&format!(
                    "{} ({})\n  issuer: {}\n  sha256: {}\n",
                    cert.subject,
                    cert.validity.label_zh(),
                    cert.issuer,
                    cert.fingerprint_sha256
                ));
            }
            Ok(out)
        }
        #[cfg(feature = "security")]
        ToolId::Ipv4CidrCalculator => {
            let result = security::parse_and_analyze(input)?;
            Ok(format!(
                "network: {}\nbroadcast: {}\nnetmask: {}\nhosts: {}\nfirst: {}\nlast: {}",
                result.network,
                result.broadcast,
                result.netmask,
                result.usable_hosts,
                result.first_host,
                result.last_host
            ))
        }
        other => Err(not_compiled(other)),
    }
}

#[cfg(feature = "conversion")]
fn detect_data_format(input: &str) -> conversion::data_format::DataFormat {
    use conversion::data_format::DataFormat;
    let trimmed = input.trim_start();
    if trimmed.starts_with('<') {
        DataFormat::Xml
    } else if trimmed.starts_with('{') || trimmed.starts_with('[') {
        DataFormat::Json
    } else if input
        .lines()
        .any(|line| line.contains('=') && !line.trim_start().starts_with('#'))
    {
        DataFormat::Toml
    } else {
        DataFormat::Yaml
    }
}

#[cfg(feature = "encode-decode")]
fn looks_like_base64(input: &str) -> bool {
    let text = input.trim();
    text.len() >= 8
        && text.len().is_multiple_of(4)
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/' | b'=' | b'-' | b'_'))
}

#[cfg(feature = "text")]
fn split_quick_input(input: &str) -> Result<(&str, &str), ToolError> {
    input.split_once(QUICK_INPUT_SEPARATOR).ok_or_else(|| {
        ToolError::new(
            ToolErrorKind::UnsupportedInput,
            format!(
                "双输入工具需要以 {:?} 分隔左右两部分",
                QUICK_INPUT_SEPARATOR
            ),
        )
        .with_hint("例如：左侧文本\\n---\\n右侧文本")
    })
}

#[cfg(feature = "security")]
fn serialize_error(err: serde_json::Error) -> ToolError {
    ToolError::new(ToolErrorKind::Internal, format!("序列化失败：{err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_implemented_tool_is_declared_offline() {
        assert!(all_offline());
        for tool in implemented_tools() {
            let caps = capabilities(tool).expect("implemented tool must expose capabilities");
            assert!(caps.offline, "{tool:?} must be offline");
        }
    }

    #[test]
    fn unimplemented_tool_has_no_capability_entry() {
        let implemented = implemented_tools();
        if let Some(missing) = ToolId::ALL.into_iter().find(|id| !implemented.contains(id)) {
            assert!(!is_implemented(missing));
            assert!(capabilities(missing).is_none());
        }
    }

    #[test]
    fn progress_fraction_is_bounded() {
        assert_eq!(Progress::new(50, Some(100)).fraction(), 0.5);
        assert_eq!(Progress::new(200, Some(100)).fraction(), 1.0);
        assert_eq!(Progress::new(1, None).fraction(), 0.0);
        assert_eq!(Progress::new(1, Some(0)).fraction(), 0.0);
    }

    #[test]
    fn implemented_tools_cover_all_twenty_eight_with_all_features() {
        #[cfg(all(
            feature = "conversion",
            feature = "encode-decode",
            feature = "format",
            feature = "generator",
            feature = "text",
            feature = "debug",
            feature = "security"
        ))]
        {
            assert_eq!(implemented_tools().len(), ToolId::ALL.len());
            for tool in ToolId::ALL {
                assert!(
                    is_implemented(tool),
                    "{tool:?} missing from implemented set"
                );
            }
        }
    }

    #[cfg(all(feature = "conversion", feature = "encode-decode", feature = "format"))]
    #[test]
    fn standard_transform_covers_all_seven_templates() {
        let cases = [
            (ToolId::DataFormatConverter, r#"{"a":1}"#),
            (ToolId::HtmlEntityUnicode, "a&b"),
            (ToolId::StringEscape, "a\"b"),
            (ToolId::GzipCompress, "hello"),
            (ToolId::JsonFormatter, r#"{"a":1}"#),
            (ToolId::SqlFormatter, "select 1"),
            (ToolId::XmlHtmlFormatter, "<a><b/></a>"),
        ];
        for (tool, input) in cases {
            let out = standard_transform(tool, input)
                .unwrap_or_else(|err| panic!("{tool:?} failed: {err}"));
            assert!(!out.is_empty(), "{tool:?} produced empty output");
        }
        assert!(standard_transform(ToolId::UuidUlidGenerator, "x").is_err());
    }

    #[cfg(feature = "conversion")]
    #[test]
    fn quick_transform_covers_conversion_custom_views() {
        let cases = [
            (ToolId::DateTimeConverter, "2024-01-01T00:00:00Z"),
            (ToolId::NumberBaseConverter, "255"),
            (ToolId::ColorConverter, "#ff0000"),
            (ToolId::JsonToTable, r#"[{"a":1}]"#),
        ];
        for (tool, input) in cases {
            let out = run_quick_transform(tool, input)
                .unwrap_or_else(|err| panic!("{tool:?} failed: {err}"));
            assert!(!out.is_empty(), "{tool:?} produced empty output");
        }
    }

    #[cfg(all(feature = "conversion", not(feature = "encode-decode")))]
    #[test]
    fn uncompiled_category_reports_not_compiled() {
        assert_eq!(
            run_quick_transform(ToolId::Base64Codec, "hello world").unwrap_err(),
            not_compiled(ToolId::Base64Codec)
        );
    }

    #[cfg(feature = "encode-decode")]
    #[test]
    fn quick_transform_base64_encodes_and_decodes() {
        let encoded = run_quick_transform(ToolId::Base64Codec, "hello world").unwrap();
        assert_eq!(encoded, "aGVsbG8gd29ybGQ=");
        let decoded = run_quick_transform(ToolId::Base64Codec, &encoded).unwrap();
        assert_eq!(decoded, "hello world");
    }

    #[cfg(feature = "generator")]
    #[test]
    fn quick_transform_covers_generators() {
        let uuid = run_quick_transform(ToolId::UuidUlidGenerator, "").unwrap();
        assert_eq!(uuid.trim().len(), 36);
        let password = run_quick_transform(ToolId::PasswordTokenGenerator, "").unwrap();
        assert_eq!(password.len(), 16);
        let svg = run_quick_transform(ToolId::QrCodeGenerator, "hello").unwrap();
        assert!(svg.contains("<svg"));
    }

    #[cfg(feature = "text")]
    #[test]
    fn quick_transform_covers_text_tools() {
        let diff = run_quick_transform(ToolId::TextDiff, "a\n---\nb").unwrap();
        assert!(diff.contains("-a"));
        let json_diff = run_quick_transform(
            ToolId::JsonDiff,
            r#"{"a":1}
---
{"a":2}"#,
        )
        .unwrap();
        assert!(json_diff.contains("/a"));
        let stats = run_quick_transform(ToolId::TextAnalysis, "hello world").unwrap();
        assert!(stats.contains("words: 2"));
        let lists = run_quick_transform(ToolId::ListCompare, "a\nb\n---\nb\nc").unwrap();
        assert!(lists.contains("intersection: b"));
        assert!(run_quick_transform(ToolId::TextDiff, "no separator").is_err());
    }

    #[cfg(feature = "debug")]
    #[test]
    fn quick_transform_covers_debug_tools() {
        let regex = run_quick_transform(ToolId::RegexTester, "^a\nabc").unwrap();
        assert!(regex.contains("0..1"));
        let json_path = run_quick_transform(ToolId::JsonPathTester, "$.a\n{\"a\":1}").unwrap();
        assert!(json_path.contains('1'));
        let cron = run_quick_transform(ToolId::CronParser, "*/5 * * * *").unwrap();
        assert!(!cron.is_empty());
        let chmod = run_quick_transform(ToolId::ChmodCalculator, "755").unwrap();
        assert!(chmod.contains("rwxr-xr-x"));
    }

    #[cfg(feature = "security")]
    #[test]
    fn quick_transform_covers_security_tools() {
        let digest = run_quick_transform(ToolId::HashHmac, "abc").unwrap();
        assert_eq!(
            digest,
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let cidr = run_quick_transform(ToolId::Ipv4CidrCalculator, "192.168.1.0/24").unwrap();
        assert!(cidr.contains("192.168.1.255"));
        let token =
            security::sign_jwt(r#"{"sub":"42"}"#, b"secret", jsonwebtoken::Algorithm::HS256)
                .unwrap();
        let decoded = run_quick_transform(ToolId::JwtTool, &token).unwrap();
        assert!(decoded.contains("HS256"));
        assert!(decoded.contains("\"sub\": \"42\""));
        assert!(run_quick_transform(ToolId::X509Parser, "not a certificate").is_err());
    }
}
