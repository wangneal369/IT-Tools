//! The seven single-input/single-output standard transforms (ADR-0004).

use devtools_core::{CancelFlag, ToolError, ToolId, ToolLaunchInput};
use devtools_tools::{
    conversion::data_format as data,
    encode_decode::{gzip, html_unicode as html, string_escape as escape},
    format::{json, markup, sql},
};
use gpui_kit::*;

use super::Model;
use super::runtime::{Inputs, ToolResult, ToolView};
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
        let mut bar = div().flex().flex_wrap().gap_2().p_2();
        macro_rules! choices {
            ($field:ident, $labels:expr) => {
                for (index, label) in $labels.into_iter().enumerate() {
                    bar = bar.child(view.chip(
                        &format!("{}-{index}", stringify!($field)),
                        label,
                        self.$field == index,
                        palette,
                        cx,
                        move |view| {
                            if let Model::Standard(options) = &mut view.model {
                                options.$field = index;
                            }
                        },
                    ));
                }
            };
        }
        macro_rules! toggle {
            ($field:ident, $label:expr) => {
                bar = bar.child(view.chip(
                    stringify!($field),
                    $label,
                    self.$field,
                    palette,
                    cx,
                    |view| {
                        if let Model::Standard(options) = &mut view.model {
                            options.$field = !options.$field;
                        }
                    },
                ));
            };
        }
        match self.tool {
            ToolId::DataFormatConverter => {
                bar = bar.child(div().text_xs().child("源"));
                choices!(from, ["JSON", "YAML", "TOML", "XML"]);
                bar = bar.child(div().text_xs().child("目标"));
                choices!(to, ["JSON", "YAML", "TOML", "XML"]);
                choices!(indent, ["缩进 2", "缩进 4"]);
                toggle!(toggle, "排序键");
            }
            ToolId::HtmlEntityUnicode => {
                toggle!(reverse, "解码（关闭为编码）");
                choices!(style, ["HTML Entity", "\\uXXXX", "CSS \\XXXXXX", "U+XXXX"]);
                toggle!(toggle, "仅非 ASCII");
            }
            ToolId::StringEscape => {
                toggle!(reverse, "反转义（关闭为转义）");
                choices!(style, ["JSON", "C-Rust", "Shell-POSIX", "Regex"]);
                toggle!(lines, "逐行");
            }
            ToolId::GzipCompress => {
                toggle!(reverse, "解压（关闭为压缩）");
                choices!(style, ["Base64", "Hex"]);
                for level in 1..=9 {
                    bar = bar.child(view.chip(
                        &format!("level-{level}"),
                        level.to_string(),
                        self.level == level,
                        palette,
                        cx,
                        move |view| {
                            if let Model::Standard(options) = &mut view.model {
                                options.level = level;
                            }
                        },
                    ));
                }
            }
            ToolId::JsonFormatter => {
                toggle!(reverse, "压缩（关闭为美化）");
                choices!(indent, ["缩进 2", "缩进 4", "Tab"]);
                toggle!(toggle, "排序键");
            }
            ToolId::SqlFormatter => {
                choices!(style, ["Upper", "Lower", "Preserve"]);
                toggle!(reverse, "Compact（关闭为 Expanded）");
                choices!(indent, ["缩进 2", "缩进 4"]);
            }
            ToolId::XmlHtmlFormatter => {
                choices!(style, ["Auto", "XML", "HTML"]);
                toggle!(reverse, "压缩（关闭为美化）");
                choices!(indent, ["缩进 2", "缩进 4", "Tab"]);
            }
            _ => (),
        }
        bar.into_any_element()
    }
}
