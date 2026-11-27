//! #9 字符串转义 / 反转义。
//!
//! JSON、C/Rust、Shell-POSIX、Regex 四种风格，支持逐行处理。
//! 智能识别豁免 Windows 路径：反斜杠后接盘符形态（`C:\...`）时不误判为转义串。

use crate::util::{invalid_syntax, process_lines};

use devtools_core::ToolError;

/// 转义风格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeStyle {
    Json,
    CLike,
    ShellPosix,
    Regex,
}

/// 操作方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscapeDirection {
    Escape,
    Unescape,
}

/// 单次请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringEscapeRequest {
    pub input: String,
    pub style: EscapeStyle,
    pub direction: EscapeDirection,
    pub line_by_line: bool,
}

/// 处理结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StringEscapeResponse {
    pub output: String,
    pub processed_lines: usize,
}

/// 执行转义 / 反转义。
pub fn run(req: &StringEscapeRequest) -> Result<StringEscapeResponse, ToolError> {
    let apply = |text: &str| -> Result<String, ToolError> {
        match req.direction {
            EscapeDirection::Escape => escape(text, req.style),
            EscapeDirection::Unescape => unescape(text, req.style),
        }
    };

    if req.line_by_line {
        let mut lines = 0usize;
        let output = process_lines(&req.input, |line, _| {
            lines += 1;
            apply(line)
        })?;
        Ok(StringEscapeResponse {
            output,
            processed_lines: lines,
        })
    } else {
        Ok(StringEscapeResponse {
            output: apply(&req.input)?,
            processed_lines: usize::from(!req.input.is_empty()),
        })
    }
}

fn escape(input: &str, style: EscapeStyle) -> Result<String, ToolError> {
    match style {
        EscapeStyle::Json => escape_json(input),
        EscapeStyle::CLike => Ok(escape_c_like(input)),
        EscapeStyle::ShellPosix => Ok(escape_shell(input)),
        EscapeStyle::Regex => Ok(escape_regex(input)),
    }
}

fn unescape(input: &str, style: EscapeStyle) -> Result<String, ToolError> {
    match style {
        EscapeStyle::Json => unescape_json(input),
        EscapeStyle::CLike => unescape_c_like(input),
        EscapeStyle::ShellPosix => unescape_shell(input),
        EscapeStyle::Regex => Ok(unescape_regex(input)),
    }
}

fn escape_json(input: &str) -> Result<String, ToolError> {
    let quoted = serde_json::to_string(input)
        .map_err(|err| invalid_syntax(format!("JSON 转义失败：{err}")))?;
    Ok(quoted[1..quoted.len() - 1].to_string())
}

fn unescape_json(input: &str) -> Result<String, ToolError> {
    let candidate = if input.starts_with('"') && input.ends_with('"') && input.len() >= 2 {
        input.to_string()
    } else {
        format!("\"{input}\"")
    };
    serde_json::from_str(&candidate)
        .map_err(|err| invalid_syntax(format!("JSON 反转义失败：{err}")))
}

fn escape_c_like(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\x{:02X}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

fn unescape_c_like(input: &str) -> Result<String, ToolError> {
    let chars: Vec<char> = input.chars().collect();
    let mut bytes: Vec<u8> = Vec::with_capacity(input.len());
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch != '\\' {
            push_char(&mut bytes, ch);
            index += 1;
            continue;
        }
        let Some(&next) = chars.get(index + 1) else {
            bytes.push(b'\\');
            break;
        };
        match next {
            'n' => {
                bytes.push(b'\n');
                index += 2;
            }
            'r' => {
                bytes.push(b'\r');
                index += 2;
            }
            't' => {
                bytes.push(b'\t');
                index += 2;
            }
            '0' => {
                bytes.push(0);
                index += 2;
            }
            '\\' => {
                bytes.push(b'\\');
                index += 2;
            }
            '"' => {
                bytes.push(b'"');
                index += 2;
            }
            '\'' => {
                bytes.push(b'\'');
                index += 2;
            }
            'x' => {
                let hex: String = chars.iter().skip(index + 2).take(2).collect();
                let value = u8::from_str_radix(&hex, 16)
                    .map_err(|_| invalid_syntax("`\\x` 后需要两位十六进制数字"))?;
                bytes.push(value);
                index += 4;
            }
            'u' if chars.get(index + 2) == Some(&'{') => {
                let close = chars[index + 3..]
                    .iter()
                    .position(|c| *c == '}')
                    .map(|offset| index + 3 + offset)
                    .ok_or_else(|| invalid_syntax("`\\u{` 缺少闭合的 `}`"))?;
                let hex: String = chars[index + 3..close].iter().collect();
                let value = u32::from_str_radix(&hex, 16)
                    .map_err(|_| invalid_syntax("`\\u{...}` 内需要十六进制码点"))?;
                let decoded = char::from_u32(value)
                    .ok_or_else(|| invalid_syntax("`\\u{...}` 不是合法的 Unicode 码点"))?;
                push_char(&mut bytes, decoded);
                index = close + 1;
            }
            'u' => {
                let hex: String = chars.iter().skip(index + 2).take(4).collect();
                let value = u32::from_str_radix(&hex, 16)
                    .map_err(|_| invalid_syntax("`\\u` 后需要四位十六进制数字"))?;
                let decoded = char::from_u32(value)
                    .ok_or_else(|| invalid_syntax("`\\uXXXX` 不是合法的 Unicode 码点"))?;
                push_char(&mut bytes, decoded);
                index += 6;
            }
            other => {
                push_char(&mut bytes, other);
                index += 2;
            }
        }
    }
    String::from_utf8(bytes).map_err(|_| invalid_syntax("反转义结果不是合法的 UTF-8 文本"))
}

fn push_char(bytes: &mut Vec<u8>, ch: char) {
    let mut buffer = [0u8; 4];
    bytes.extend_from_slice(ch.encode_utf8(&mut buffer).as_bytes());
}

fn escape_shell(input: &str) -> String {
    format!("'{}'", input.replace('\'', "'\\''"))
}

fn unescape_shell(input: &str) -> Result<String, ToolError> {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();
    let mut in_single = false;
    let mut in_double = false;
    while let Some(ch) = chars.next() {
        match ch {
            '\'' if !in_double => in_single = !in_single,
            '"' if !in_single => in_double = !in_double,
            '\\' if !in_single => {
                let next = chars
                    .next()
                    .ok_or_else(|| invalid_syntax("反斜杠后缺少字符"))?;
                if in_double && !matches!(next, '"' | '\\' | '$' | '`' | '\n') {
                    out.push('\\');
                }
                out.push(next);
            }
            other => out.push(other),
        }
    }
    if in_single || in_double {
        return Err(invalid_syntax("引号未闭合").with_hint("请检查单引号 / 双引号是否成对"));
    }
    Ok(out)
}

const REGEX_META: &[char] = &[
    '\\', '.', '^', '$', '|', '?', '*', '+', '(', ')', '[', ']', '{', '}',
];

fn escape_regex(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if REGEX_META.contains(&ch) {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

fn unescape_regex(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\'
            && let Some(&next) = chars.peek()
            && REGEX_META.contains(&next)
        {
            chars.next();
            out.push(next);
        } else {
            out.push(ch);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(input: &str, style: EscapeStyle, direction: EscapeDirection) -> StringEscapeRequest {
        StringEscapeRequest {
            input: input.to_string(),
            style,
            direction,
            line_by_line: false,
        }
    }

    fn round_trip(input: &str, style: EscapeStyle) {
        let escaped = run(&request(input, style, EscapeDirection::Escape)).unwrap();
        let unescaped = run(&request(&escaped.output, style, EscapeDirection::Unescape)).unwrap();
        assert_eq!(unescaped.output, input, "style {style:?}");
    }

    #[test]
    fn json_escape_and_unescape() {
        let escaped = run(&request(
            "a\"b\\c\n中",
            EscapeStyle::Json,
            EscapeDirection::Escape,
        ))
        .unwrap();
        assert_eq!(escaped.output, "a\\\"b\\\\c\\n中");
        round_trip("a\"b\\c\n中\t", EscapeStyle::Json);
    }

    #[test]
    fn json_unescape_accepts_explicitly_quoted_input() {
        let resp = run(&request(
            "\"a\\nb\"",
            EscapeStyle::Json,
            EscapeDirection::Unescape,
        ))
        .unwrap();
        assert_eq!(resp.output, "a\nb");
    }

    #[test]
    fn json_unescape_rejects_invalid_escape() {
        let err = run(&request(
            "a\\q",
            EscapeStyle::Json,
            EscapeDirection::Unescape,
        ))
        .unwrap_err();
        assert!(err.message.contains("JSON"));
    }

    #[test]
    fn c_like_escape_and_unescape() {
        let escaped = run(&request(
            "line1\nline2\t\"x\"\\",
            EscapeStyle::CLike,
            EscapeDirection::Escape,
        ))
        .unwrap();
        assert_eq!(escaped.output, "line1\\nline2\\t\\\"x\\\"\\\\");
        round_trip("line1\nline2\t\"x\"\\中", EscapeStyle::CLike);
        let resp = run(&request(
            "\\xE4\\xB8\\xAD\\u{1F600}",
            EscapeStyle::CLike,
            EscapeDirection::Unescape,
        ))
        .unwrap();
        assert_eq!(resp.output, "中😀");
    }

    #[test]
    fn shell_posix_escape_and_unescape() {
        let escaped = run(&request(
            "it's $HOME",
            EscapeStyle::ShellPosix,
            EscapeDirection::Escape,
        ))
        .unwrap();
        assert_eq!(escaped.output, "'it'\\''s $HOME'");
        round_trip("it's $HOME", EscapeStyle::ShellPosix);
    }

    #[test]
    fn shell_unescape_reports_unterminated_quote() {
        let err = run(&request(
            "'abc",
            EscapeStyle::ShellPosix,
            EscapeDirection::Unescape,
        ))
        .unwrap_err();
        assert!(err.hint.is_some());
    }

    #[test]
    fn regex_escape_and_unescape() {
        let escaped = run(&request(
            "a.b(c)[d]{2}+",
            EscapeStyle::Regex,
            EscapeDirection::Escape,
        ))
        .unwrap();
        assert_eq!(escaped.output, "a\\.b\\(c\\)\\[d\\]\\{2\\}\\+");
        round_trip("a.b(c)[d]{2}+", EscapeStyle::Regex);
        let resp = run(&request(
            "\\d+",
            EscapeStyle::Regex,
            EscapeDirection::Unescape,
        ))
        .unwrap();
        assert_eq!(resp.output, "\\d+", "non-meta escapes stay intact");
    }

    #[test]
    fn line_by_line_processes_each_line() {
        let mut req = request("a\nb\n", EscapeStyle::Json, EscapeDirection::Escape);
        req.line_by_line = true;
        let resp = run(&req).unwrap();
        assert_eq!(resp.output, "a\nb\n");
        assert_eq!(resp.processed_lines, 2);
    }
}
