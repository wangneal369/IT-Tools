//! Native text, developer and offline security tool controllers.

use devtools_core::{CancelFlag, RawPayload, ToolError, ToolErrorKind, ToolId};
use devtools_tools::{debug, security, text};
use gpui_kit::*;

use super::Model;
use super::converters_gen::{read_bytes, read_file};
use super::runtime::{Inputs, Slot, ToolResult, ToolView};
use crate::theme::Palette;

#[derive(Clone)]
pub(super) enum DebugOptions {
    Diff {
        mode: usize,
        whitespace: bool,
        case: bool,
    },
    JsonDiff {
        unordered: bool,
        canonical: bool,
    },
    Analysis {
        tab: usize,
        dedupe: bool,
        sort: bool,
        reverse: bool,
        trim: bool,
        crlf: bool,
    },
    List {
        tab: usize,
        trim: bool,
        dedupe: bool,
        case: bool,
    },
    Regex {
        flags: [bool; 6],
        strict: bool,
    },
    JsonPath {
        values: bool,
    },
    Cron {
        zone: usize,
        count: usize,
    },
    Chmod,
    Jwt {
        mode: usize,
        algorithm: usize,
    },
    Hash {
        algorithm: usize,
        file: bool,
        hmac: bool,
    },
    X509 {
        file: bool,
    },
    Cidr,
}

const ZONES: [&str; 4] = ["UTC", "Asia/Shanghai", "America/New_York", "Europe/London"];

impl DebugOptions {
    pub fn new(tool: ToolId) -> Self {
        match tool {
            ToolId::TextDiff => Self::Diff {
                mode: 0,
                whitespace: false,
                case: false,
            },
            ToolId::JsonDiff => Self::JsonDiff {
                unordered: false,
                canonical: false,
            },
            ToolId::TextAnalysis => Self::Analysis {
                tab: 0,
                dedupe: false,
                sort: false,
                reverse: false,
                trim: false,
                crlf: false,
            },
            ToolId::ListCompare => Self::List {
                tab: 0,
                trim: true,
                dedupe: true,
                case: false,
            },
            ToolId::RegexTester => Self::Regex {
                flags: [false, false, false, false, false, true],
                strict: false,
            },
            ToolId::JsonPathTester => Self::JsonPath { values: true },
            ToolId::CronParser => Self::Cron { zone: 0, count: 1 },
            ToolId::ChmodCalculator => Self::Chmod,
            ToolId::JwtTool => Self::Jwt {
                mode: 0,
                algorithm: 0,
            },
            ToolId::HashHmac => Self::Hash {
                algorithm: 2,
                file: false,
                hmac: false,
            },
            ToolId::X509Parser => Self::X509 { file: false },
            ToolId::Ipv4CidrCalculator => Self::Cidr,
            _ => unreachable!("all other tools have standard or conversion views"),
        }
    }

    pub fn inject(&mut self, _: &str) {}

    pub fn execute(
        &self,
        inputs: &Inputs,
        cancel: &CancelFlag,
        manual: bool,
    ) -> Result<ToolResult, ToolError> {
        let input = inputs.text.as_str();
        Ok(match self {
            Self::Diff {
                mode,
                whitespace,
                case,
            } => {
                let options = text::TextDiffOptions {
                    ignore_whitespace: *whitespace,
                    ignore_case: *case,
                    ..text::TextDiffOptions::default()
                };
                let result = text::diff_text(input, &inputs.second, &options, cancel)?;
                let mut out =
                    ToolResult::text(text::export_unified_diff(input, &inputs.second, &options)?);
                out.footer = format!(
                    "+{} / −{} · 词级高亮 {}",
                    result.additions,
                    result.deletions,
                    if result.word_diff_enabled {
                        "开启"
                    } else {
                        "大文本已关闭"
                    }
                );
                if *mode == 1 {
                    out.colored = result
                        .inline
                        .into_iter()
                        .filter_map(|row| {
                            let tag = format!("{:?}", row.tag);
                            (tag != "Equal").then(|| {
                                (
                                    format!(
                                        "{} {}",
                                        if tag == "Insert" { "+" } else { "−" },
                                        row.text
                                    ),
                                    tag == "Insert",
                                )
                            })
                        })
                        .collect();
                }
                if *mode == 2 {
                    out.table = Some((
                        vec!["A".into(), "B".into()],
                        result
                            .side_by_side
                            .into_iter()
                            .map(|row| {
                                vec![
                                    row.left.map(|l| l.text).unwrap_or_default(),
                                    row.right.map(|l| l.text).unwrap_or_default(),
                                ]
                            })
                            .collect(),
                    ));
                }
                out
            }
            Self::JsonDiff {
                unordered,
                canonical,
            } => {
                let result = text::diff_json(
                    input,
                    &inputs.second,
                    &text::JsonDiffOptions {
                        unordered_arrays: *unordered,
                    },
                )?;
                let mut out = ToolResult::text(result.report);
                out.footer = format!("{} 处 JSON Pointer 差异", result.entries.len());
                if *canonical {
                    out.table = Some((
                        vec!["A · 规范化键排序".into(), "B · 规范化键排序".into()],
                        vec![vec![result.left_canonical, result.right_canonical]],
                    ));
                } else {
                    out.rows = result
                        .entries
                        .into_iter()
                        .map(|entry| (entry.pointer().to_owned(), format!("{entry:?}")))
                        .collect();
                }
                out
            }
            Self::Analysis {
                tab,
                dedupe,
                sort,
                reverse,
                trim,
                crlf,
            } => match tab {
                0 => ToolResult::rows(
                    text::all_naming_cases(input)
                        .into_iter()
                        .map(|(case, value)| (case.label().into(), value))
                        .collect(),
                ),
                1 => ToolResult::text(text::transform_lines(
                    input,
                    &text::LineOptions {
                        dedupe: *dedupe,
                        sort: *sort,
                        reverse: *reverse,
                        trim: *trim,
                        lf: !*crlf,
                    },
                )),
                _ => {
                    let stats = text::analyze_text(input);
                    let mut rows = vec![
                        ("Unicode 字素".into(), stats.graphemes.to_string()),
                        ("字节".into(), stats.bytes.to_string()),
                        ("单词".into(), stats.words.to_string()),
                        ("行".into(), stats.lines.to_string()),
                        ("段落".into(), stats.paragraphs.to_string()),
                    ];
                    rows.extend(
                        stats
                            .top_words
                            .into_iter()
                            .map(|(word, count)| (format!("词频 · {word}"), count.to_string())),
                    );
                    ToolResult::rows(rows)
                }
            },
            Self::List {
                tab,
                trim,
                dedupe,
                case,
            } => {
                let out = text::compare_lists(
                    input,
                    &inputs.second,
                    &text::ListCompareOptions {
                        delimiter: if inputs.query.is_empty() {
                            "\n".into()
                        } else {
                            inputs.query.clone()
                        },
                        trim: *trim,
                        dedupe: *dedupe,
                        ignore_case: *case,
                    },
                );
                let counts = format!(
                    "交集 {} · 并集 {} · 仅 A {} · 仅 B {}",
                    out.intersection.len(),
                    out.union.len(),
                    out.only_a.len(),
                    out.only_b.len()
                );
                let values = match tab {
                    0 => out.intersection,
                    1 => out.union,
                    2 => out.only_a,
                    _ => out.only_b,
                };
                ToolResult {
                    output: values.join("\n"),
                    footer: counts,
                    ..ToolResult::default()
                }
            }
            Self::Regex { flags, strict } => {
                let pattern = inputs.query.clone();
                let out = debug::test_regex(
                    &pattern,
                    input,
                    &debug::RegexFlags {
                        i: flags[0],
                        m: flags[1],
                        s: flags[2],
                        x: flags[3],
                        U: flags[4],
                        g: flags[5],
                    },
                    Some(&inputs.replacement),
                    *strict,
                    manual,
                    cancel,
                )?;
                let rows = out
                    .matches
                    .iter()
                    .enumerate()
                    .flat_map(|(index, m)| {
                        let mut rows = vec![(
                            format!("匹配 {} · {}..{}", index + 1, m.start, m.end),
                            m.text.clone(),
                        )];
                        rows.extend(m.groups.iter().map(|(group, value)| {
                            (
                                format!("捕获组 {group}"),
                                value.clone().unwrap_or_else(|| "未匹配".into()),
                            )
                        }));
                        rows
                    })
                    .collect();
                let output = out.replacement_preview.unwrap_or_default();
                let mut result = ToolResult {
                    output: output.clone(),
                    highlights: out
                        .matches
                        .iter()
                        .enumerate()
                        .filter(|(_, m)| m.start < m.end)
                        .map(|(index, m)| (m.start..m.end, index % 2 == 0))
                        .collect(),
                    highlighted_text: input.to_owned(),
                    rows,
                    footer: format!(
                        "{:?} · {} 匹配{} · 预算 {} 步",
                        out.engine,
                        out.matches.len(),
                        if out.truncated {
                            "（已截断 5k）"
                        } else {
                            ""
                        },
                        if manual { 1_000_000 } else { 200_000 }
                    ),
                    ..ToolResult::default()
                };
                result.rows.push(("替换预览".into(), output));
                result
            }
            Self::JsonPath { values } => {
                let out = debug::query_json_path(
                    input,
                    if inputs.query.is_empty() {
                        "$"
                    } else {
                        &inputs.query
                    },
                )?;
                let mut result = ToolResult::rows(
                    out.matches
                        .into_iter()
                        .map(|m| {
                            if *values {
                                (m.normalized, m.value.to_string())
                            } else {
                                (m.normalized, m.pointer)
                            }
                        })
                        .collect(),
                );
                result.footer = if out.empty_result {
                    "查询合法 · 0 命中".into()
                } else {
                    format!("{} 命中", result.rows.len())
                };
                result
            }
            Self::Cron { zone, count } => {
                let out = debug::parse_cron(
                    input.trim(),
                    ZONES[*zone],
                    [
                        debug::TriggerCount::Five,
                        debug::TriggerCount::Ten,
                        debug::TriggerCount::Twenty,
                        debug::TriggerCount::Fifty,
                    ][*count],
                )?;
                let mut rows = vec![
                    ("中文释义".into(), out.description_zh),
                    ("English".into(), out.description_en),
                ];
                rows.extend(
                    out.next_triggers
                        .into_iter()
                        .enumerate()
                        .map(|(i, t)| (format!("{} · 下一次 {}", i + 1, out.time_zone), t)),
                );
                ToolResult::rows(rows)
            }
            Self::Chmod => {
                let p = parse_permissions(input)?;
                ToolResult::rows(vec![
                    ("八进制".into(), p.to_octal()),
                    ("符号".into(), p.to_symbolic()),
                    (
                        "命令".into(),
                        p.to_command(if inputs.query.is_empty() {
                            "path"
                        } else {
                            &inputs.query
                        }),
                    ),
                ])
            }
            Self::Jwt { mode, algorithm } => {
                let algorithms = ["HS256", "HS384", "HS512"];
                let algorithm = algorithms[*algorithm]
                    .parse()
                    .map_err(|_| ToolError::new(ToolErrorKind::Internal, "JWT 算法无效"))?;
                if *mode == 2 {
                    ToolResult::text(security::sign_jwt(
                        input,
                        inputs.secret.as_bytes(),
                        algorithm,
                    )?)
                } else {
                    let out = if *mode == 1 {
                        security::verify_jwt(input.trim(), inputs.secret.as_bytes(), algorithm)?
                    } else {
                        security::decode_without_verification(input.trim())?
                    };
                    // Canonical formatter keeps the rendered header and claims readable.
                    let pretty = |value: String| {
                        devtools_tools::format::json::run(
                            &devtools_tools::format::json::JsonFormatterRequest {
                                input: value,
                                pretty: true,
                                indent: devtools_tools::format::json::JsonIndent::Two,
                                sort_keys: false,
                            },
                        )
                        .map(|r| r.output)
                    };
                    ToolResult::rows(vec![
                        ("算法".into(), out.algorithm),
                        ("时效".into(), out.status.label_zh().into()),
                        ("Header".into(), pretty(out.header.to_string())?),
                        ("Claims".into(), pretty(out.claims.to_string())?),
                        (
                            "验签".into(),
                            if *mode == 1 {
                                "签名有效"
                            } else {
                                "仅解析，未验证签名"
                            }
                            .into(),
                        ),
                    ])
                }
            }
            Self::Hash {
                algorithm,
                file,
                hmac,
            } => {
                let algorithm = [
                    security::HashAlgorithm::Md5,
                    security::HashAlgorithm::Sha1,
                    security::HashAlgorithm::Sha256,
                    security::HashAlgorithm::Sha512,
                    security::HashAlgorithm::Blake3,
                ][*algorithm];
                let key = hmac.then_some(inputs.secret.as_bytes());
                let out = if *file || matches!(inputs.payload, Some(RawPayload::Files(_))) {
                    let path = match &inputs.payload {
                        Some(RawPayload::Files(paths)) if !paths.is_empty() => paths[0].clone(),
                        _ => std::path::PathBuf::from(input.trim()),
                    };
                    security::hash_file_streaming(&path, algorithm, key, cancel, &mut |progress| {
                        *inputs.progress.lock().expect("progress lock") = Some(progress);
                        !cancel.is_cancelled()
                    })?
                } else {
                    security::hash_text(input.as_bytes(), algorithm, key)
                };
                let mut result = ToolResult::text(out.hex.clone());
                if !inputs.expected.trim().is_empty() {
                    result.rows = vec![
                        ("摘要".into(), out.hex.clone()),
                        (
                            "恒定时间校验".into(),
                            if security::compare_digest_compute_time(
                                &out.hex,
                                inputs.expected.trim(),
                            ) {
                                "✓ 匹配"
                            } else {
                                "× 不匹配"
                            }
                            .into(),
                        ),
                    ];
                }
                result.footer =
                    format!("{:?}{}", out.algorithm, if *hmac { " · HMAC" } else { "" });
                result
            }
            Self::X509 { file } => {
                let bytes = if *file && !matches!(inputs.payload, Some(RawPayload::Files(_))) {
                    read_file(std::path::Path::new(input.trim()), 20 * 1024 * 1024)?
                } else {
                    read_bytes(inputs, 20 * 1024 * 1024)?
                };
                let out = security::parse_x509_input(
                    &bytes,
                    (!inputs.secret.is_empty()).then_some(inputs.secret.as_str()),
                )?;
                let mut rows = Vec::new();
                for (i, cert) in out.certificates.into_iter().enumerate() {
                    rows.extend([
                        (format!("证书 {} · Subject", i + 1), cert.subject),
                        ("Issuer".into(), cert.issuer),
                        ("SAN".into(), cert.san.join("\n")),
                        (
                            "有效期".into(),
                            format!(
                                "{} · {} → {}",
                                cert.validity.label_zh(),
                                cert.not_before,
                                cert.not_after
                            ),
                        ),
                        ("SHA-256".into(), cert.fingerprint_sha256),
                        ("SHA-1".into(), cert.fingerprint_sha1),
                    ]);
                }
                ToolResult::rows(rows)
            }
            Self::Cidr => {
                let out = security::parse_and_analyze(input)?;
                ToolResult::rows(vec![
                    ("子网".into(), out.networks.join("\n")),
                    ("网络地址".into(), out.network.to_string()),
                    ("广播地址".into(), out.broadcast.to_string()),
                    (
                        "掩码".into(),
                        format!("{} /{}", out.netmask, out.prefix_len),
                    ),
                    (
                        "主机范围".into(),
                        format!("{} → {}", out.first_host, out.last_host),
                    ),
                    (
                        "可用 / 总地址".into(),
                        format!("{} / {}", out.usable_hosts, out.total_addresses),
                    ),
                    ("二进制".into(), security::to_binary(out.network)),
                    ("十六进制".into(), security::to_hex(out.network)),
                    (
                        "IPv4-mapped IPv6".into(),
                        out.network.to_ipv6_mapped().to_string(),
                    ),
                    (
                        "地址属性".into(),
                        format!(
                            "私有 {} · 回环 {} · 链路本地 {} · 组播 {} · 保留 {}",
                            out.is_private,
                            out.is_loopback,
                            out.is_link_local,
                            out.is_multicast,
                            out.is_reserved
                        ),
                    ),
                ])
            }
        })
    }

    pub fn inputs(&self, view: &ToolView) -> AnyElement {
        let mut inputs = div().flex().flex_col().gap_3();
        match self {
            Self::Diff { .. } | Self::JsonDiff { .. } | Self::List { .. } => {
                if matches!(self, Self::List { .. }) {
                    inputs = inputs.child(view.field(Slot::Query, "分隔符（留空为换行）"));
                }
                inputs = inputs
                    .child(view.field(Slot::Text, "输入 A"))
                    .child(view.field(Slot::Second, "输入 B"));
            }
            Self::Regex { .. } => {
                inputs = inputs
                    .child(view.field(Slot::Query, "正则模式"))
                    .child(view.field(Slot::Text, "测试文本"))
                    .child(view.field(Slot::Replacement, "替换文本（$1 / ${name}）"))
            }
            Self::JsonPath { .. } => {
                inputs = inputs
                    .child(view.field(Slot::Query, "JSONPath · RFC 9535（默认 $）"))
                    .child(view.field(Slot::Text, "JSON 输入"))
            }
            Self::Chmod => {
                inputs = inputs
                    .child(view.field(Slot::Text, "八进制 / 符号权限（空值默认 755）"))
                    .child(view.field(Slot::Query, "目标路径"))
            }
            Self::Jwt { mode, .. } => {
                inputs = inputs
                    .child(view.field(Slot::Text, if *mode == 2 { "Claims JSON" } else { "JWT" }));
                if *mode != 0 {
                    inputs = inputs.child(view.field(Slot::Secret, "HMAC 密钥（仅内存）"));
                }
            }
            Self::Hash { file, hmac, .. } => {
                inputs = inputs
                    .child(view.field(
                        Slot::Text,
                        if *file {
                            "文件路径（或智能粘贴文件）"
                        } else {
                            "文本"
                        },
                    ))
                    .child(view.field(Slot::Expected, "预期摘要（恒定时间校验）"));
                if *hmac {
                    inputs = inputs.child(view.field(Slot::Secret, "HMAC 密钥"));
                }
            }
            Self::X509 { file } => {
                inputs = inputs
                    .child(view.field(
                        Slot::Text,
                        if *file {
                            "DER / PFX / PEM 文件路径"
                        } else {
                            "PEM 证书（严格拒绝私钥）"
                        },
                    ))
                    .child(view.field(Slot::Secret, "PFX 可选口令（仅内存）"))
            }
            _ => inputs = inputs.child(view.field(Slot::Text, "输入")),
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
            ($variant:ident,$field:ident,$value:expr,$labels:expr) => {
                for (index, label) in $labels.into_iter().enumerate() {
                    bar = bar.child(view.chip(
                        &format!("{}-{index}", stringify!($field)),
                        label,
                        *$value == index,
                        palette,
                        cx,
                        move |view| {
                            if let Model::Debug(Self::$variant { $field, .. }) = &mut view.model {
                                *$field = index;
                            }
                        },
                    ));
                }
            };
        }
        macro_rules! toggle {
            ($variant:ident,$field:ident,$value:expr,$label:expr) => {
                bar = bar.child(view.chip(
                    stringify!($field),
                    $label,
                    *$value,
                    palette,
                    cx,
                    |view| {
                        if let Model::Debug(Self::$variant { $field, .. }) = &mut view.model {
                            *$field = !*$field;
                        }
                    },
                ));
            };
        }
        match self {
            Self::Diff {
                mode,
                whitespace,
                case,
            } => {
                choices!(Diff, mode, mode, ["Unified", "Inline", "Side-by-side"]);
                toggle!(Diff, whitespace, whitespace, "忽略空白");
                toggle!(Diff, case, case, "忽略大小写");
            }
            Self::JsonDiff {
                unordered,
                canonical,
            } => {
                toggle!(JsonDiff, unordered, unordered, "无序数组");
                toggle!(
                    JsonDiff,
                    canonical,
                    canonical,
                    "规范化预览（关闭为 Pointer 列表）"
                );
            }
            Self::Analysis {
                tab,
                dedupe,
                sort,
                reverse,
                trim,
                crlf,
            } => {
                choices!(Analysis, tab, tab, ["12 种命名", "行操作", "Unicode 统计"]);
                if *tab == 1 {
                    toggle!(Analysis, dedupe, dedupe, "去重");
                    toggle!(Analysis, sort, sort, "排序");
                    toggle!(Analysis, reverse, reverse, "反转");
                    toggle!(Analysis, trim, trim, "修剪");
                    toggle!(Analysis, crlf, crlf, "CRLF（关闭为 LF）");
                }
            }
            Self::List {
                tab,
                trim,
                dedupe,
                case,
            } => {
                choices!(List, tab, tab, ["交集", "并集", "仅 A", "仅 B"]);
                toggle!(List, trim, trim, "修剪");
                toggle!(List, dedupe, dedupe, "去重");
                toggle!(List, case, case, "忽略大小写");
            }
            Self::Regex { flags, strict } => {
                for (index, label) in ["i", "m", "s", "x", "U", "g"].into_iter().enumerate() {
                    bar = bar.child(view.chip(
                        &format!("flag-{index}"),
                        label,
                        flags[index],
                        palette,
                        cx,
                        move |view| {
                            if let Model::Debug(Self::Regex { flags, .. }) = &mut view.model {
                                flags[index] = !flags[index];
                            }
                        },
                    ));
                }
                toggle!(Regex, strict, strict, "严格线性");
            }
            Self::JsonPath { values } => {
                toggle!(JsonPath, values, values, "Values（关闭为 Paths）");
            }
            Self::Cron { zone, count } => {
                choices!(Cron, zone, zone, ZONES);
                choices!(
                    Cron,
                    count,
                    count,
                    ["未来 5 次", "未来 10 次", "未来 20 次", "未来 50 次"]
                );
                for (index, (label, expr)) in [
                    ("每分钟", "* * * * *"),
                    ("每 5 分钟", "*/5 * * * *"),
                    ("每天 09:00", "0 9 * * *"),
                    ("工作日", "0 9 * * 1-5"),
                ]
                .into_iter()
                .enumerate()
                {
                    bar = bar.child(view.chip(
                        &format!("preset-{index}"),
                        label,
                        false,
                        palette,
                        cx,
                        move |view| view.inputs.text = expr.into(),
                    ));
                }
            }
            Self::Chmod => {
                let permissions = parse_permissions(&view.inputs.text).unwrap_or_else(|_| {
                    debug::Permissions::from_octal(0o755).expect("valid permissions")
                });
                for (row, (label, triplet)) in [
                    ("Owner", permissions.owner),
                    ("Group", permissions.group),
                    ("Other", permissions.others),
                ]
                .into_iter()
                .enumerate()
                {
                    let mut line = div()
                        .flex()
                        .gap_2()
                        .child(div().w(px(55.)).text_xs().child(label));
                    for (col, (label, active)) in [
                        ("r", triplet.read),
                        ("w", triplet.write),
                        ("x", triplet.execute),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        let bit = 1u16 << (8 - (row * 3 + col));
                        line = line.child(view.chip(
                            &format!("permission-{row}-{col}"),
                            label,
                            active,
                            palette,
                            cx,
                            move |view| {
                                let permissions = parse_permissions(&view.inputs.text)
                                    .unwrap_or_else(|_| {
                                        debug::Permissions::from_octal(0o755)
                                            .expect("valid permissions")
                                    });
                                let value = u16::from_str_radix(&permissions.to_octal(), 8)
                                    .unwrap_or(0o755)
                                    ^ bit;
                                view.inputs.text = debug::Permissions::from_octal(value)
                                    .expect("permission bits")
                                    .to_octal();
                            },
                        ));
                    }
                    bar = bar.child(line);
                }
                for (label, bit, active) in [
                    ("SUID", 0o4000, permissions.suid),
                    ("SGID", 0o2000, permissions.sgid),
                    ("Sticky", 0o1000, permissions.sticky),
                ] {
                    bar = bar.child(view.chip(label, label, active, palette, cx, move |view| {
                        let permissions =
                            parse_permissions(&view.inputs.text).unwrap_or_else(|_| {
                                debug::Permissions::from_octal(0o755).expect("valid permissions")
                            });
                        let value =
                            u16::from_str_radix(&permissions.to_octal(), 8).unwrap_or(0o755) ^ bit;
                        view.inputs.text = debug::Permissions::from_octal(value)
                            .expect("permission bits")
                            .to_octal();
                    }));
                }
            }
            Self::Jwt { mode, algorithm } => {
                choices!(Jwt, mode, mode, ["Decode", "Verify", "Sign"]);
                choices!(Jwt, algorithm, algorithm, ["HS256", "HS384", "HS512"]);
            }
            Self::Hash {
                algorithm,
                file,
                hmac,
            } => {
                choices!(
                    Hash,
                    algorithm,
                    algorithm,
                    ["MD5", "SHA1", "SHA256", "SHA512", "BLAKE3"]
                );
                toggle!(Hash, file, file, "文件输入");
                toggle!(Hash, hmac, hmac, "HMAC");
            }
            Self::X509 { file } => {
                toggle!(X509, file, file, "文件输入（DER / PFX / PEM）");
                bar = bar.child(
                    div()
                        .text_xs()
                        .text_color(rgb(palette.danger))
                        .child("严格拒绝私钥 · 离线证书链解析"),
                );
            }
            Self::Cidr => {
                bar = bar.child(
                    div()
                        .text_xs()
                        .child("CIDR / IP + 掩码 / 起止 IP · /31 和 /32 特例"),
                );
            }
        }
        bar.into_any_element()
    }
}

fn parse_permissions(input: &str) -> Result<debug::Permissions, ToolError> {
    let raw = input.trim();
    if raw.is_empty() {
        return debug::Permissions::from_octal(0o755);
    }
    if raw.bytes().all(|b| b.is_ascii_digit()) {
        debug::Permissions::from_octal(
            u16::from_str_radix(raw, 8)
                .map_err(|_| ToolError::new(ToolErrorKind::InvalidSyntax, "无效八进制权限"))?,
        )
    } else {
        debug::Permissions::from_symbolic(raw)
    }
}
