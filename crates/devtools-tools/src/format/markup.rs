//! #13 XML / HTML 格式化（`markup_fmt`）。
//!
//! 自动 / XML / HTML 模式，美化或压缩，缩进 2/4/Tab；未闭合标签统一返回
//! `ToolError::InvalidSyntax` 并携带行列位置，供 UI 保留上一份有效输出。

use std::borrow::Cow;

use markup_fmt::config::FormatOptions;
use markup_fmt::{FormatError, Language};

use crate::util::invalid_syntax;

use devtools_core::{SourceSpan, ToolError};

/// 标记语言。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkupLanguage {
    /// 依据 `<!doctype html>` / `<html` 自动判断。
    Auto,
    Xml,
    Html,
}

/// 缩进风格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkupIndent {
    Two,
    Four,
    Tab,
}

impl MarkupIndent {
    const fn width(self) -> usize {
        match self {
            MarkupIndent::Two => 2,
            MarkupIndent::Four => 4,
            MarkupIndent::Tab => 1,
        }
    }

    const fn use_tabs(self) -> bool {
        matches!(self, MarkupIndent::Tab)
    }
}

/// 单次请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlHtmlFormatterRequest {
    pub input: String,
    pub language: MarkupLanguage,
    /// `true` 美化，`false` 压缩。
    pub pretty: bool,
    pub indent: MarkupIndent,
}

/// 结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlHtmlFormatterResponse {
    pub output: String,
    /// 实际使用的语言（`Auto` 已解析）。
    pub detected: MarkupLanguage,
}

/// 执行 XML / HTML 格式化。
pub fn run(req: &XmlHtmlFormatterRequest) -> Result<XmlHtmlFormatterResponse, ToolError> {
    const MAX_INPUT: usize = 8 * 1024 * 1024;
    if req.input.len() > MAX_INPUT {
        return Err(crate::util::size_limit("输入超过 8 MB 上限"));
    }
    let language = match req.language {
        MarkupLanguage::Auto => detect(&req.input),
        explicit => explicit,
    };
    let markup_language = match language {
        MarkupLanguage::Html => Language::Html,
        MarkupLanguage::Xml | MarkupLanguage::Auto => Language::Xml,
    };

    let mut options = FormatOptions::default();
    options.layout.use_tabs = req.indent.use_tabs();
    options.layout.indent_width = req.indent.width();

    // 始终先解析校验，未闭合标签即使选择压缩也要报错。
    let formatted = markup_fmt::format_text(&req.input, markup_language, &options, |code, _| {
        Ok(Cow::Borrowed(code))
    })
    .map_err(map_format_error)?;

    let output = if req.pretty {
        formatted
    } else {
        minify(&req.input, language)
    };
    Ok(XmlHtmlFormatterResponse {
        output,
        detected: language,
    })
}

fn map_format_error(error: FormatError) -> ToolError {
    match error {
        FormatError::Syntax(syntax) => {
            let line = syntax.line.max(1);
            let column = syntax.column.max(1);
            invalid_syntax(format!("标记语法错误：{syntax}"))
                .with_span(SourceSpan::new(line, column, line, column))
        }
        other => invalid_syntax(format!("格式化失败：{other}")),
    }
}

fn detect(input: &str) -> MarkupLanguage {
    let lower = input.to_ascii_lowercase();
    if lower.contains("<!doctype html") || lower.contains("<html") {
        MarkupLanguage::Html
    } else {
        MarkupLanguage::Xml
    }
}

fn minify(input: &str, language: MarkupLanguage) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut index = 0usize;
    while index < input.len() {
        if input.as_bytes()[index] == b'<' {
            if input[index..].starts_with("<!--")
                && let Some(offset) = input[index..].find("-->")
            {
                let end = index + offset + 3;
                out.push_str(&input[index..end]);
                index = end;
                continue;
            }
            if let Some(end) = find_tag_end(input, index) {
                let tag = &input[index..=end];
                if language == MarkupLanguage::Html
                    && let Some(name) = protected_element(tag)
                {
                    let close_token = format!("</{name}");
                    if let Some(offset) = lower[index..].find(&close_token) {
                        let close_start = index + offset;
                        if close_start > end
                            && let Some(close_end) = find_tag_end(input, close_start)
                        {
                            out.push_str(&input[index..close_end + 1]);
                            index = close_end + 1;
                            continue;
                        }
                    }
                }
                out.push_str(tag);
                index = end + 1;
                continue;
            }
        }
        let next = input[index..]
            .find('<')
            .map(|offset| index + offset)
            .unwrap_or(input.len());
        let text = collapse_whitespace(&input[index..next]);
        out.push_str(&text);
        index = next;
    }
    out
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn find_tag_end(input: &str, start: usize) -> Option<usize> {
    let bytes = input.as_bytes();
    let mut quote: Option<u8> = None;
    let mut index = start + 1;
    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(open) if byte == open => quote = None,
            Some(_) => {}
            None => match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'>' => return Some(index),
                _ => {}
            },
        }
        index += 1;
    }
    None
}

fn protected_element(tag: &str) -> Option<&'static str> {
    for name in ["script", "style", "textarea", "pre"] {
        let prefix = format!("<{name}");
        if tag.len() > prefix.len() && tag[..prefix.len()].eq_ignore_ascii_case(&prefix) {
            let next = tag.as_bytes()[prefix.len()];
            if next.is_ascii_whitespace() || matches!(next, b'>' | b'/') {
                return Some(name);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use devtools_core::ToolErrorKind;

    fn request(input: &str) -> XmlHtmlFormatterRequest {
        XmlHtmlFormatterRequest {
            input: input.to_string(),
            language: MarkupLanguage::Auto,
            pretty: true,
            indent: MarkupIndent::Two,
        }
    }

    #[test]
    fn html_is_detected_and_pretty_printed() {
        let resp = run(&request(
            "<!DOCTYPE html><html><body><div><p>hello</p></div></body></html>",
        ))
        .unwrap();
        assert_eq!(resp.detected, MarkupLanguage::Html);
        assert!(resp.output.contains('\n'), "{}", resp.output);
        assert!(resp.output.contains("<p>hello</p>"), "{}", resp.output);
    }

    #[test]
    fn xml_pretty_print_and_indent_four() {
        let mut req = request("<root><a>1</a><b>2</b></root>");
        req.language = MarkupLanguage::Xml;
        req.indent = MarkupIndent::Four;
        let resp = run(&req).unwrap();
        assert_eq!(resp.detected, MarkupLanguage::Xml);
        assert!(resp.output.contains('\n'), "{}", resp.output);
        assert!(
            resp.output.lines().any(|line| line.starts_with("    <")),
            "{}",
            resp.output
        );
    }

    #[test]
    fn tab_indent_is_applied() {
        let mut req = request("<root><a>1</a><b>2</b><c>3</c></root>");
        req.language = MarkupLanguage::Xml;
        req.indent = MarkupIndent::Tab;
        let resp = run(&req).unwrap();
        assert!(resp.output.contains('\t'), "{}", resp.output);
    }

    #[test]
    fn unclosed_tag_is_invalid_syntax_with_span() {
        let err = run(&request("<div><p>hello</div>")).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
        assert!(err.span.is_some());
    }

    #[test]
    fn minify_removes_inter_tag_whitespace() {
        let mut req = request("<div>\n  <p> hello   world </p>\n</div>");
        req.language = MarkupLanguage::Xml;
        req.pretty = false;
        let resp = run(&req).unwrap();
        assert_eq!(resp.output, "<div><p>hello world</p></div>");
    }

    #[test]
    fn minify_preserves_script_contents() {
        let mut req = request("<div>\n<script>if (a < b) {  x(); }</script>\n</div>");
        req.language = MarkupLanguage::Html;
        req.pretty = false;
        let resp = run(&req).unwrap();
        assert!(
            resp.output
                .contains("<script>if (a < b) {  x(); }</script>"),
            "{}",
            resp.output
        );
    }

    #[test]
    fn minify_keeps_comments() {
        let mut req = request("<div>\n<!-- keep > me -->\n<span>x</span>\n</div>");
        req.pretty = false;
        let resp = run(&req).unwrap();
        assert!(
            resp.output.contains("<!-- keep > me -->"),
            "{}",
            resp.output
        );
    }
}
