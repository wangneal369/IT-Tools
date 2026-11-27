//! Native text, developer and offline security tool controllers.

use devtools_core::{CancelFlag, RawPayload, ToolError, ToolErrorKind, ToolId};
use devtools_tools::{debug, security, text};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::converters_gen::{read_bytes, read_file};
use super::runtime::{Inputs, Slot, ToolResult, ToolView, option_group, options_bar};
use super::{EditorLanguages, Model};
use crate::icons::{IconName, icon_el};
use crate::syntax::SyntaxLang;
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

    pub fn syntax_languages(&self) -> EditorLanguages {
        match self {
            Self::Diff { mode, .. } => EditorLanguages {
                text: None,
                second: None,
                output: (*mode == 0).then_some(SyntaxLang::Diff),
            },
            Self::JsonDiff { .. } => EditorLanguages {
                text: Some(SyntaxLang::Json),
                second: Some(SyntaxLang::Json),
                output: None,
            },
            Self::JsonPath { .. } => EditorLanguages {
                text: Some(SyntaxLang::Json),
                second: None,
                output: None,
            },
            Self::Jwt { mode, .. } => EditorLanguages {
                text: (*mode == 2).then_some(SyntaxLang::Json),
                second: None,
                output: None,
            },
            _ => EditorLanguages::default(),
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
                    out.diff_lines = result
                        .inline
                        .into_iter()
                        .map(|row| {
                            let tag = format!("{:?}", row.tag);
                            match tag.as_str() {
                                "Insert" => (format!("+ {}", row.text), Some(true)),
                                "Delete" => (format!("− {}", row.text), Some(false)),
                                _ => (format!("  {}", row.text), None),
                            }
                        })
                        .collect();
                }
                if *mode == 2 {
                    out.table = Some((
                        vec!["A（原始文本）".into(), "B（对比文本）".into()],
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
                        ("UTF-8 字节".into(), stats.bytes.to_string()),
                        ("单词数".into(), stats.words.to_string()),
                        ("行数".into(), stats.lines.to_string()),
                        ("段落数".into(), stats.paragraphs.to_string()),
                    ];
                    rows.extend(
                        stats
                            .top_words
                            .into_iter()
                            .map(|(word, count)| (format!("高频词 · {word}"), count.to_string())),
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
                            format!("匹配 #{} [{}..{}]", index + 1, m.start, m.end),
                            m.text.clone(),
                        )];
                        rows.extend(m.groups.iter().map(|(group, value)| {
                            (
                                format!("  捕获组 {group}"),
                                value.clone().unwrap_or_else(|| "未命中".into()),
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
                        "{:?} 引擎 · {} 处匹配{} · 步数上限 {}",
                        out.engine,
                        out.matches.len(),
                        if out.truncated {
                            "（已截断前 5k 条）"
                        } else {
                            ""
                        },
                        if manual { "1,000,000" } else { "200,000" }
                    ),
                    ..ToolResult::default()
                };
                if !inputs.replacement.is_empty() {
                    result.rows.push(("替换结果预览".into(), output));
                }
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
                if result.rows.is_empty() {
                    result.rows.push((
                        "查询结果".into(),
                        "RFC 9535 表达式合法，当前 JSON 中命中 0 个节点".into(),
                    ));
                }
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
                    ("中文自然释义".into(), out.description_zh),
                    ("English".into(), out.description_en),
                ];
                rows.extend(
                    out.next_triggers
                        .into_iter()
                        .enumerate()
                        .map(|(i, t)| (format!("第 {} 次触发 ({})", i + 1, out.time_zone), t)),
                );
                ToolResult::rows(rows)
            }
            Self::Chmod => {
                let p = parse_permissions(input)?;
                let target = if inputs.query.is_empty() {
                    "file.sh"
                } else {
                    inputs.query.as_str()
                };
                let mut res = ToolResult::rows(vec![
                    ("八进制表示".into(), p.to_octal()),
                    ("符号表示".into(), p.to_symbolic()),
                    ("终端命令".into(), p.to_command(target)),
                    (
                        "特殊权限位".into(),
                        format!(
                            "SUID: {} · SGID: {} · Sticky: {}",
                            if p.suid { "开启" } else { "关闭" },
                            if p.sgid { "开启" } else { "关闭" },
                            if p.sticky { "开启" } else { "关闭" }
                        ),
                    ),
                ]);
                res.footer = format!("{} ({})", p.to_octal(), p.to_symbolic());
                res
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
                    let verify_label = if *mode == 1 {
                        "✓ HMAC 签名校验通过"
                    } else {
                        "仅离线解码（未验证签名）"
                    };
                    let mut res = ToolResult::rows(vec![
                        ("签名算法".into(), out.algorithm.clone()),
                        ("Claims 时效".into(), out.status.label_zh().into()),
                        ("验签状态".into(), verify_label.into()),
                        ("Header".into(), pretty(out.header.to_string())?),
                        ("Payload Claims".into(), pretty(out.claims.to_string())?),
                    ]);
                    res.footer = format!("{} · {}", out.algorithm, out.status.label_zh());
                    res
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
                    let matched =
                        security::compare_digest_compute_time(&out.hex, inputs.expected.trim());
                    result.rows = vec![
                        ("计算摘要".into(), out.hex.clone()),
                        ("预期摘要".into(), inputs.expected.trim().to_owned()),
                        (
                            "恒定时间比对".into(),
                            if matched {
                                "✓ 摘要完全一致"
                            } else {
                                "× 摘要不匹配"
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
                let cert_count = out.certificates.len();
                let mut rows = Vec::new();
                for (i, cert) in out.certificates.into_iter().enumerate() {
                    rows.extend([
                        (format!("证书 #{} Subject", i + 1), cert.subject),
                        ("签发者 Issuer".into(), cert.issuer),
                        (
                            "主题备用名 SAN".into(),
                            if cert.san.is_empty() {
                                "（未配置 SAN 扩展）".into()
                            } else {
                                cert.san.join(", ")
                            },
                        ),
                        (
                            "有效期状态".into(),
                            format!(
                                "{} ({} → {})",
                                cert.validity.label_zh(),
                                cert.not_before,
                                cert.not_after
                            ),
                        ),
                        ("SHA-256 指纹".into(), cert.fingerprint_sha256),
                        ("SHA-1 指纹".into(), cert.fingerprint_sha1),
                    ]);
                }
                let mut res = ToolResult::rows(rows);
                res.footer = format!("成功解析 {cert_count} 张证书 · 零私钥暴露");
                res
            }
            Self::Cidr => {
                let out = security::parse_and_analyze(input)?;
                let mut res = ToolResult::rows(vec![
                    ("规范化子网".into(), out.networks.join(", ")),
                    ("网络地址".into(), out.network.to_string()),
                    ("广播地址".into(), out.broadcast.to_string()),
                    (
                        "子网掩码".into(),
                        format!("{} (/{})", out.netmask, out.prefix_len),
                    ),
                    (
                        "可用主机范围".into(),
                        format!("{} — {}", out.first_host, out.last_host),
                    ),
                    (
                        "可用主机 / 总地址".into(),
                        format!("{} / {}", out.usable_hosts, out.total_addresses),
                    ),
                    ("二进制位模式".into(), security::to_binary(out.network)),
                    ("十六进制表示".into(), security::to_hex(out.network)),
                    (
                        "IPv4-Mapped IPv6".into(),
                        out.network.to_ipv6_mapped().to_string(),
                    ),
                    (
                        "RFC 地址属性".into(),
                        format!(
                            "私有网段: {} · 回环: {} · 链路本地: {} · 组播: {}",
                            if out.is_private { "是" } else { "否" },
                            if out.is_loopback { "是" } else { "否" },
                            if out.is_link_local { "是" } else { "否" },
                            if out.is_multicast { "是" } else { "否" },
                        ),
                    ),
                ]);
                res.footer = format!(
                    "{}/{} · 可用主机 {}",
                    out.network, out.prefix_len, out.usable_hosts
                );
                res
            }
        })
    }

    pub fn inputs(&self, view: &ToolView, palette: Palette, cx: &Context<ToolView>) -> AnyElement {
        let mut stack = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .size_full()
            .flex()
            .flex_col();

        match self {
            Self::Diff { .. } | Self::JsonDiff { .. } | Self::List { .. } => {
                if matches!(self, Self::List { .. }) {
                    stack = stack.child(view.inline_field(
                        Slot::Query,
                        "项分隔符（留空默认按换行符切分）",
                        palette,
                    ));
                }
                stack = stack
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .px_3()
                                    .py_1()
                                    .bg(rgb(palette.panel_alt))
                                    .border_b_1()
                                    .border_color(rgb(palette.border))
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(palette.muted))
                                    .child("输入 A（基准）"),
                            )
                            .child(view.full_editor(Slot::Text)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .border_t_1()
                            .border_color(rgb(palette.border))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .px_3()
                                    .py_1()
                                    .bg(rgb(palette.panel_alt))
                                    .border_b_1()
                                    .border_color(rgb(palette.border))
                                    .text_size(px(11.0))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(palette.muted))
                                    .child("输入 B（对比）"),
                            )
                            .child(view.full_editor(Slot::Second)),
                    );
            }
            Self::Regex { .. } => {
                stack = stack
                    .child(view.inline_field(Slot::Query, "正则表达式 Pattern", palette))
                    .child(view.inline_field(
                        Slot::Replacement,
                        "替换模板（可选，支持 $1 / ${name} 捕获组引用）",
                        palette,
                    ))
                    .child(view.full_editor(Slot::Text));
            }
            Self::JsonPath { .. } => {
                stack = stack
                    .child(view.inline_field(
                        Slot::Query,
                        "JSONPath 查询表达式（RFC 9535 标准，默认 $）",
                        palette,
                    ))
                    .child(view.full_editor(Slot::Text));
            }
            Self::Chmod => {
                let permissions = parse_permissions(&view.inputs.text).unwrap_or_else(|_| {
                    debug::Permissions::from_octal(0o755).expect("valid permissions")
                });
                stack = stack
                    .child(render_chmod_matrix(permissions, view, palette, cx))
                    .child(view.inline_field(
                        Slot::Query,
                        "目标文件/目录路径（用于生成 chmod 命令）",
                        palette,
                    ))
                    .child(
                        div()
                            .flex_shrink_0()
                            .px_3()
                            .py_1()
                            .bg(rgb(palette.panel_alt))
                            .border_b_1()
                            .border_color(rgb(palette.border))
                            .text_size(px(11.0))
                            .text_color(rgb(palette.muted))
                            .child("八进制或符号表达式（如 755、0644、rwxr-xr-x，与上方 3×3 矩阵双向联动）"),
                    )
                    .child(view.full_editor(Slot::Text));
            }
            Self::Jwt { mode, .. } => {
                if *mode != 0 {
                    stack = stack.child(view.inline_field(
                        Slot::Secret,
                        "HMAC 共享密钥（仅驻内存，退出后自动清零）",
                        palette,
                    ));
                }
                stack = stack.child(view.full_editor(Slot::Text));
            }
            Self::Hash { hmac, .. } => {
                if *hmac {
                    stack = stack.child(view.inline_field(
                        Slot::Secret,
                        "HMAC 密钥（仅驻内存）",
                        palette,
                    ));
                }
                stack = stack
                    .child(view.inline_field(
                        Slot::Expected,
                        "预期摘要比对框（可选，采用恒定时间算法防时序侧信道）",
                        palette,
                    ))
                    .child(view.full_editor(Slot::Text));
            }
            Self::X509 { .. } => {
                stack = stack
                    .child(view.inline_field(
                        Slot::Secret,
                        "PFX / PKCS#12 解密口令（可选，仅内存 Zeroize）",
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
            Self::Diff {
                mode,
                whitespace,
                case,
            } => {
                groups.push(option_group(
                    "差异视图",
                    palette,
                    view.segmented(
                        "diff-mode",
                        &["Unified 补丁", "行内着色", "并排表格"],
                        *mode,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Debug(Self::Diff { mode, .. }) = &mut view.model {
                                *mode = idx;
                            }
                        },
                    ),
                ));
                groups.push(view.toggle_pill(
                    "diff-ws",
                    "忽略空白",
                    *whitespace,
                    palette,
                    cx,
                    |v| {
                        if let Model::Debug(Self::Diff { whitespace, .. }) = &mut v.model {
                            *whitespace = !*whitespace;
                        }
                    },
                ));
                groups.push(view.toggle_pill(
                    "diff-case",
                    "忽略大小写",
                    *case,
                    palette,
                    cx,
                    |v| {
                        if let Model::Debug(Self::Diff { case, .. }) = &mut v.model {
                            *case = !*case;
                        }
                    },
                ));
            }
            Self::JsonDiff {
                unordered,
                canonical,
            } => {
                groups.push(option_group(
                    "展示方式",
                    palette,
                    view.segmented(
                        "jdiff-view",
                        &["JSON Pointer 差异列表", "规范化双栏预览"],
                        usize::from(*canonical),
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Debug(Self::JsonDiff { canonical, .. }) = &mut view.model
                            {
                                *canonical = idx == 1;
                            }
                        },
                    ),
                ));
                groups.push(view.toggle_pill(
                    "jdiff-unord",
                    "数组忽略顺序",
                    *unordered,
                    palette,
                    cx,
                    |v| {
                        if let Model::Debug(Self::JsonDiff { unordered, .. }) = &mut v.model {
                            *unordered = !*unordered;
                        }
                    },
                ));
            }
            Self::Analysis {
                tab,
                dedupe,
                sort,
                reverse,
                trim,
                crlf,
            } => {
                groups.push(option_group(
                    "功能面板",
                    palette,
                    view.segmented(
                        "ana-tab",
                        &["12 种命名转换", "行批处理", "Unicode 与词频"],
                        *tab,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Debug(Self::Analysis { tab, .. }) = &mut view.model {
                                *tab = idx;
                            }
                        },
                    ),
                ));
                if *tab == 1 {
                    groups.push(option_group(
                        "换行符",
                        palette,
                        view.segmented(
                            "ana-eol",
                            &["LF (\\n)", "CRLF (\\r\\n)"],
                            usize::from(*crlf),
                            palette,
                            cx,
                            |view, idx| {
                                if let Model::Debug(Self::Analysis { crlf, .. }) = &mut view.model {
                                    *crlf = idx == 1;
                                }
                            },
                        ),
                    ));
                    groups.push(view.toggle_pill(
                        "ana-dedupe",
                        "去重",
                        *dedupe,
                        palette,
                        cx,
                        |v| {
                            if let Model::Debug(Self::Analysis { dedupe, .. }) = &mut v.model {
                                *dedupe = !*dedupe;
                            }
                        },
                    ));
                    groups.push(view.toggle_pill(
                        "ana-sort",
                        "字典排序",
                        *sort,
                        palette,
                        cx,
                        |v| {
                            if let Model::Debug(Self::Analysis { sort, .. }) = &mut v.model {
                                *sort = !*sort;
                            }
                        },
                    ));
                    groups.push(view.toggle_pill(
                        "ana-rev",
                        "反转行序",
                        *reverse,
                        palette,
                        cx,
                        |v| {
                            if let Model::Debug(Self::Analysis { reverse, .. }) = &mut v.model {
                                *reverse = !*reverse;
                            }
                        },
                    ));
                    groups.push(view.toggle_pill(
                        "ana-trim",
                        "修剪首尾空白",
                        *trim,
                        palette,
                        cx,
                        |v| {
                            if let Model::Debug(Self::Analysis { trim, .. }) = &mut v.model {
                                *trim = !*trim;
                            }
                        },
                    ));
                }
            }
            Self::List {
                tab,
                trim,
                dedupe,
                case,
            } => {
                groups.push(option_group(
                    "集合运算",
                    palette,
                    view.segmented(
                        "list-tab",
                        &["交集 (A ∩ B)", "并集 (A ∪ B)", "仅在 A", "仅在 B"],
                        *tab,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Debug(Self::List { tab, .. }) = &mut view.model {
                                *tab = idx;
                            }
                        },
                    ),
                ));
                groups.push(view.toggle_pill(
                    "list-trim",
                    "首尾修剪",
                    *trim,
                    palette,
                    cx,
                    |v| {
                        if let Model::Debug(Self::List { trim, .. }) = &mut v.model {
                            *trim = !*trim;
                        }
                    },
                ));
                groups.push(
                    view.toggle_pill("list-dedupe", "去重", *dedupe, palette, cx, |v| {
                        if let Model::Debug(Self::List { dedupe, .. }) = &mut v.model {
                            *dedupe = !*dedupe;
                        }
                    }),
                );
                groups.push(view.toggle_pill(
                    "list-case",
                    "忽略大小写",
                    *case,
                    palette,
                    cx,
                    |v| {
                        if let Model::Debug(Self::List { case, .. }) = &mut v.model {
                            *case = !*case;
                        }
                    },
                ));
            }
            Self::Regex { flags, strict } => {
                let mut flag_row = div().flex().items_center().gap_1();
                for (index, label) in ["i", "m", "s", "x", "U", "g"].into_iter().enumerate() {
                    flag_row = flag_row.child(view.toggle_pill(
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
                groups.push(option_group("标志位", palette, flag_row));
                groups.push(option_group(
                    "引擎策略",
                    palette,
                    view.segmented(
                        "re-engine",
                        &["自动双引擎", "严格线性 DFA"],
                        usize::from(*strict),
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Debug(Self::Regex { strict, .. }) = &mut view.model {
                                *strict = idx == 1;
                            }
                        },
                    ),
                ));
            }
            Self::JsonPath { values } => {
                groups.push(option_group(
                    "输出内容",
                    palette,
                    view.segmented(
                        "jp-view",
                        &["匹配值 (Values)", "规范路径 (Paths)"],
                        usize::from(!*values),
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Debug(Self::JsonPath { values }) = &mut view.model {
                                *values = idx == 0;
                            }
                        },
                    ),
                ));
                let mut presets = div().flex().items_center().gap_1();
                for (i, (label, expr)) in [
                    ("根节点 $", "$"),
                    ("递归键 $..*", "$..*"),
                    ("首元素 $[0]", "$[0]"),
                ]
                .into_iter()
                .enumerate()
                {
                    presets = presets.child(view.action_pill(
                        &format!("jp-preset-{i}"),
                        None,
                        label,
                        palette,
                        cx,
                        move |view| view.inputs.query = expr.into(),
                    ));
                }
                groups.push(option_group("快捷表达式", palette, presets));
            }
            Self::Cron { zone, count } => {
                groups.push(option_group(
                    "时区",
                    palette,
                    view.segmented("cron-zone", &ZONES, *zone, palette, cx, |view, idx| {
                        if let Model::Debug(Self::Cron { zone, .. }) = &mut view.model {
                            *zone = idx;
                        }
                    }),
                ));
                groups.push(option_group(
                    "预测条数",
                    palette,
                    view.segmented(
                        "cron-count",
                        &["5 次", "10 次", "20 次", "50 次"],
                        *count,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Debug(Self::Cron { count, .. }) = &mut view.model {
                                *count = idx;
                            }
                        },
                    ),
                ));
                let mut presets = div().flex().items_center().gap_1();
                for (index, (label, expr)) in [
                    ("每分钟", "* * * * *"),
                    ("每 5 分钟", "*/5 * * * *"),
                    ("每日 09:00", "0 9 * * *"),
                    ("工作日 09:00", "0 9 * * 1-5"),
                ]
                .into_iter()
                .enumerate()
                {
                    presets = presets.child(view.action_pill(
                        &format!("cron-preset-{index}"),
                        None,
                        label,
                        palette,
                        cx,
                        move |view| view.inputs.text = expr.into(),
                    ));
                }
                groups.push(option_group("预设", palette, presets));
            }
            Self::Chmod => {
                let permissions = parse_permissions(&view.inputs.text).unwrap_or_else(|_| {
                    debug::Permissions::from_octal(0o755).expect("valid permissions")
                });
                let mut presets = div().flex().items_center().gap_1();
                for (idx, (label, oct)) in [
                    ("755 可执行", "755"),
                    ("644 常规文件", "644"),
                    ("600 私密密钥", "600"),
                    ("700 私有目录", "700"),
                    ("777 完全开放", "777"),
                ]
                .into_iter()
                .enumerate()
                {
                    presets = presets.child(view.action_pill(
                        &format!("chmod-preset-{idx}"),
                        None,
                        label,
                        palette,
                        cx,
                        move |view| view.inputs.text = oct.into(),
                    ));
                }
                groups.push(option_group("常用预设", palette, presets));

                let mut special = div().flex().items_center().gap_1();
                for (label, bit, active) in [
                    ("SUID (4)", 0o4000u16, permissions.suid),
                    ("SGID (2)", 0o2000u16, permissions.sgid),
                    ("Sticky (1)", 0o1000u16, permissions.sticky),
                ] {
                    special = special.child(view.toggle_pill(
                        label,
                        label,
                        active,
                        palette,
                        cx,
                        move |view| {
                            let current =
                                parse_permissions(&view.inputs.text).unwrap_or_else(|_| {
                                    debug::Permissions::from_octal(0o755)
                                        .expect("valid permissions")
                                });
                            let value =
                                u16::from_str_radix(&current.to_octal(), 8).unwrap_or(0o755) ^ bit;
                            view.inputs.text = debug::Permissions::from_octal(value)
                                .expect("permission bits")
                                .to_octal();
                        },
                    ));
                }
                groups.push(option_group("特殊位", palette, special));
            }
            Self::Jwt { mode, algorithm } => {
                groups.push(option_group(
                    "操作模式",
                    palette,
                    view.segmented(
                        "jwt-mode",
                        &["零密钥解码", "HMAC 验签", "HMAC 签发"],
                        *mode,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Debug(Self::Jwt { mode, .. }) = &mut view.model {
                                *mode = idx;
                            }
                        },
                    ),
                ));
                if *mode != 0 {
                    groups.push(option_group(
                        "算法",
                        palette,
                        view.segmented(
                            "jwt-alg",
                            &["HS256", "HS384", "HS512"],
                            *algorithm,
                            palette,
                            cx,
                            |view, idx| {
                                if let Model::Debug(Self::Jwt { algorithm, .. }) = &mut view.model {
                                    *algorithm = idx;
                                }
                            },
                        ),
                    ));
                }
            }
            Self::Hash {
                algorithm,
                file,
                hmac,
            } => {
                groups.push(option_group(
                    "摘要算法",
                    palette,
                    view.segmented(
                        "hash-alg",
                        &["MD5", "SHA-1", "SHA-256", "SHA-512", "BLAKE3"],
                        *algorithm,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Debug(Self::Hash { algorithm, .. }) = &mut view.model {
                                *algorithm = idx;
                            }
                        },
                    ),
                ));
                groups.push(option_group(
                    "输入源",
                    palette,
                    view.segmented(
                        "hash-src",
                        &["文本内容", "本地大文件流式"],
                        usize::from(*file),
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Debug(Self::Hash { file, .. }) = &mut view.model {
                                *file = idx == 1;
                            }
                        },
                    ),
                ));
                groups.push(view.toggle_pill(
                    "hash-hmac",
                    "HMAC 密钥认证",
                    *hmac,
                    palette,
                    cx,
                    |v| {
                        if let Model::Debug(Self::Hash { hmac, .. }) = &mut v.model {
                            *hmac = !*hmac;
                        }
                    },
                ));
            }
            Self::X509 { file } => {
                groups.push(option_group(
                    "证书来源",
                    palette,
                    view.segmented(
                        "x509-src",
                        &["PEM 证书文本", "本地文件 (DER / PFX / PEM)"],
                        usize::from(*file),
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Debug(Self::X509 { file }) = &mut view.model {
                                *file = idx == 1;
                            }
                        },
                    ),
                ));
                groups.push(
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(rgb(palette.success_soft))
                        .text_size(px(11.0))
                        .text_color(rgb(palette.success))
                        .child(icon_el(IconName::ShieldCheck, 12.0, palette.success))
                        .child("严格拒绝私钥明文 · 纯离线解析")
                        .into_any_element(),
                );
            }
            Self::Cidr => {
                let mut presets = div().flex().items_center().gap_1();
                for (idx, (label, cidr)) in [
                    ("/24 局域网", "192.168.1.0/24"),
                    ("/16 私有网段", "10.20.0.0/16"),
                    ("/31 点对点", "172.16.0.10/31"),
                    ("/32 单主机", "203.0.113.8/32"),
                ]
                .into_iter()
                .enumerate()
                {
                    presets = presets.child(view.action_pill(
                        &format!("cidr-preset-{idx}"),
                        None,
                        label,
                        palette,
                        cx,
                        move |view| view.inputs.text = cidr.into(),
                    ));
                }
                groups.push(option_group("典型网段预设", palette, presets));
            }
        }

        options_bar(palette, groups)
    }
}

fn render_chmod_matrix(
    permissions: debug::Permissions,
    _view: &ToolView,
    palette: Palette,
    cx: &Context<ToolView>,
) -> AnyElement {
    let header = div()
        .flex()
        .items_center()
        .gap_2()
        .px_2p5()
        .py_1p5()
        .bg(rgb(palette.chrome))
        .border_b_1()
        .border_color(rgb(palette.border))
        .text_size(px(11.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(palette.muted))
        .child(div().w(px(112.0)).child("角色 Role"))
        .child(div().flex_1().text_center().child("读取 Read (4)"))
        .child(div().flex_1().text_center().child("写入 Write (2)"))
        .child(div().flex_1().text_center().child("执行 Exec (1)"));

    let mut table = div()
        .rounded_md()
        .border_1()
        .border_color(rgb(palette.border))
        .overflow_hidden()
        .child(header);

    for (row_idx, (role_label, triplet)) in [
        ("所有者 Owner", permissions.owner),
        ("用户组 Group", permissions.group),
        ("其他人 Others", permissions.others),
    ]
    .into_iter()
    .enumerate()
    {
        let mut line = div()
            .flex()
            .items_center()
            .gap_2()
            .px_2p5()
            .py_1p5()
            .when(row_idx < 2, |el| {
                el.border_b_1().border_color(rgb(palette.border))
            })
            .child(
                div()
                    .w(px(112.0))
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(palette.text))
                    .child(role_label),
            );

        for (col_idx, (sym, active)) in [
            ("r", triplet.read),
            ("w", triplet.write),
            ("x", triplet.execute),
        ]
        .into_iter()
        .enumerate()
        {
            let bit = 1u16 << (8 - (row_idx * 3 + col_idx));
            line = line.child(
                div()
                    .id(format!("chmod-cell-{row_idx}-{col_idx}"))
                    .flex_1()
                    .h(px(28.0))
                    .rounded_md()
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_1()
                    .border_1()
                    .border_color(rgb(if active {
                        palette.accent
                    } else {
                        palette.border
                    }))
                    .bg(rgb(if active {
                        palette.accent_soft
                    } else {
                        palette.panel_alt
                    }))
                    .hover(move |s| {
                        s.bg(rgb(if active {
                            palette.accent_soft
                        } else {
                            palette.hover
                        }))
                    })
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(if active {
                        palette.accent
                    } else {
                        palette.subtle
                    }))
                    .when(active, |el| {
                        el.child(icon_el(IconName::Check, 11.5, palette.accent))
                    })
                    .child(sym)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let current = parse_permissions(&this.inputs.text).unwrap_or_else(|_| {
                            debug::Permissions::from_octal(0o755).expect("valid permissions")
                        });
                        let value =
                            u16::from_str_radix(&current.to_octal(), 8).unwrap_or(0o755) ^ bit;
                        this.inputs.text = debug::Permissions::from_octal(value)
                            .expect("permission bits")
                            .to_octal();
                        this.trigger_option_change(true, cx);
                    })),
            );
        }
        table = table.child(line);
    }

    div()
        .flex_shrink_0()
        .p_3()
        .border_b_1()
        .border_color(rgb(palette.border))
        .child(table)
        .into_any_element()
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
