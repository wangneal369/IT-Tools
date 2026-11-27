//! #12 SQL 格式化（`lax-sql`）。
//!
//! 关键字大写 / 小写 / 保留，展开 / 紧凑布局，缩进 2/4；
//! 30 种方言通用，无需选择方言。

use std::path::Path;

use dprint_core::configuration::NewLineKind;
use lax_sql::configuration::{ClauseStyle, Configuration, KeywordCase};

use crate::util::invalid_syntax;

use devtools_core::ToolError;

/// 关键字大小写策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlKeywordCase {
    Upper,
    Lower,
    Preserve,
}

/// 布局策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlLayout {
    /// 每个子句独立成行，子句体缩进（经典 SQL 风格）。
    Expanded,
    /// 子句内按行宽填充（紧凑）。
    Compact,
}

/// 缩进宽度。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlIndent {
    Two,
    Four,
}

impl SqlIndent {
    const fn width(self) -> u8 {
        match self {
            SqlIndent::Two => 2,
            SqlIndent::Four => 4,
        }
    }
}

/// 单次请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlFormatterRequest {
    pub input: String,
    pub keyword_case: SqlKeywordCase,
    pub layout: SqlLayout,
    pub indent: SqlIndent,
}

/// 结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlFormatterResponse {
    pub output: String,
    /// 格式化是否改变了输入。
    pub changed: bool,
}

/// 执行 SQL 格式化。
pub fn run(req: &SqlFormatterRequest) -> Result<SqlFormatterResponse, ToolError> {
    const MAX_INPUT: usize = 4 * 1024 * 1024;
    if req.input.len() > MAX_INPUT {
        return Err(crate::util::size_limit("输入超过 4 MB 上限"));
    }
    let config = Configuration {
        line_width: 100,
        use_tabs: false,
        indent_width: req.indent.width(),
        new_line_kind: NewLineKind::LineFeed,
        keyword_case: match req.keyword_case {
            SqlKeywordCase::Upper => KeywordCase::Upper,
            SqlKeywordCase::Lower => KeywordCase::Lower,
            SqlKeywordCase::Preserve => KeywordCase::Preserve,
        },
        clause_style: match req.layout {
            SqlLayout::Expanded => ClauseStyle::Expanded,
            SqlLayout::Compact => ClauseStyle::Fill,
        },
        ignore_node_comment_text: "dprint-ignore".to_string(),
        ignore_file_comment_text: "dprint-ignore-file".to_string(),
    };
    let formatted = lax_sql::format_text(Path::new("query.sql"), &req.input, &config)
        .map_err(|err| invalid_syntax(format!("SQL 格式化失败：{err}")))?;
    match formatted {
        Some(output) => Ok(SqlFormatterResponse {
            changed: output != req.input,
            output,
        }),
        None => Ok(SqlFormatterResponse {
            output: req.input.clone(),
            changed: false,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(input: &str) -> SqlFormatterRequest {
        SqlFormatterRequest {
            input: input.to_string(),
            keyword_case: SqlKeywordCase::Upper,
            layout: SqlLayout::Expanded,
            indent: SqlIndent::Two,
        }
    }

    #[test]
    fn keywords_are_uppercased() {
        let resp = run(&request("select id, name from users where id = 1")).unwrap();
        assert!(resp.output.contains("SELECT"), "{}", resp.output);
        assert!(resp.output.contains("FROM"), "{}", resp.output);
        assert!(resp.output.contains("WHERE"), "{}", resp.output);
        assert!(resp.changed);
    }

    #[test]
    fn keywords_can_be_lowercased_or_preserved() {
        let mut req = request("SELECT ID FROM users");
        req.keyword_case = SqlKeywordCase::Lower;
        let lower = run(&req).unwrap();
        assert!(lower.output.contains("select"), "{}", lower.output);
        assert!(!lower.output.contains("SELECT"), "{}", lower.output);

        let mut req = request("SeLeCt id FROM users");
        req.keyword_case = SqlKeywordCase::Preserve;
        let preserved = run(&req).unwrap();
        assert!(preserved.output.contains("SeLeCt"), "{}", preserved.output);
    }

    #[test]
    fn expanded_layout_breaks_clauses_onto_lines() {
        let resp = run(&request(
            "select a, b from t where a = 1 and b = 2 order by a",
        ))
        .unwrap();
        assert!(resp.output.contains('\n'), "{}", resp.output);
        assert!(resp.output.lines().count() >= 3, "{}", resp.output);
    }

    #[test]
    fn compact_layout_is_shorter_than_expanded() {
        let input = "select a, b from t where a = 1 and b = 2 order by a";
        let expanded = run(&request(input)).unwrap();
        let mut req = request(input);
        req.layout = SqlLayout::Compact;
        let compact = run(&req).unwrap();
        assert!(compact.output.lines().count() <= expanded.output.lines().count());
    }

    #[test]
    fn indent_width_four_is_applied_in_expanded_layout() {
        let mut req = request("select a, b from t where a = 1");
        req.indent = SqlIndent::Four;
        let resp = run(&req).unwrap();
        assert!(
            resp.output.lines().any(|line| line.starts_with("    ")),
            "{}",
            resp.output
        );
    }

    #[test]
    fn empty_input_yields_empty_output() {
        let resp = run(&request("   ")).unwrap();
        assert!(resp.output.trim().is_empty());
        let resp = run(&request("")).unwrap();
        assert!(resp.output.is_empty());
        assert!(!resp.changed);
    }
}
