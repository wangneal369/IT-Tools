//! #8 HTML 实体与 Unicode 转义。
//!
//! 编码支持 HTML 实体、`\uXXXX`、CSS `\XXXXXX`、`U+XXXX` 四种风格；
//! 解码统一识别全部四种写法（含 UTF-16 代理对与 CSS 的空白终止符），
//! 并可仅处理非 ASCII 字符。

use devtools_core::ToolError;

/// 操作方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HtmlUnicodeMode {
    Encode,
    Decode,
}

/// 非 ASCII / 特殊字符的编码风格。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnicodeEscapeStyle {
    /// HTML 实体：`&amp;` / `&#x4E2D;`。
    HtmlEntity,
    /// JSON 风格：`中`（代理对处理增补平面）。
    JsonUnicode,
    /// CSS 风格：`\4E2D`（固定 6 位 + 空白终止符）。
    CssEscape,
    /// 码点记法：`U+4E2D`。
    Codepoint,
}

/// 单次请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlEntityUnicodeRequest {
    pub input: String,
    pub mode: HtmlUnicodeMode,
    pub style: UnicodeEscapeStyle,
    /// 仅转换非 ASCII 字符，ASCII（含 `<`、`&`）保持原样。
    pub non_ascii_only: bool,
}

/// 处理结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HtmlEntityUnicodeResponse {
    pub output: String,
    /// 实际替换次数。
    pub substitutions: usize,
}

/// 执行 HTML 实体 / Unicode 转义。
pub fn run(req: &HtmlEntityUnicodeRequest) -> Result<HtmlEntityUnicodeResponse, ToolError> {
    match req.mode {
        HtmlUnicodeMode::Encode => Ok(encode(&req.input, req.style, req.non_ascii_only)),
        HtmlUnicodeMode::Decode => Ok(decode(&req.input)),
    }
}

fn encode(
    input: &str,
    style: UnicodeEscapeStyle,
    non_ascii_only: bool,
) -> HtmlEntityUnicodeResponse {
    let mut out = String::with_capacity(input.len());
    let mut substitutions = 0usize;
    for ch in input.chars() {
        if non_ascii_only && ch.is_ascii() {
            out.push(ch);
            continue;
        }
        let replaced = match style {
            UnicodeEscapeStyle::HtmlEntity => encode_html_char(ch, non_ascii_only),
            UnicodeEscapeStyle::JsonUnicode => encode_json_char(ch, non_ascii_only),
            UnicodeEscapeStyle::CssEscape => encode_css_char(ch, non_ascii_only),
            UnicodeEscapeStyle::Codepoint => {
                if non_ascii_only {
                    (!ch.is_ascii()).then(|| format!("U+{:04X}", ch as u32))
                } else if out.is_empty() {
                    Some(format!("U+{:04X}", ch as u32))
                } else {
                    Some(format!(" U+{:04X}", ch as u32))
                }
            }
        };
        match replaced {
            Some(text) => {
                substitutions += 1;
                out.push_str(&text);
            }
            None => out.push(ch),
        }
    }
    HtmlEntityUnicodeResponse {
        output: out,
        substitutions,
    }
}

fn encode_html_char(ch: char, non_ascii_only: bool) -> Option<String> {
    if !ch.is_ascii() {
        return Some(format!("&#x{:X};", ch as u32));
    }
    if non_ascii_only {
        return None;
    }
    match ch {
        '&' => Some("&amp;".to_string()),
        '<' => Some("&lt;".to_string()),
        '>' => Some("&gt;".to_string()),
        '"' => Some("&quot;".to_string()),
        '\'' => Some("&#39;".to_string()),
        _ => None,
    }
}

fn encode_json_char(ch: char, non_ascii_only: bool) -> Option<String> {
    if !ch.is_ascii() {
        let value = ch as u32;
        if value <= 0xFFFF {
            return Some(format!("\\u{value:04X}"));
        }
        let adjusted = value - 0x1_0000;
        let high = 0xD800 + (adjusted >> 10);
        let low = 0xDC00 + (adjusted & 0x3FF);
        return Some(format!("\\u{high:04X}\\u{low:04X}"));
    }
    if non_ascii_only {
        return None;
    }
    match ch {
        '\\' => Some("\\\\".to_string()),
        '"' => Some("\\\"".to_string()),
        '\n' => Some("\\n".to_string()),
        '\r' => Some("\\r".to_string()),
        '\t' => Some("\\t".to_string()),
        c if c.is_control() => Some(format!("\\u{:04X}", c as u32)),
        _ => None,
    }
}

fn encode_css_char(ch: char, non_ascii_only: bool) -> Option<String> {
    if ch.is_ascii() {
        if non_ascii_only {
            return None;
        }
        return match ch {
            '\\' => Some("\\5C ".to_string()),
            c if c.is_control() => Some(format!("\\{:06X} ", c as u32)),
            _ => None,
        };
    }
    Some(format!("\\{:06X} ", ch as u32))
}

fn decode(input: &str) -> HtmlEntityUnicodeResponse {
    let after_html = html_escape::decode_html_entities(input).into_owned();
    let chars: Vec<char> = after_html.chars().collect();
    let mut out = String::with_capacity(after_html.len());
    let mut substitutions = 0usize;
    let mut index = 0usize;

    while index < chars.len() {
        let ch = chars[index];
        if ch == '\\' {
            if let Some((text, consumed)) = decode_backslash(&chars, index) {
                out.push_str(&text);
                substitutions += 1;
                index += consumed;
                continue;
            }
        } else if (ch == 'U' || ch == 'u')
            && chars.get(index + 1) == Some(&'+')
            && let Some((text, consumed)) = decode_codepoint(&chars, index)
        {
            out.push_str(&text);
            substitutions += 1;
            index += consumed;
            continue;
        }
        out.push(ch);
        index += 1;
    }
    HtmlEntityUnicodeResponse {
        output: out,
        substitutions,
    }
}

/// 解析 `\uXXXX`、`\u{...}` 与 CSS `\XXXXXX`。
fn decode_backslash(chars: &[char], start: usize) -> Option<(String, usize)> {
    let next = *chars.get(start + 1)?;
    if next == 'u' {
        if chars.get(start + 2) == Some(&'{') {
            let mut end = start + 3;
            while end < chars.len() && chars[end] != '}' {
                end += 1;
            }
            if end >= chars.len() || end == start + 3 {
                return None;
            }
            let hex: String = chars[start + 3..end].iter().collect();
            let value = u32::from_str_radix(&hex, 16).ok()?;
            let ch = char::from_u32(value)?;
            return Some((ch.to_string(), end - start + 1));
        }
        let value = read_hex(chars, start + 2, 4)?.0;
        if (0xD800..=0xDBFF).contains(&value) {
            // UTF-16 高代理，尝试与紧随其后的低代理组合。
            if chars.get(start + 6) == Some(&'\\')
                && chars.get(start + 7) == Some(&'u')
                && let Some((low, _)) = read_hex(chars, start + 8, 4)
                && (0xDC00..=0xDFFF).contains(&low)
            {
                let combined = 0x1_0000 + ((value - 0xD800) << 10) + (low - 0xDC00);
                if let Some(ch) = char::from_u32(combined) {
                    return Some((ch.to_string(), 12));
                }
            }
            return None;
        }
        let ch = char::from_u32(value)?;
        return Some((ch.to_string(), 6));
    }
    if next.is_ascii_hexdigit() {
        let (value, digits) = read_hex(chars, start + 1, 6)?;
        let mut consumed = 1 + digits;
        // CSS 允许且仅消耗一个空白字符作为终止符。
        if chars
            .get(start + consumed)
            .is_some_and(|c| c.is_whitespace())
        {
            consumed += 1;
        }
        let ch = char::from_u32(value)?;
        return Some((ch.to_string(), consumed));
    }
    None
}

/// 解析 `U+XXXX`（最多 6 位十六进制，超出 Unicode 范围时回退到更短位数）。
fn decode_codepoint(chars: &[char], start: usize) -> Option<(String, usize)> {
    let hex_start = start + 2;
    let available = chars.len().saturating_sub(hex_start);
    for digits in (4..=6).rev() {
        if digits > available {
            continue;
        }
        let slice = &chars[hex_start..hex_start + digits];
        if !slice.iter().all(char::is_ascii_hexdigit) {
            continue;
        }
        let hex: String = slice.iter().collect();
        if let Ok(value) = u32::from_str_radix(&hex, 16)
            && let Some(ch) = char::from_u32(value)
        {
            return Some((ch.to_string(), 2 + digits));
        }
    }
    None
}

fn read_hex(chars: &[char], start: usize, max: usize) -> Option<(u32, usize)> {
    let mut value = 0u32;
    let mut digits = 0usize;
    while digits < max {
        let Some(ch) = chars.get(start + digits) else {
            break;
        };
        let Some(digit) = ch.to_digit(16) else {
            break;
        };
        if value > 0x10FFFF >> 4 {
            break;
        }
        value = value * 16 + digit;
        digits += 1;
    }
    (digits > 0).then_some((value, digits))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_req(
        input: &str,
        style: UnicodeEscapeStyle,
        non_ascii_only: bool,
    ) -> HtmlEntityUnicodeResponse {
        run(&HtmlEntityUnicodeRequest {
            input: input.to_string(),
            mode: HtmlUnicodeMode::Encode,
            style,
            non_ascii_only,
        })
        .unwrap()
    }

    fn decode_req(input: &str) -> HtmlEntityUnicodeResponse {
        run(&HtmlEntityUnicodeRequest {
            input: input.to_string(),
            mode: HtmlUnicodeMode::Decode,
            style: UnicodeEscapeStyle::HtmlEntity,
            non_ascii_only: false,
        })
        .unwrap()
    }

    #[test]
    fn html_entity_encoding_non_ascii_only() {
        let resp = encode_req("中文<>", UnicodeEscapeStyle::HtmlEntity, true);
        assert_eq!(resp.output, "&#x4E2D;&#x6587;<>");
        assert_eq!(resp.substitutions, 2);
    }

    #[test]
    fn html_entity_encoding_escapes_ascii_specials() {
        let resp = encode_req("a&b<c>\"'中", UnicodeEscapeStyle::HtmlEntity, false);
        assert_eq!(resp.output, "a&amp;b&lt;c&gt;&quot;&#39;&#x4E2D;");
    }

    #[test]
    fn html_entities_decode() {
        assert_eq!(decode_req("&lt;b&gt; &amp; &#x4E2D;").output, "<b> & 中");
    }

    #[test]
    fn json_unicode_escapes_and_decodes() {
        let resp = encode_req("中\"\n", UnicodeEscapeStyle::JsonUnicode, false);
        assert_eq!(resp.output, "\\u4E2D\\\"\\n");
        assert_eq!(decode_req("\\u4E2D\\u6587").output, "中文");
    }

    #[test]
    fn surrogate_pairs_round_trip() {
        let resp = encode_req("😀", UnicodeEscapeStyle::JsonUnicode, false);
        assert_eq!(resp.output, "\\uD83D\\uDE00");
        assert_eq!(decode_req("\\uD83D\\uDE00").output, "😀");
    }

    #[test]
    fn css_escape_uses_six_digits_and_whitespace_terminator() {
        let resp = encode_req("中文", UnicodeEscapeStyle::CssEscape, true);
        assert_eq!(resp.output, "\\004E2D \\006587 ");
        assert_eq!(decode_req("\\004E2D \\006587 ").output, "中文");
    }

    #[test]
    fn codepoint_notation_decodes() {
        assert_eq!(decode_req("U+4E2D").output, "中");
        assert_eq!(decode_req("U+1F600").output, "😀");
        assert_eq!(decode_req("U+0061 U+0062").output, "a b");
    }

    #[test]
    fn codepoint_encoding_non_ascii_only() {
        let resp = encode_req("a中b", UnicodeEscapeStyle::Codepoint, true);
        assert_eq!(resp.output, "aU+4E2Db");
    }

    #[test]
    fn mixed_forms_decode_together() {
        let resp = decode_req("&lt;\\u4E2D\\4E2D U+4E2D&gt;");
        assert_eq!(resp.output, "<中中中>");
        assert_eq!(resp.substitutions, 3);
    }

    #[test]
    fn invalid_codepoint_stays_literal() {
        assert_eq!(decode_req("U+XYZ").output, "U+XYZ");
        assert_eq!(decode_req("\\uZZZZ").output, "\\uZZZZ");
    }
}
