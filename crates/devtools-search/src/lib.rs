//! 快速命令面板 / 侧边栏的纯 Rust 中英、缩写、拼音模糊检索。

use devtools_core::{Confidence, ToolDescriptor, ToolId};
use nucleo_matcher::{Config, Matcher, Utf32Str};

pub use devtools_core;

macro_rules! tool {
    ($id:ident, $name:literal, [$($alias:literal),*]) => {
        ToolDescriptor { id: ToolId::$id, name: $name, aliases: &[$($alias),*],
            category: ToolId::$id.category(), ui_track: ToolId::$id.ui_track(),
            capabilities: ToolId::$id.default_capabilities() }
    };
}

/// 与核心闭集同序的集中静态目录。内核不依赖 UI 或 devtools-tools。
pub const CATALOG: [ToolDescriptor; 28] = [
    tool!(
        DataFormatConverter,
        "数据格式互转",
        [
            "Data Format Converter",
            "JSON YAML TOML XML",
            "格式转换",
            "shujugeshi",
            "sjgs",
            "geshizhuanhuan"
        ]
    ),
    tool!(
        DateTimeConverter,
        "日期与时间戳转换",
        [
            "Date Time Timestamp Converter",
            "timestamp",
            "ts",
            "时间戳",
            "shijianchuo",
            "sjc",
            "riqi"
        ]
    ),
    tool!(
        NumberBaseConverter,
        "进制转换",
        [
            "Number Base Converter",
            "hex",
            "binary",
            "十六进制",
            "jinzhi",
            "jz",
            "jinzhizhuanhuan"
        ]
    ),
    tool!(
        ColorConverter,
        "颜色转换",
        ["Color Converter", "rgb", "hsl", "oklch", "yanse", "ys"]
    ),
    tool!(
        JsonToTable,
        "JSON 转表格 / CSV",
        [
            "JSON to Table CSV",
            "j2c",
            "csv",
            "表格",
            "zhuanbiaoge",
            "zbg"
        ]
    ),
    tool!(
        Base64Codec,
        "Base64 编解码",
        [
            "Base64 Encoder Decoder",
            "b64",
            "base64",
            "编码",
            "bianma",
            "bm",
            "bianjiema"
        ]
    ),
    tool!(
        UrlCodec,
        "URL 编解码与解析",
        [
            "URL Encoder Decoder Parser",
            "url",
            "percent",
            "网址",
            "wangzhi",
            "wz"
        ]
    ),
    tool!(
        HtmlEntityUnicode,
        "HTML 实体与 Unicode",
        [
            "HTML Entity Unicode",
            "unicode",
            "entity",
            "实体",
            "shiti",
            "st"
        ]
    ),
    tool!(
        StringEscape,
        "字符串转义 / 反转义",
        ["String Escape Unescape", "escape", "转义", "zhuanyi", "zy"]
    ),
    tool!(
        GzipCompress,
        "GZip 压缩与解压",
        ["GZip Compress Decompress", "gzip", "压缩", "yasuo", "ysjd"]
    ),
    tool!(
        JsonFormatter,
        "JSON 格式化与压缩",
        [
            "JSON Formatter Minifier",
            "json",
            "格式化",
            "geshihua",
            "gsh",
            "jsonformat"
        ]
    ),
    tool!(
        SqlFormatter,
        "SQL 格式化",
        ["SQL Formatter", "sql", "sqlgeshihua", "sqlgsh"]
    ),
    tool!(
        XmlHtmlFormatter,
        "XML / HTML 格式化",
        ["XML HTML Formatter", "xml", "html", "xmlgeshihua", "xmlgsh"]
    ),
    tool!(
        UuidUlidGenerator,
        "UUID / ULID 生成器",
        [
            "UUID ULID Generator",
            "uuid",
            "ulid",
            "唯一标识",
            "weiyibiaoshi",
            "wybs"
        ]
    ),
    tool!(
        PasswordTokenGenerator,
        "密码与随机 Token",
        [
            "Password Random Token Generator",
            "password",
            "token",
            "密码",
            "mima",
            "mm"
        ]
    ),
    tool!(
        QrCodeGenerator,
        "二维码生成与解码",
        [
            "QR Code Generator Decoder",
            "qr",
            "qrcode",
            "二维码",
            "erweima",
            "ewm"
        ]
    ),
    tool!(
        TextDiff,
        "文本对比",
        [
            "Text Diff Compare",
            "diff",
            "对比",
            "duibi",
            "db",
            "wenbenduibi"
        ]
    ),
    tool!(
        JsonDiff,
        "JSON 语义对比",
        ["JSON Semantic Diff", "jsondiff", "jsonduibi", "jsondb"]
    ),
    tool!(
        TextAnalysis,
        "文本分析与实用工具",
        [
            "Text Analysis Utilities",
            "case",
            "字数",
            "wenbenfenxi",
            "wbfx"
        ]
    ),
    tool!(
        ListCompare,
        "列表集合对比",
        [
            "List Set Compare",
            "intersection",
            "集合",
            "liebiaoduibi",
            "lbdb",
            "jihe"
        ]
    ),
    tool!(
        RegexTester,
        "正则表达式测试",
        [
            "Regular Expression Regex Tester",
            "regex",
            "re",
            "正则",
            "zhengze",
            "zz"
        ]
    ),
    tool!(
        JsonPathTester,
        "JSONPath 查询测试",
        ["JSONPath Query Tester", "jsonpath", "jp", "chaxun", "cx"]
    ),
    tool!(
        CronParser,
        "Cron 表达式解析",
        ["Cron Expression Parser", "cron", "定时", "dingshi", "ds"]
    ),
    tool!(
        ChmodCalculator,
        "Chmod 权限计算器",
        [
            "Chmod Permission Calculator",
            "chmod",
            "权限",
            "quanxian",
            "qx"
        ]
    ),
    tool!(
        JwtTool,
        "JWT 解析与签发",
        ["JWT JSON Web Token", "jwt", "令牌", "lingpai", "lp"]
    ),
    tool!(
        HashHmac,
        "Hash / HMAC 与文件校验",
        [
            "Hash HMAC File Checksum",
            "hash",
            "hmac",
            "sha",
            "md5",
            "哈希",
            "haxi",
            "hx"
        ]
    ),
    tool!(
        X509Parser,
        "X.509 证书解析",
        [
            "X.509 Certificate Parser",
            "x509",
            "certificate",
            "证书",
            "zhengshu",
            "zs"
        ]
    ),
    tool!(
        Ipv4CidrCalculator,
        "IPv4 / CIDR 子网计算",
        [
            "IPv4 CIDR Subnet Calculator",
            "cidr",
            "ip",
            "子网",
            "ziwang",
            "zw"
        ]
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DetectionBoost {
    pub tool_id: ToolId,
    pub confidence: Confidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SearchGroup {
    Detection,
    Favorite,
    Catalog,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchResult {
    pub descriptor: &'static ToolDescriptor,
    pub score: u32,
    pub group: SearchGroup,
}

/// 匹配器复用工作缓冲区；持有实例的后台调用方自行串行访问或每任务建实例。
pub struct SearchIndex {
    matcher: Matcher,
}

impl Default for SearchIndex {
    fn default() -> Self {
        Self {
            matcher: Matcher::new(Config::DEFAULT),
        }
    }
}

impl SearchIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn catalog(&self) -> &'static [ToolDescriptor] {
        &CATALOG
    }

    pub fn search(&mut self, query: &str) -> Vec<SearchResult> {
        self.search_with(query, &[], &[])
    }

    /// 仅提升匹配查询的项目；检测组去重后最多五项，收藏保持用户顺序。
    pub fn search_with(
        &mut self,
        query: &str,
        detections: &[DetectionBoost],
        favorites: &[ToolId],
    ) -> Vec<SearchResult> {
        // 输入查询不是载荷，仍限制长度以约束模糊匹配的矩阵大小。
        let query: String = query
            .trim()
            .chars()
            .take(256)
            .collect::<String>()
            .to_lowercase();
        let mut detection_order = Vec::new();
        for d in detections
            .iter()
            .filter(|d| d.confidence >= Confidence::Medium)
        {
            if !detection_order.contains(&d.tool_id) {
                detection_order.push(d.tool_id);
            }
            if detection_order.len() == 5 {
                break;
            }
        }
        let mut found = Vec::new();
        for (catalog_order, descriptor) in CATALOG.iter().enumerate() {
            let score = if query.is_empty() {
                Some(0)
            } else {
                self.score(descriptor, &query)
            };
            let Some(score) = score else {
                continue;
            };
            let (group, position) =
                if let Some(pos) = detection_order.iter().position(|id| *id == descriptor.id) {
                    (SearchGroup::Detection, pos)
                } else if let Some(pos) = favorites.iter().position(|id| *id == descriptor.id) {
                    (SearchGroup::Favorite, pos)
                } else {
                    (SearchGroup::Catalog, catalog_order)
                };
            found.push((
                SearchResult {
                    descriptor,
                    score,
                    group,
                },
                position,
                catalog_order,
            ));
        }
        found.sort_by_key(|(r, position, order)| {
            (
                r.group,
                if r.group == SearchGroup::Catalog {
                    0
                } else {
                    *position
                },
                std::cmp::Reverse(r.score),
                *order,
            )
        });
        found.into_iter().map(|(r, _, _)| r).collect()
    }

    fn score(&mut self, descriptor: &ToolDescriptor, query: &str) -> Option<u32> {
        std::iter::once(descriptor.name)
            .chain(descriptor.aliases.iter().copied())
            .chain(std::iter::once(descriptor.id.as_str()))
            .filter_map(|field| {
                let field = field.to_lowercase();
                let mut haystack = Vec::new();
                let mut needle = Vec::new();
                self.matcher
                    .fuzzy_match(
                        Utf32Str::new(&field, &mut haystack),
                        Utf32Str::new(query, &mut needle),
                    )
                    .map(|score| {
                        u32::from(score)
                            + if field == query {
                                100_000
                            } else if field.starts_with(query) {
                                10_000
                            } else {
                                0
                            }
                    })
            })
            .max()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_tools_are_indexed_in_core_order() {
        let mut index = SearchIndex::new();
        assert_eq!(index.catalog().len(), 28);
        assert_eq!(
            index
                .search("")
                .iter()
                .map(|r| r.descriptor.id)
                .collect::<Vec<_>>(),
            ToolId::ALL
        );
        for descriptor in CATALOG {
            assert_eq!(
                index.search(descriptor.name)[0].descriptor.id,
                descriptor.id
            );
            assert_eq!(
                index.search(descriptor.aliases[0])[0].descriptor.id,
                descriptor.id
            );
        }
    }

    #[test]
    fn bilingual_aliases_and_pinyin_rank_first() {
        let mut index = SearchIndex::new();
        for (id, queries) in [
            (
                ToolId::Base64Codec,
                vec!["b64", "bianma", "bm", "Base64 编解码"],
            ),
            (
                ToolId::DateTimeConverter,
                vec!["ts", "shijianchuo", "sjc", "时间戳"],
            ),
            (
                ToolId::JsonFormatter,
                vec!["geshihua", "gsh", "json", "格式化"],
            ),
            (ToolId::QrCodeGenerator, vec!["erweima", "ewm", "二维码"]),
            (ToolId::PasswordTokenGenerator, vec!["mima", "mm", "密码"]),
            (ToolId::RegexTester, vec!["zhengze", "zz", "re", "正则"]),
            (ToolId::X509Parser, vec!["zhengshu", "zs", "x509", "证书"]),
            (ToolId::TextDiff, vec!["duibi", "db", "diff", "对比"]),
            (ToolId::JsonToTable, vec!["j2c", "csv"]),
            (ToolId::Ipv4CidrCalculator, vec!["cidr", "ip"]),
            (ToolId::CronParser, vec!["cron"]),
            (ToolId::UrlCodec, vec!["url"]),
            (ToolId::NumberBaseConverter, vec!["hex"]),
            (ToolId::ColorConverter, vec!["rgb"]),
        ] {
            for query in queries {
                assert_eq!(index.search(query)[0].descriptor.id, id, "{query}");
            }
        }
    }

    #[test]
    fn fuzzy_case_and_determinism() {
        let mut index = SearchIndex::new();
        assert_eq!(index.search("  B64 ")[0].descriptor.id, ToolId::Base64Codec);
        assert_eq!(
            index.search("jsnfrm")[0].descriptor.id,
            ToolId::JsonFormatter
        );
        assert_eq!(index.search(" "), index.search(""));
        assert_eq!(index.search("转换"), index.search("转换"));
        assert!(index.search("完全无匹配字符").is_empty());
    }

    #[test]
    fn recommendations_and_favorites_are_unique_and_bounded() {
        let mut index = SearchIndex::new();
        let mut boosts: Vec<_> = ToolId::ALL[..7]
            .iter()
            .map(|&tool_id| DetectionBoost {
                tool_id,
                confidence: Confidence::High,
            })
            .collect();
        boosts.insert(0, boosts[0]);
        boosts.insert(
            0,
            DetectionBoost {
                tool_id: ToolId::HashHmac,
                confidence: Confidence::Low,
            },
        );
        let results = index.search_with(
            "",
            &boosts,
            &[ToolId::RegexTester, ToolId::RegexTester, ToolId::HashHmac],
        );
        assert_eq!(results.len(), 28);
        assert_eq!(
            results
                .iter()
                .filter(|r| r.group == SearchGroup::Detection)
                .count(),
            5
        );
        assert_eq!(results[5].descriptor.id, ToolId::RegexTester);
        assert_eq!(results[6].descriptor.id, ToolId::HashHmac);
        let results = index.search_with("regex", &boosts, &[ToolId::RegexTester]);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].descriptor.id, ToolId::RegexTester);
    }
}
