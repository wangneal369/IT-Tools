//! The seven single-input/single-output standard transforms (ADR-0004).

use devtools_core::{CancelFlag, ToolError, ToolId, ToolLaunchInput};
use devtools_tools::{
    conversion::data_format as data,
    encode_decode::{gzip, html_unicode as html, string_escape as escape},
    format::{json, markup, sql},
};
use gpui_kit::*;

use super::Model;
use super::runtime::{Inputs, ToolResult, ToolView, option_group, options_bar};
use crate::icons::IconName;
use crate::theme::Palette;

#[derive(Clone)]
pub(super) struct StandardOptions {
    tool: ToolId,
    reverse: bool,
    style: usize,
    from: usize,
    to: usize,
    indent: usize,
    toggle: bool,
    lines: bool,
    level: u32,
}

impl StandardOptions {
    pub fn new(tool: ToolId) -> Self {
        Self {
            tool,
            reverse: false,
            style: 0,
            from: 0,
            to: 1,
            indent: 0,
            toggle: false,
            lines: false,
            level: 6,
        }
    }

    pub fn inject(&mut self, text: &str, _: Option<&ToolLaunchInput>) {
        match self.tool {
            ToolId::DataFormatConverter => {
                self.from = if text.trim_start().starts_with('<') {
                    3
                } else if text.trim_start().starts_with(['{', '[']) {
                    0
                } else if text.lines().any(|l| l.contains('=')) {
                    2
                } else {
                    1
                };
                self.to = if self.from == 0 { 1 } else { 0 };
            }
            ToolId::GzipCompress => self.reverse = text.trim().starts_with("H4sI"),
            ToolId::HtmlEntityUnicode => {
                self.reverse = text.contains("\\u") || text.contains("&#") || text.contains("&amp;")
            }
            _ => (),
        }
    }

    pub fn execute(&self, input: &Inputs, cancel: &CancelFlag) -> Result<ToolResult, ToolError> {
        let text = input.text.clone();
        let output = match self.tool {
            ToolId::DataFormatConverter => {
                data::convert(&data::DataFormatConverterRequest {
                    input: text,
                    from: [
                        data::DataFormat::Json,
                        data::DataFormat::Yaml,
                        data::DataFormat::Toml,
                        data::DataFormat::Xml,
                    ][self.from],
                    to: [
                        data::DataFormat::Json,
                        data::DataFormat::Yaml,
                        data::DataFormat::Toml,
                        data::DataFormat::Xml,
                    ][self.to],
                    indent: if self.indent == 0 {
                        data::IndentStyle::Two
                    } else {
                        data::IndentStyle::Four
                    },
                    sort_keys: self.toggle,
                })?
                .output
            }
            ToolId::HtmlEntityUnicode => {
                html::run(&html::HtmlEntityUnicodeRequest {
                    input: text,
                    mode: if self.reverse {
                        html::HtmlUnicodeMode::Decode
                    } else {
                        html::HtmlUnicodeMode::Encode
                    },
                    style: [
                        html::UnicodeEscapeStyle::HtmlEntity,
                        html::UnicodeEscapeStyle::JsonUnicode,
                        html::UnicodeEscapeStyle::CssEscape,
                        html::UnicodeEscapeStyle::Codepoint,
                    ][self.style],
                    non_ascii_only: self.toggle,
                })?
                .output
            }
            ToolId::StringEscape => {
                escape::run(&escape::StringEscapeRequest {
                    input: text,
                    direction: if self.reverse {
                        escape::EscapeDirection::Unescape
                    } else {
                        escape::EscapeDirection::Escape
                    },
                    style: [
                        escape::EscapeStyle::Json,
                        escape::EscapeStyle::CLike,
                        escape::EscapeStyle::ShellPosix,
                        escape::EscapeStyle::Regex,
                    ][self.style],
                    line_by_line: self.lines,
                })?
                .output
            }
            ToolId::GzipCompress => {
                let result = gzip::run(
                    &gzip::GzipCompressRequest {
                        input: text,
                        action: if self.reverse {
                            gzip::GzipAction::Decompress
                        } else {
                            gzip::GzipAction::Compress
                        },
                        transport: if self.style == 0 {
                            gzip::GzipTransport::Base64
                        } else {
                            gzip::GzipTransport::Hex
                        },
                        level: self.level,
                    },
                    Some(cancel),
                )?;
                return Ok(ToolResult {
                    output: result.output,
                    footer: format!(
                        "{} → {} 字节 · 压缩比 {:.2}%",
                        result.input_bytes,
                        result.output_bytes,
                        result.ratio * 100.
                    ),
                    ..ToolResult::default()
                });
            }
            ToolId::JsonFormatter => {
                json::run(&json::JsonFormatterRequest {
                    input: text,
                    pretty: !self.reverse,
                    indent: [
                        json::JsonIndent::Two,
                        json::JsonIndent::Four,
                        json::JsonIndent::Tab,
                    ][self.indent],
                    sort_keys: self.toggle,
                })?
                .output
            }
            ToolId::SqlFormatter => {
                sql::run(&sql::SqlFormatterRequest {
                    input: text,
                    keyword_case: [
                        sql::SqlKeywordCase::Upper,
                        sql::SqlKeywordCase::Lower,
                        sql::SqlKeywordCase::Preserve,
                    ][self.style],
                    layout: if self.reverse {
                        sql::SqlLayout::Compact
                    } else {
                        sql::SqlLayout::Expanded
                    },
                    indent: if self.indent == 0 {
                        sql::SqlIndent::Two
                    } else {
                        sql::SqlIndent::Four
                    },
                })?
                .output
            }
            ToolId::XmlHtmlFormatter => {
                markup::run(&markup::XmlHtmlFormatterRequest {
                    input: text,
                    language: [
                        markup::MarkupLanguage::Auto,
                        markup::MarkupLanguage::Xml,
                        markup::MarkupLanguage::Html,
                    ][self.style],
                    pretty: !self.reverse,
                    indent: [
                        markup::MarkupIndent::Two,
                        markup::MarkupIndent::Four,
                        markup::MarkupIndent::Tab,
                    ][self.indent],
                })?
                .output
            }
            _ => unreachable!("only standard tools construct StandardOptions"),
        };
        Ok(ToolResult::text(output))
    }

    pub fn controls(
        &self,
        view: &ToolView,
        palette: Palette,
        cx: &Context<ToolView>,
    ) -> AnyElement {
        let mut groups: Vec<AnyElement> = Vec::new();

        match self.tool {
            ToolId::DataFormatConverter => {
                groups.push(option_group(
                    "源格式",
                    palette,
                    view.segmented(
                        "from",
                        &["JSON", "YAML", "TOML", "XML"],
                        self.from,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Standard(opt) = &mut view.model {
                                opt.from = idx;
                            }
                        },
                    ),
                ));
                groups.push(view.action_pill(
                    "swap-format",
                    Some(IconName::ArrowLeftRight),
                    "互换",
                    palette,
                    cx,
                    |view| {
                        if let Model::Standard(opt) = &mut view.model {
                            std::mem::swap(&mut opt.from, &mut opt.to);
                        }
                    },
                ));
                groups.push(option_group(
                    "目标格式",
                    palette,
                    view.segmented(
                        "to",
                        &["JSON", "YAML", "TOML", "XML"],
                        self.to,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Standard(opt) = &mut view.model {
                                opt.to = idx;
                            }
                        },
                    ),
                ));
                groups.push(option_group(
                    "缩进",
                    palette,
                    view.segmented(
                        "indent",
                        &["2 空格", "4 空格"],
                        self.indent,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Standard(opt) = &mut view.model {
                                opt.indent = idx;
                            }
                        },
                    ),
                ));
                groups.push(view.toggle_pill(
                    "sort-keys",
                    "按键名排序",
                    self.toggle,
                    palette,
                    cx,
                    |view| {
                        if let Model::Standard(opt) = &mut view.model {
                            opt.toggle = !opt.toggle;
                        }
                    },
                ));
            }
            ToolId::HtmlEntityUnicode => {
                groups.push(option_group(
                    "方向",
                    palette,
                    view.segmented(
                        "mode",
                        &["编码", "解码"],
                        usize::from(self.reverse),
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Standard(opt) = &mut view.model {
                                opt.reverse = idx == 1;
                            }
                        },
                    ),
                ));
                groups.push(option_group(
                    "格式",
                    palette,
                    view.segmented(
                        "style",
                        &["HTML 实体", "\\uXXXX", "CSS \\XXXXXX", "U+XXXX"],
                        self.style,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Standard(opt) = &mut view.model {
                                opt.style = idx;
                            }
                        },
                    ),
                ));
                groups.push(view.toggle_pill(
                    "non-ascii",
                    "仅非 ASCII",
                    self.toggle,
                    palette,
                    cx,
                    |view| {
                        if let Model::Standard(opt) = &mut view.model {
                            opt.toggle = !opt.toggle;
                        }
                    },
                ));
            }
            ToolId::StringEscape => {
                groups.push(option_group(
                    "方向",
                    palette,
                    view.segmented(
                        "escape-dir",
                        &["转义", "反转义"],
                        usize::from(self.reverse),
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Standard(opt) = &mut view.model {
                                opt.reverse = idx == 1;
                            }
                        },
                    ),
                ));
                groups.push(option_group(
                    "语法风格",
                    palette,
                    view.segmented(
                        "style",
                        &["JSON", "C / Rust", "Shell POSIX", "Regex"],
                        self.style,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Standard(opt) = &mut view.model {
                                opt.style = idx;
                            }
                        },
                    ),
                ));
                groups.push(view.toggle_pill(
                    "lines",
                    "逐行处理",
                    self.lines,
                    palette,
                    cx,
                    |view| {
                        if let Model::Standard(opt) = &mut view.model {
                            opt.lines = !opt.lines;
                        }
                    },
                ));
            }
            ToolId::GzipCompress => {
                groups.push(option_group(
                    "操作",
                    palette,
                    view.segmented(
                        "gzip-action",
                        &["GZip 压缩", "GZip 解压"],
                        usize::from(self.reverse),
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Standard(opt) = &mut view.model {
                                opt.reverse = idx == 1;
                            }
                        },
                    ),
                ));
                groups.push(option_group(
                    "编码载体",
                    palette,
                    view.segmented(
                        "transport",
                        &["Base64", "Hex"],
                        self.style,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Standard(opt) = &mut view.model {
                                opt.style = idx;
                            }
                        },
                    ),
                ));
                if !self.reverse {
                    groups.push(option_group(
                        "压缩等级",
                        palette,
                        view.segmented(
                            "gzip-level",
                            &["1", "2", "3", "4", "5", "6", "7", "8", "9"],
                            (self.level.clamp(1, 9) - 1) as usize,
                            palette,
                            cx,
                            |view, idx| {
                                if let Model::Standard(opt) = &mut view.model {
                                    opt.level = (idx + 1) as u32;
                                }
                            },
                        ),
                    ));
                }
            }
            ToolId::JsonFormatter => {
                groups.push(option_group(
                    "模式",
                    palette,
                    view.segmented(
                        "json-mode",
                        &["美化排版", "单行压缩"],
                        usize::from(self.reverse),
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Standard(opt) = &mut view.model {
                                opt.reverse = idx == 1;
                            }
                        },
                    ),
                ));
                if !self.reverse {
                    groups.push(option_group(
                        "缩进",
                        palette,
                        view.segmented(
                            "indent",
                            &["2 空格", "4 空格", "Tab"],
                            self.indent,
                            palette,
                            cx,
                            |view, idx| {
                                if let Model::Standard(opt) = &mut view.model {
                                    opt.indent = idx;
                                }
                            },
                        ),
                    ));
                }
                groups.push(view.toggle_pill(
                    "sort-keys",
                    "键字典序重排",
                    self.toggle,
                    palette,
                    cx,
                    |view| {
                        if let Model::Standard(opt) = &mut view.model {
                            opt.toggle = !opt.toggle;
                        }
                    },
                ));
            }
            ToolId::SqlFormatter => {
                groups.push(option_group(
                    "布局",
                    palette,
                    view.segmented(
                        "sql-layout",
                        &["展开美化", "紧凑单行"],
                        usize::from(self.reverse),
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Standard(opt) = &mut view.model {
                                opt.reverse = idx == 1;
                            }
                        },
                    ),
                ));
                groups.push(option_group(
                    "关键字",
                    palette,
                    view.segmented(
                        "sql-case",
                        &["大写", "小写", "保留原样"],
                        self.style,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Standard(opt) = &mut view.model {
                                opt.style = idx;
                            }
                        },
                    ),
                ));
                if !self.reverse {
                    groups.push(option_group(
                        "缩进",
                        palette,
                        view.segmented(
                            "sql-indent",
                            &["2 空格", "4 空格"],
                            self.indent,
                            palette,
                            cx,
                            |view, idx| {
                                if let Model::Standard(opt) = &mut view.model {
                                    opt.indent = idx;
                                }
                            },
                        ),
                    ));
                }
            }
            ToolId::XmlHtmlFormatter => {
                groups.push(option_group(
                    "语言",
                    palette,
                    view.segmented(
                        "markup-lang",
                        &["自动识别", "XML", "HTML"],
                        self.style,
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Standard(opt) = &mut view.model {
                                opt.style = idx;
                            }
                        },
                    ),
                ));
                groups.push(option_group(
                    "模式",
                    palette,
                    view.segmented(
                        "markup-mode",
                        &["美化排版", "紧凑压缩"],
                        usize::from(self.reverse),
                        palette,
                        cx,
                        |view, idx| {
                            if let Model::Standard(opt) = &mut view.model {
                                opt.reverse = idx == 1;
                            }
                        },
                    ),
                ));
                if !self.reverse {
                    groups.push(option_group(
                        "缩进",
                        palette,
                        view.segmented(
                            "markup-indent",
                            &["2 空格", "4 空格", "Tab"],
                            self.indent,
                            palette,
                            cx,
                            |view, idx| {
                                if let Model::Standard(opt) = &mut view.model {
                                    opt.indent = idx;
                                }
                            },
                        ),
                    ));
                }
            }
            _ => (),
        }

        options_bar(palette, groups)
    }
}
