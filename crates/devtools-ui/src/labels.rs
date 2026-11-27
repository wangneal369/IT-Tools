//! Chinese UI labels for the closed V1 catalog (spec §1: 界面中文为主).

use devtools_core::{Category, ToolId};

/// Sidebar label for a category.
pub const fn category_label(category: Category) -> &'static str {
    match category {
        Category::Conversion => "转换",
        Category::EncodeDecode => "编解码",
        Category::Format => "格式化",
        Category::Generator => "生成器",
        Category::Text => "文本对比",
        Category::Debug => "调试开发",
        Category::Security => "安全网络",
    }
}

/// Compact glyph used by the collapsed icon rail.
pub const fn category_glyph(category: Category) -> &'static str {
    match category {
        Category::Conversion => "转",
        Category::EncodeDecode => "码",
        Category::Format => "格",
        Category::Generator => "生",
        Category::Text => "文",
        Category::Debug => "调",
        Category::Security => "安",
    }
}

/// Display name for a tool (V1 spec §6).
pub const fn tool_name(tool: ToolId) -> &'static str {
    match tool {
        ToolId::DataFormatConverter => "数据格式互转",
        ToolId::DateTimeConverter => "日期与时间戳转换",
        ToolId::NumberBaseConverter => "进制转换",
        ToolId::ColorConverter => "颜色转换",
        ToolId::JsonToTable => "JSON 转表格 / CSV",
        ToolId::Base64Codec => "Base64 编解码",
        ToolId::UrlCodec => "URL 编解码与解析",
        ToolId::HtmlEntityUnicode => "HTML 实体与 Unicode",
        ToolId::StringEscape => "字符串转义 / 反转义",
        ToolId::GzipCompress => "GZip 压缩与解压",
        ToolId::JsonFormatter => "JSON 格式化与压缩",
        ToolId::SqlFormatter => "SQL 格式化",
        ToolId::XmlHtmlFormatter => "XML / HTML 格式化",
        ToolId::UuidUlidGenerator => "UUID / ULID 生成器",
        ToolId::PasswordTokenGenerator => "密码与随机 Token",
        ToolId::QrCodeGenerator => "二维码生成与解码",
        ToolId::TextDiff => "文本对比",
        ToolId::JsonDiff => "JSON 语义对比",
        ToolId::TextAnalysis => "文本分析与实用工具",
        ToolId::ListCompare => "列表集合对比",
        ToolId::RegexTester => "正则表达式测试",
        ToolId::JsonPathTester => "JSONPath 查询测试",
        ToolId::CronParser => "Cron 表达式解析",
        ToolId::ChmodCalculator => "Chmod 权限计算器",
        ToolId::JwtTool => "JWT 解析与签发",
        ToolId::HashHmac => "Hash / HMAC 与文件校验",
        ToolId::X509Parser => "X.509 证书解析",
        ToolId::Ipv4CidrCalculator => "IPv4 / CIDR 子网计算",
    }
}

/// All tools belonging to a category, in spec order.
pub fn tools_in(category: Category) -> impl Iterator<Item = ToolId> {
    ToolId::ALL
        .into_iter()
        .filter(move |tool| tool.category() == category)
}

/// One-line description shown on the workbench home cards.
pub const fn tool_hint(tool: ToolId) -> &'static str {
    match tool.category() {
        Category::Conversion => "转换 · 离线计算",
        Category::EncodeDecode => "编解码 · 离线计算",
        Category::Format => "格式化 · 离线计算",
        Category::Generator => "生成器 · 离线计算",
        Category::Text => "文本处理 · 离线计算",
        Category::Debug => "开发调试 · 离线计算",
        Category::Security => "安全网络 · 离线计算",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_category_and_tool_has_a_label() {
        for category in Category::ALL {
            assert!(!category_label(category).is_empty());
            assert!(!category_glyph(category).is_empty());
            assert_eq!(
                tools_in(category).count(),
                ToolId::ALL
                    .iter()
                    .filter(|t| t.category() == category)
                    .count()
            );
        }
        for tool in ToolId::ALL {
            assert!(!tool_name(tool).is_empty());
        }
    }
}
