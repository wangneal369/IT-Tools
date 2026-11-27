//! Syntax highlighting language mapping, large-text degradation guardrails
//! (ADR-0001), and a lightweight pure-Rust SQL highlighter.
//!
//! Seven languages (`json`, `yaml`, `toml`, `html`/XML, `diff`, `markdown`,
//! `css`) use `gpui-kit`'s native `tree-sitter` grammars (~1.8 MiB total). SQL
//! uses a dedicated pure-Rust `InputHighlighter` instead of `tree-sitter-sequel`
//! (whose C parse table alone adds ~13 MiB) so all three release platforms stay
//! comfortably beneath the 45 MiB binary budget without sacrificing SQL coloring.

use std::ops::Range;
use std::rc::Rc;

use gpui_kit::base::input::{
    EditorState, FoldRange, HighlightStyleResolver, InputEdit, InputHighlighter,
    InputHighlighterFactory, Rope,
};
use gpui_kit::{Context, HighlightStyle, SharedString, Window};

/// Hard total-size limit for syntax highlighting (1 MiB, ADR-0001).
pub const HIGHLIGHT_MAX_BYTES: usize = 1024 * 1024;

/// Hard single-line limit for syntax highlighting (50 KiB, ADR-0001) to avoid
/// sluggish parsing or shaping on huge minified single-line blobs.
pub const HIGHLIGHT_MAX_LINE_BYTES: usize = 50 * 1024;

/// Curated V1 syntax languages supported across DevTools editors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyntaxLang {
    Json,
    Yaml,
    Toml,
    Sql,
    Html,
    /// XML uses the HTML tree-sitter grammar for tags, attributes, strings and
    /// comments while displaying an `XML` badge.
    Xml,
    /// Auto-detected XML/HTML displays `XML / HTML`.
    XmlHtml,
    Diff,
    Markdown,
    Css,
}

impl SyntaxLang {
    pub const ALL: [Self; 10] = [
        Self::Json,
        Self::Yaml,
        Self::Toml,
        Self::Sql,
        Self::Html,
        Self::Xml,
        Self::XmlHtml,
        Self::Diff,
        Self::Markdown,
        Self::Css,
    ];

    /// Canonical language identifier passed to `EditorState::set_highlighter`.
    pub const fn highlighter_name(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Yaml => "yaml",
            Self::Toml => "toml",
            Self::Sql => "sql",
            Self::Html | Self::Xml | Self::XmlHtml => "html",
            Self::Diff => "diff",
            Self::Markdown => "markdown",
            Self::Css => "css",
        }
    }

    /// True when backed by `gpui-kit`'s bundled `tree-sitter` registry.
    pub const fn uses_tree_sitter(self) -> bool {
        !matches!(self, Self::Sql)
    }

    /// Short user-facing badge shown in input/output pane headers.
    pub const fn badge_label(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Yaml => "YAML",
            Self::Toml => "TOML",
            Self::Sql => "SQL",
            Self::Html => "HTML",
            Self::Xml => "XML",
            Self::XmlHtml => "XML / HTML",
            Self::Diff => "Diff",
            Self::Markdown => "Markdown",
            Self::Css => "CSS",
        }
    }
}

/// Resolved highlighting configuration for a concrete editor buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntaxSpec {
    /// Language passed to `EditorState::set_highlighter` (`"text"` when plain or degraded).
    pub highlighter: &'static str,
    /// Short language badge for the pane header, if the slot is structured code.
    pub badge: Option<&'static str>,
    /// True when a structured language was degraded to plain text for large-text safety.
    pub degraded: bool,
}

/// Returns `true` when `text` exceeds ADR-0001's 1 MiB total or 50 KiB single-line
/// syntax-highlighting safety threshold.
pub fn should_degrade_to_plain(text: &str) -> bool {
    if text.len() > HIGHLIGHT_MAX_BYTES {
        return true;
    }
    text.as_bytes()
        .split(|&b| b == b'\n')
        .any(|line| line.len() > HIGHLIGHT_MAX_LINE_BYTES)
}

/// Resolve the active highlighter language and header badge for `text`.
pub fn resolve_syntax(lang: Option<SyntaxLang>, text: &str) -> SyntaxSpec {
    match lang {
        None => SyntaxSpec {
            highlighter: "text",
            badge: None,
            degraded: false,
        },
        Some(lang) if should_degrade_to_plain(text) => SyntaxSpec {
            highlighter: "text",
            badge: Some(lang.badge_label()),
            degraded: true,
        },
        Some(lang) => SyntaxSpec {
            highlighter: lang.highlighter_name(),
            badge: Some(lang.badge_label()),
            degraded: false,
        },
    }
}

/// Highlighter factory installed on `SqlFormatter` editors: serves the
/// lightweight pure-Rust SQL highlighter for `"sql"` and returns `None` for
/// `"text"` (large-text degradation).
pub fn sql_highlighter_factory() -> InputHighlighterFactory {
    Rc::new(|language| {
        (language == "sql")
            .then(|| Box::new(SqlInputHighlighter::default()) as Box<dyn InputHighlighter>)
    })
}

#[derive(Default)]
struct SqlInputHighlighter {
    tokens: Vec<(Range<usize>, &'static str)>,
}

impl SqlInputHighlighter {
    fn reparse(&mut self, source: &str) {
        self.tokens = tokenize_sql(source);
    }

    fn resolve_range(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        if range.start >= range.end {
            return Vec::new();
        }
        let mut runs = Vec::new();
        let mut cursor = range.start;
        let idx = self
            .tokens
            .partition_point(|(tok_range, _)| tok_range.end <= range.start);

        for (tok_range, scope) in &self.tokens[idx..] {
            if tok_range.start >= range.end {
                break;
            }
            let start = tok_range.start.max(range.start);
            let end = tok_range.end.min(range.end);
            if start >= end {
                continue;
            }
            if start > cursor {
                runs.push((cursor..start, HighlightStyle::default()));
            }
            let style = resolver.style(scope).unwrap_or_default();
            runs.push((start..end, style));
            cursor = end;
        }
        if cursor < range.end {
            runs.push((cursor..range.end, HighlightStyle::default()));
        }
        runs
    }
}

impl InputHighlighter for SqlInputHighlighter {
    fn language(&self) -> SharedString {
        SharedString::new_static("sql")
    }

    fn update(
        &mut self,
        _: Option<InputEdit>,
        text: &Rope,
        _: bool,
        _: &mut Window,
        _: &mut Context<EditorState>,
    ) {
        self.reparse(&text.to_string());
    }

    fn update_batch(
        &mut self,
        edits: &[(InputEdit, Rope)],
        _: bool,
        _: &mut Window,
        _: &mut Context<EditorState>,
    ) {
        if let Some((_, text)) = edits.last() {
            self.reparse(&text.to_string());
        }
    }

    fn styles(
        &self,
        range: &Range<usize>,
        resolver: &dyn HighlightStyleResolver,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        self.resolve_range(range, resolver)
    }

    fn fold_ranges(&self, _: &Rope) -> Vec<FoldRange> {
        Vec::new()
    }
}

fn tokenize_sql(source: &str) -> Vec<(Range<usize>, &'static str)> {
    let bytes = source.as_bytes();
    let len = bytes.len();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        // Line comment: `-- ...`
        if b == b'-' && i + 1 < len && bytes[i + 1] == b'-' {
            let start = i;
            i += 2;
            while i < len && bytes[i] != b'\n' {
                i += 1;
            }
            tokens.push((start..i, "comment"));
            continue;
        }

        // Block comment: `/* ... */`
        if b == b'/' && i + 1 < len && bytes[i + 1] == b'*' {
            let start = i;
            i += 2;
            while i + 1 < len && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                i += 1;
            }
            i = if i + 1 < len { i + 2 } else { len };
            tokens.push((start..i, "comment"));
            continue;
        }

        // Single-quoted SQL string literal (`''` escapes)
        if b == b'\'' {
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == b'\'' {
                    i += 1;
                    if i < len && bytes[i] == b'\'' {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += utf8_step(source, i);
            }
            tokens.push((start..i, "string"));
            continue;
        }

        // Quoted identifier: `"..."` or `` `...` ``
        if b == b'"' || b == b'`' {
            let quote = b;
            let start = i;
            i += 1;
            while i < len {
                if bytes[i] == quote {
                    i += 1;
                    if i < len && bytes[i] == quote {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += utf8_step(source, i);
            }
            tokens.push((start..i, "property"));
            continue;
        }

        // Numbers: `123`, `3.14`, `0xFF`
        if b.is_ascii_digit() {
            let start = i;
            if b == b'0' && i + 1 < len && matches!(bytes[i + 1], b'x' | b'X') {
                i += 2;
                while i < len && bytes[i].is_ascii_hexdigit() {
                    i += 1;
                }
            } else {
                while i < len && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                if i + 1 < len && bytes[i] == b'.' && bytes[i + 1].is_ascii_digit() {
                    i += 1;
                    while i < len && bytes[i].is_ascii_digit() {
                        i += 1;
                    }
                }
            }
            tokens.push((start..i, "number"));
            continue;
        }

        // Bind parameters: `:name`, `@name`, `$1`, `?`
        if matches!(b, b':' | b'@' | b'$')
            && i + 1 < len
            && (bytes[i + 1].is_ascii_alphanumeric() || bytes[i + 1] == b'_')
            && !(b == b':' && i > 0 && bytes[i - 1] == b':')
        {
            let start = i;
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            tokens.push((start..i, "variable.special"));
            continue;
        }
        if b == b'?' {
            tokens.push((i..i + 1, "variable.special"));
            i += 1;
            continue;
        }

        // Identifier / keyword / type / function
        if b.is_ascii_alphabetic() || b == b'_' {
            let start = i;
            i += 1;
            while i < len && (bytes[i].is_ascii_alphanumeric() || matches!(bytes[i], b'_' | b'$')) {
                i += 1;
            }
            let word = &source[start..i];
            let mut lookahead = i;
            while lookahead < len && matches!(bytes[lookahead], b' ' | b'\t') {
                lookahead += 1;
            }
            let followed_by_paren = lookahead < len && bytes[lookahead] == b'(';
            if let Some(scope) = classify_sql_word(word, followed_by_paren) {
                tokens.push((start..i, scope));
            }
            continue;
        }

        // Brackets / delimiters / operators
        if matches!(b, b'(' | b')' | b'[' | b']') {
            tokens.push((i..i + 1, "punctuation.bracket"));
            i += 1;
            continue;
        }
        if matches!(b, b',' | b';' | b'.') {
            tokens.push((i..i + 1, "punctuation.delimiter"));
            i += 1;
            continue;
        }
        if matches!(
            b,
            b'=' | b'<'
                | b'>'
                | b'!'
                | b'+'
                | b'-'
                | b'*'
                | b'/'
                | b'%'
                | b'|'
                | b'&'
                | b'~'
                | b'^'
                | b':'
        ) {
            let start = i;
            i += 1;
            while i < len && matches!(bytes[i], b'=' | b'<' | b'>' | b'|' | b':' | b'&') {
                i += 1;
            }
            tokens.push((start..i, "operator"));
            continue;
        }

        i += utf8_step(source, i);
    }

    tokens
}

#[inline]
fn utf8_step(source: &str, offset: usize) -> usize {
    source[offset..].chars().next().map_or(1, char::len_utf8)
}

fn classify_sql_word(word: &str, followed_by_paren: bool) -> Option<&'static str> {
    if word.len() > 24 {
        return followed_by_paren.then_some("function");
    }
    let mut buf = [0u8; 24];
    for (dst, &src) in buf.iter_mut().zip(word.as_bytes()) {
        *dst = src.to_ascii_uppercase();
    }
    let upper = std::str::from_utf8(&buf[..word.len()]).ok()?;

    if matches!(upper, "TRUE" | "FALSE" | "NULL" | "UNKNOWN") {
        return Some("boolean");
    }
    if is_sql_keyword(upper) {
        return Some("keyword");
    }
    if is_sql_type(upper) {
        return Some("type");
    }
    if followed_by_paren || is_sql_builtin_fn(upper) {
        return Some("function");
    }
    None
}

fn is_sql_keyword(upper: &str) -> bool {
    matches!(
        upper,
        "ADD"
            | "ALL"
            | "ALTER"
            | "ANALYZE"
            | "AND"
            | "AS"
            | "ASC"
            | "AUTO_INCREMENT"
            | "BEGIN"
            | "BETWEEN"
            | "BY"
            | "CASCADE"
            | "CASE"
            | "CHECK"
            | "COLUMN"
            | "COMMIT"
            | "CONFLICT"
            | "CONSTRAINT"
            | "CREATE"
            | "CROSS"
            | "CURRENT"
            | "DATABASE"
            | "DEFAULT"
            | "DELETE"
            | "DESC"
            | "DISTINCT"
            | "DO"
            | "DROP"
            | "ELSE"
            | "END"
            | "EXCEPT"
            | "EXISTS"
            | "EXPLAIN"
            | "FETCH"
            | "FILTER"
            | "FIRST"
            | "FOLLOWING"
            | "FOREIGN"
            | "FROM"
            | "FULL"
            | "FUNCTION"
            | "GRANT"
            | "GROUP"
            | "HAVING"
            | "IF"
            | "ILIKE"
            | "IN"
            | "INDEX"
            | "INNER"
            | "INSERT"
            | "INTERSECT"
            | "INTERVAL"
            | "INTO"
            | "IS"
            | "JOIN"
            | "KEY"
            | "LAST"
            | "LATERAL"
            | "LEFT"
            | "LIKE"
            | "LIMIT"
            | "MERGE"
            | "NATURAL"
            | "NEXT"
            | "NOT"
            | "NOTHING"
            | "NULLS"
            | "OFFSET"
            | "ON"
            | "ONLY"
            | "OR"
            | "ORDER"
            | "OUTER"
            | "OVER"
            | "PARTITION"
            | "PRECEDING"
            | "PRIMARY"
            | "PROCEDURE"
            | "RANGE"
            | "RECURSIVE"
            | "REFERENCES"
            | "RENAME"
            | "REPLACE"
            | "RESTRICT"
            | "RETURNING"
            | "RETURNS"
            | "REVOKE"
            | "RIGHT"
            | "ROLLBACK"
            | "ROW"
            | "ROWS"
            | "SCHEMA"
            | "SELECT"
            | "SET"
            | "SHOW"
            | "TABLE"
            | "TEMP"
            | "TEMPORARY"
            | "THEN"
            | "TO"
            | "TOP"
            | "TRANSACTION"
            | "TRIGGER"
            | "TRUNCATE"
            | "UNBOUNDED"
            | "UNION"
            | "UNIQUE"
            | "UPDATE"
            | "USING"
            | "VALUES"
            | "VIEW"
            | "WHEN"
            | "WHERE"
            | "WINDOW"
            | "WITH"
            | "WITHIN"
    )
}

fn is_sql_type(upper: &str) -> bool {
    matches!(
        upper,
        "ARRAY"
            | "BIGINT"
            | "BINARY"
            | "BLOB"
            | "BOOL"
            | "BOOLEAN"
            | "BYTEA"
            | "CHAR"
            | "CHARACTER"
            | "CLOB"
            | "DATE"
            | "DATETIME"
            | "DECIMAL"
            | "DOUBLE"
            | "FLOAT"
            | "INT"
            | "INT2"
            | "INT4"
            | "INT8"
            | "INTEGER"
            | "JSON"
            | "JSONB"
            | "NCHAR"
            | "NUMERIC"
            | "NVARCHAR"
            | "REAL"
            | "SERIAL"
            | "SMALLINT"
            | "TEXT"
            | "TIME"
            | "TIMESTAMP"
            | "TIMESTAMPTZ"
            | "TINYINT"
            | "UUID"
            | "VARBINARY"
            | "VARCHAR"
            | "XML"
    )
}

fn is_sql_builtin_fn(upper: &str) -> bool {
    matches!(
        upper,
        "ABS"
            | "ARRAY_AGG"
            | "AVG"
            | "CAST"
            | "CEIL"
            | "CEILING"
            | "CHAR_LENGTH"
            | "COALESCE"
            | "CONCAT"
            | "COUNT"
            | "CURRENT_DATE"
            | "CURRENT_TIME"
            | "CURRENT_TIMESTAMP"
            | "DATE_ADD"
            | "DATE_DIFF"
            | "DATE_TRUNC"
            | "DENSE_RANK"
            | "EXTRACT"
            | "FIRST_VALUE"
            | "FLOOR"
            | "GROUP_CONCAT"
            | "IFNULL"
            | "JSON_ARRAY"
            | "JSON_EXTRACT"
            | "JSON_OBJECT"
            | "LAG"
            | "LAST_VALUE"
            | "LEAD"
            | "LENGTH"
            | "LOWER"
            | "LTRIM"
            | "MAX"
            | "MIN"
            | "NOW"
            | "NULLIF"
            | "NVL"
            | "RANK"
            | "ROUND"
            | "ROW_NUMBER"
            | "RTRIM"
            | "STRING_AGG"
            | "SUBSTR"
            | "SUBSTRING"
            | "SUM"
            | "TRIM"
            | "UPPER"
    )
}

#[cfg(test)]
mod tests {
    use super::{
        HIGHLIGHT_MAX_BYTES, HIGHLIGHT_MAX_LINE_BYTES, SqlInputHighlighter, SyntaxLang,
        resolve_syntax, should_degrade_to_plain, sql_highlighter_factory,
    };
    use gpui_kit::base::input::Rope;
    use gpui_kit::component::highlighter::{LanguageRegistry, SyntaxHighlighter};

    #[test]
    fn every_tree_sitter_language_has_grammar_queries_and_produces_styles() {
        let registry = LanguageRegistry::singleton();
        let theme = crate::theme::highlight_theme(true);
        let samples = [
            (
                SyntaxLang::Json,
                r#"{"name": "DevTools", "count": 28, "offline": true}"#,
            ),
            (
                SyntaxLang::Yaml,
                "app:\n  name: DevTools\n  offline: true\n  count: 28\n",
            ),
            (
                SyntaxLang::Toml,
                "[app]\nname = \"DevTools\"\ncount = 28\noffline = true\n",
            ),
            (
                SyntaxLang::Html,
                "<div class=\"card\"><span>DevTools</span></div>",
            ),
            (
                SyntaxLang::Xml,
                "<config><tool id=\"1\">DevTools</tool></config>",
            ),
            (
                SyntaxLang::XmlHtml,
                "<section data-id=\"1\"><p>Hello</p></section>",
            ),
            (
                SyntaxLang::Diff,
                "--- a/file.txt\n+++ b/file.txt\n@@ -1 +1 @@\n-old\n+new\n",
            ),
            (
                SyntaxLang::Markdown,
                "# DevTools\n\n| Name | Status |\n| --- | --- |\n| JSON | `ok` |\n",
            ),
            (SyntaxLang::Css, ".card { color: #3b82f6; padding: 12px; }"),
        ];

        for (lang, snippet) in samples {
            assert!(lang.uses_tree_sitter());
            let name = lang.highlighter_name();
            let config = registry
                .language(name)
                .unwrap_or_else(|| panic!("missing language registration for {lang:?} ({name})"));
            assert!(
                config.has_grammar(),
                "missing tree-sitter grammar for {lang:?} ({name})"
            );
            assert!(
                !config.highlights.is_empty(),
                "missing highlight query for {lang:?} ({name})"
            );
            let rope = Rope::from_str(snippet);
            let mut hl = SyntaxHighlighter::new(name);
            hl.update(None, &rope, None);
            let styles = hl.styles(&(0..snippet.len()), theme.as_ref());
            assert!(
                !styles.is_empty(),
                "expected syntax highlight spans for {lang:?} ({name})"
            );
        }

        // Plain text stays grammar-free for large-text fallback.
        let plain = registry.language("text").expect("plain text registration");
        assert!(!plain.has_grammar());
    }

    #[test]
    fn sql_highlighter_covers_full_range_and_colors_sql_tokens() {
        let factory = sql_highlighter_factory();
        assert!(factory("sql").is_some());
        assert!(factory("text").is_none());

        let sql = "-- 查询活跃工具\nSELECT u.id, COUNT(*) AS total FROM users u WHERE u.name = '张三''s' AND u.active = TRUE LIMIT 10; /* 注释 */";
        let mut hl = SqlInputHighlighter::default();
        hl.reparse(sql);

        let dark = crate::theme::highlight_theme(true);
        let runs = hl.resolve_range(&(0..sql.len()), dark.as_ref());
        assert!(!runs.is_empty());

        // Verify strict contiguous full-range coverage invariant required by gpui-base.
        let mut expected_start = 0usize;
        let mut colored_runs = 0usize;
        for (span, style) in &runs {
            assert_eq!(span.start, expected_start);
            assert!(span.start < span.end);
            assert!(sql.is_char_boundary(span.start));
            assert!(sql.is_char_boundary(span.end));
            if style.color.is_some() {
                colored_runs += 1;
            }
            expected_start = span.end;
        }
        assert_eq!(expected_start, sql.len());
        assert!(
            colored_runs >= 10,
            "expected rich SQL coloring, got only {colored_runs} colored runs"
        );

        // Verify sub-range slicing also maintains exact coverage.
        let sub_start = sql.find("SELECT").unwrap();
        let sub_end = sql.find("LIMIT").unwrap();
        let sub_runs = hl.resolve_range(&(sub_start..sub_end), dark.as_ref());
        assert_eq!(sub_runs.first().unwrap().0.start, sub_start);
        assert_eq!(sub_runs.last().unwrap().0.end, sub_end);
    }

    #[test]
    fn large_text_and_long_single_line_degrade_to_plain_text() {
        assert!(!should_degrade_to_plain("{\"a\": 1}\n"));

        // Exact 50 KiB single line stays highlighted; 50 KiB + 1 byte degrades.
        let exact_line = "a".repeat(HIGHLIGHT_MAX_LINE_BYTES);
        assert!(!should_degrade_to_plain(&exact_line));
        let over_line = "a".repeat(HIGHLIGHT_MAX_LINE_BYTES + 1);
        assert!(should_degrade_to_plain(&over_line));
        let degraded_line = resolve_syntax(Some(SyntaxLang::Json), &over_line);
        assert_eq!(degraded_line.highlighter, "text");
        assert_eq!(degraded_line.badge, Some("JSON"));
        assert!(degraded_line.degraded);

        // Multi-line 200 KiB payload with short lines remains highlighted.
        let multiline_ok = "{\"key\": \"value\"}\n".repeat(12_000);
        assert!(multiline_ok.len() > HIGHLIGHT_MAX_LINE_BYTES);
        assert!(multiline_ok.len() < HIGHLIGHT_MAX_BYTES);
        let ok_spec = resolve_syntax(Some(SyntaxLang::Json), &multiline_ok);
        assert_eq!(ok_spec.highlighter, "json");
        assert!(!ok_spec.degraded);

        // > 1 MiB multi-line payload degrades even when every line is short.
        let over_total = "0123456789abcdef\n".repeat((HIGHLIGHT_MAX_BYTES / 17) + 2);
        assert!(over_total.len() > HIGHLIGHT_MAX_BYTES);
        let degraded_total = resolve_syntax(Some(SyntaxLang::Sql), &over_total);
        assert_eq!(degraded_total.highlighter, "text");
        assert_eq!(degraded_total.badge, Some("SQL"));
        assert!(degraded_total.degraded);
    }
}
