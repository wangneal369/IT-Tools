//! Chinese UI labels and concise functional descriptions for the closed V1 catalog.

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

/// Compact glyph retained for plain-text fallbacks and tests.
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

/// Concrete one-line functional summary shown on workbench home cards,
/// command palette rows, and empty states.
pub const fn tool_hint(tool: ToolId) -> &'static str {
    match tool {
        ToolId::DataFormatConverter => "JSON / YAML / TOML / XML 四向互转与键排序",
        ToolId::DateTimeConverter => "ISO 8601、RFC 日期与秒/毫秒/微秒/纳秒时间戳互转",
        ToolId::NumberBaseConverter => "2 / 8 / 10 / 16 与任意 2–36 进制、补码同步换算",
        ToolId::ColorConverter => "HEX / RGB / HSL / OKLCH / CMYK 互转与 WCAG 对比度",
        ToolId::JsonToTable => "嵌套 JSON 拍平预览排序，一键导出 CSV / TSV / Markdown",
        ToolId::Base64Codec => "标准 / URL-Safe 文本与图片 Data URI 离线编解码",
        ToolId::UrlCodec => "URL 结构拆解、重复键 Query 表与百分号编解码",
        ToolId::HtmlEntityUnicode => "HTML 实体、\\uXXXX、CSS 转义与 Unicode 码点互转",
        ToolId::StringEscape => "JSON / C-Rust / Shell / 正则风格字符串转义与反转义",
        ToolId::GzipCompress => "GZip 1–9 级压缩解压，支持 Base64 / Hex 传输编码",
        ToolId::JsonFormatter => "保序美化、压缩与键字典序重排，精准定位语法行列",
        ToolId::SqlFormatter => "多方言 SQL 关键字大小写与展开/紧凑排版美化",
        ToolId::XmlHtmlFormatter => "XML / HTML 标签层级缩进美化、压缩与闭合校验",
        ToolId::UuidUlidGenerator => "批量生成 UUID v4 / v7 / v1 / ULID 并反解版本时间戳",
        ToolId::PasswordTokenGenerator => "高熵随机密码与密钥 Token 生成，内置 zxcvbn 强度评估",
        ToolId::QrCodeGenerator => "文本 / URL / WiFi 二维码 SVG/PNG 生成与本地图片解码",
        ToolId::TextDiff => "Unified / 行内 / 并排文本差异比对，支持忽略空白与大小写",
        ToolId::JsonDiff => "忽略键序与空白噪声，按 JSON Pointer 精确定位结构差异",
        ToolId::TextAnalysis => "12 种命名风格转换、行批处理与 Unicode 字素词频统计",
        ToolId::ListCompare => "A / B 双列表交集、并集、差集保序计算与去重修剪",
        ToolId::RegexTester => "多标志位实时高亮匹配、捕获组拆解与安全步数保护",
        ToolId::JsonPathTester => "RFC 9535 标准 JSONPath 查询，支持匹配值与规范路径视图",
        ToolId::CronParser => "5 / 6 段与 Quartz Cron 自然语言释义及多时区触发时间预测",
        ToolId::ChmodCalculator => "3×3 rwx 权限矩阵与 SUID / SGID / Sticky 八进制符号联动",
        ToolId::JwtTool => "JWT 零密钥拆解时效校验，支持离线 HMAC 验签与签发",
        ToolId::HashHmac => "MD5 / SHA-2 / BLAKE3 文本与流式大文件摘要及恒定时间比对",
        ToolId::X509Parser => "PEM / DER / PFX 证书链、SAN、有效期与指纹离线解析",
        ToolId::Ipv4CidrCalculator => "CIDR / 掩码 / 地址段子网划分、主机范围与二进制位分析",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn every_category_and_tool_has_unique_meaningful_labels() {
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
        let mut hints = HashSet::new();
        for tool in ToolId::ALL {
            assert!(!tool_name(tool).is_empty());
            let hint = tool_hint(tool);
            assert!(!hint.is_empty());
            assert!(
                !hint.ends_with("· 离线计算"),
                "{tool:?} still uses generic hint"
            );
            assert!(hints.insert(hint), "duplicate hint for {tool:?}: {hint}");
        }
    }
}
