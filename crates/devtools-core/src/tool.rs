//! Tool identity and descriptor contracts (ADR-0004).
//!
//! The 28 V1 tools are a closed, compile-time set. Each belongs to exactly one
//! flat primary [`Category`] and renders on one of two [`UiTrack`]s.

use serde::{Deserialize, Serialize};

/// The seven flat primary tool categories shown in the sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    /// 转换（数据格式、时间戳、进制、颜色、JSON 转表格）
    Conversion,
    /// 编解码（Base64、URL、HTML 实体、转义、GZip）
    EncodeDecode,
    /// 格式化（JSON、SQL、XML/HTML）
    Format,
    /// 生成器（UUID/ULID、密码、二维码）
    Generator,
    /// 文本对比（文本、JSON、文本分析、列表集合）
    Text,
    /// 调试开发（正则、JSONPath、Cron、Chmod）
    Debug,
    /// 安全网络（JWT、Hash/HMAC、X.509、IPv4/CIDR）
    Security,
}

impl Category {
    /// Ordered set of all seven sidebar categories.
    pub const ALL: [Category; 7] = [
        Category::Conversion,
        Category::EncodeDecode,
        Category::Format,
        Category::Generator,
        Category::Text,
        Category::Debug,
        Category::Security,
    ];
}

/// The 28 closed V1 tools. Every tool maps to exactly one [`Category`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolId {
    // 1. 转换
    DataFormatConverter,
    DateTimeConverter,
    NumberBaseConverter,
    ColorConverter,
    JsonToTable,
    // 2. 编解码
    Base64Codec,
    UrlCodec,
    HtmlEntityUnicode,
    StringEscape,
    GzipCompress,
    // 3. 格式化
    JsonFormatter,
    SqlFormatter,
    XmlHtmlFormatter,
    // 4. 生成器
    UuidUlidGenerator,
    PasswordTokenGenerator,
    QrCodeGenerator,
    // 5. 文本对比
    TextDiff,
    JsonDiff,
    TextAnalysis,
    ListCompare,
    // 6. 调试开发
    RegexTester,
    JsonPathTester,
    CronParser,
    ChmodCalculator,
    // 7. 安全网络
    JwtTool,
    HashHmac,
    X509Parser,
    Ipv4CidrCalculator,
}

impl ToolId {
    /// The full ordered set of V1 tools (28).
    pub const ALL: [ToolId; 28] = [
        ToolId::DataFormatConverter,
        ToolId::DateTimeConverter,
        ToolId::NumberBaseConverter,
        ToolId::ColorConverter,
        ToolId::JsonToTable,
        ToolId::Base64Codec,
        ToolId::UrlCodec,
        ToolId::HtmlEntityUnicode,
        ToolId::StringEscape,
        ToolId::GzipCompress,
        ToolId::JsonFormatter,
        ToolId::SqlFormatter,
        ToolId::XmlHtmlFormatter,
        ToolId::UuidUlidGenerator,
        ToolId::PasswordTokenGenerator,
        ToolId::QrCodeGenerator,
        ToolId::TextDiff,
        ToolId::JsonDiff,
        ToolId::TextAnalysis,
        ToolId::ListCompare,
        ToolId::RegexTester,
        ToolId::JsonPathTester,
        ToolId::CronParser,
        ToolId::ChmodCalculator,
        ToolId::JwtTool,
        ToolId::HashHmac,
        ToolId::X509Parser,
        ToolId::Ipv4CidrCalculator,
    ];

    /// Stable kebab-case identifier matching serde serialization.
    pub const fn as_str(self) -> &'static str {
        match self {
            ToolId::DataFormatConverter => "data-format-converter",
            ToolId::DateTimeConverter => "date-time-converter",
            ToolId::NumberBaseConverter => "number-base-converter",
            ToolId::ColorConverter => "color-converter",
            ToolId::JsonToTable => "json-to-table",
            ToolId::Base64Codec => "base64-codec",
            ToolId::UrlCodec => "url-codec",
            ToolId::HtmlEntityUnicode => "html-entity-unicode",
            ToolId::StringEscape => "string-escape",
            ToolId::GzipCompress => "gzip-compress",
            ToolId::JsonFormatter => "json-formatter",
            ToolId::SqlFormatter => "sql-formatter",
            ToolId::XmlHtmlFormatter => "xml-html-formatter",
            ToolId::UuidUlidGenerator => "uuid-ulid-generator",
            ToolId::PasswordTokenGenerator => "password-token-generator",
            ToolId::QrCodeGenerator => "qr-code-generator",
            ToolId::TextDiff => "text-diff",
            ToolId::JsonDiff => "json-diff",
            ToolId::TextAnalysis => "text-analysis",
            ToolId::ListCompare => "list-compare",
            ToolId::RegexTester => "regex-tester",
            ToolId::JsonPathTester => "json-path-tester",
            ToolId::CronParser => "cron-parser",
            ToolId::ChmodCalculator => "chmod-calculator",
            ToolId::JwtTool => "jwt-tool",
            ToolId::HashHmac => "hash-hmac",
            ToolId::X509Parser => "x509-parser",
            ToolId::Ipv4CidrCalculator => "ipv4-cidr-calculator",
        }
    }

    /// The single primary category this tool belongs to.
    pub const fn category(self) -> Category {
        match self {
            ToolId::DataFormatConverter
            | ToolId::DateTimeConverter
            | ToolId::NumberBaseConverter
            | ToolId::ColorConverter
            | ToolId::JsonToTable => Category::Conversion,
            ToolId::Base64Codec
            | ToolId::UrlCodec
            | ToolId::HtmlEntityUnicode
            | ToolId::StringEscape
            | ToolId::GzipCompress => Category::EncodeDecode,
            ToolId::JsonFormatter | ToolId::SqlFormatter | ToolId::XmlHtmlFormatter => {
                Category::Format
            }
            ToolId::UuidUlidGenerator
            | ToolId::PasswordTokenGenerator
            | ToolId::QrCodeGenerator => Category::Generator,
            ToolId::TextDiff | ToolId::JsonDiff | ToolId::TextAnalysis | ToolId::ListCompare => {
                Category::Text
            }
            ToolId::RegexTester
            | ToolId::JsonPathTester
            | ToolId::CronParser
            | ToolId::ChmodCalculator => Category::Debug,
            ToolId::JwtTool
            | ToolId::HashHmac
            | ToolId::X509Parser
            | ToolId::Ipv4CidrCalculator => Category::Security,
        }
    }

    /// Which UI track renders this tool (ADR-0004 dual-track UI).
    pub const fn ui_track(self) -> UiTrack {
        match self {
            ToolId::DataFormatConverter
            | ToolId::HtmlEntityUnicode
            | ToolId::StringEscape
            | ToolId::GzipCompress
            | ToolId::JsonFormatter
            | ToolId::SqlFormatter
            | ToolId::XmlHtmlFormatter => UiTrack::StandardTransform,
            _ => UiTrack::CustomView,
        }
    }

    /// Baseline V1 capability declaration for each tool (all 28 offline).
    pub const fn default_capabilities(self) -> ToolCapabilities {
        let sensitive = matches!(self, ToolId::PasswordTokenGenerator | ToolId::JwtTool);
        ToolCapabilities::offline(sensitive)
    }
}

/// The two UI extension tracks a tool may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiTrack {
    /// Declarative text-in/text-out double-pane template (7 tools).
    StandardTransform,
    /// Native GPUI custom view composed from shared building blocks (21 tools).
    CustomView,
}

/// Static, compile-time capabilities of a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ToolCapabilities {
    /// Always `true` in V1: every tool runs 100% offline.
    pub offline: bool,
    /// Whether copying output marks clipboard content as excluded from OS history.
    pub sensitive_output: bool,
}

impl ToolCapabilities {
    pub const fn offline(sensitive_output: bool) -> Self {
        Self {
            offline: true,
            sensitive_output,
        }
    }
}

impl Default for ToolCapabilities {
    fn default() -> Self {
        Self::offline(false)
    }
}

/// A static descriptor of a single built-in tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolDescriptor {
    pub id: ToolId,
    /// 中文显示名（界面主语言）。
    pub name: &'static str,
    /// 检索别名（中英 / 缩写 / 拼音加权模糊检索）。
    pub aliases: &'static [&'static str],
    pub category: Category,
    pub ui_track: UiTrack,
    pub capabilities: ToolCapabilities,
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn tool_catalog_is_closed_unique_and_partitioned() {
        assert_eq!(ToolId::ALL.len(), 28);
        let unique_ids: HashSet<_> = ToolId::ALL.into_iter().collect();
        assert_eq!(unique_ids.len(), 28);

        let unique_names: HashSet<_> = ToolId::ALL.into_iter().map(ToolId::as_str).collect();
        assert_eq!(unique_names.len(), 28);

        let count = |c: Category| ToolId::ALL.iter().filter(|id| id.category() == c).count();
        assert_eq!(count(Category::Conversion), 5);
        assert_eq!(count(Category::EncodeDecode), 5);
        assert_eq!(count(Category::Format), 3);
        assert_eq!(count(Category::Generator), 3);
        assert_eq!(count(Category::Text), 4);
        assert_eq!(count(Category::Debug), 4);
        assert_eq!(count(Category::Security), 4);
    }

    #[test]
    fn seven_standard_templates_twenty_one_custom_views() {
        let standard = ToolId::ALL
            .iter()
            .filter(|id| id.ui_track() == UiTrack::StandardTransform)
            .count();
        let custom = ToolId::ALL
            .iter()
            .filter(|id| id.ui_track() == UiTrack::CustomView)
            .count();
        assert_eq!(standard, 7);
        assert_eq!(custom, 21);
    }

    #[test]
    fn all_v1_tools_declare_offline_and_sensitive_tools_are_marked() {
        assert!(ToolCapabilities::default().offline);
        assert!(!ToolCapabilities::default().sensitive_output);

        for tool in ToolId::ALL {
            assert!(
                tool.default_capabilities().offline,
                "{tool:?} must be offline in V1"
            );
        }
        assert!(
            ToolId::PasswordTokenGenerator
                .default_capabilities()
                .sensitive_output
        );
        assert!(ToolId::JwtTool.default_capabilities().sensitive_output);
    }
}
