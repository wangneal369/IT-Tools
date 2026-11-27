//! Windows Release + thin LTO 验活：`serde_json_path` 的 `inventory` 函数注册。
//!
//! `serde_json_path 0.7` 通过 `inventory` 收集内置函数扩展。启用
//! `lto = "thin"`、`codegen-units = 1`、`strip = "symbols"`、`panic = "abort"`
//! 的 Release 构建若发生符号裁剪，内置函数会在解析期报 `Undefined`，而不是
//! 编译失败。本测试必须在 `cargo test --release`（真实 Release Profile）下
//! 运行，逐项验证 RFC 9535 的 5 个内置函数仍被注册并可求值：
//!
//! - `match()` / `search()`：正则逻辑函数（`fancy-regex`）
//! - `length()`：字符串 / 数组 / 对象长度
//! - `count()`：节点集合计数
//! - `value()`：节点转逻辑值
//!
//! 同时复验上游 `#124` 的未闭合 `\u` panic 护栏在 Release 下仍返回 `Err`。
#![cfg(feature = "debug")]

use devtools_tools::debug::query_json_path;

const DOC: &str = r#"{
    "items": [
        {"name": "alpha", "size": 3, "tags": ["red", "blue"]},
        {"name": "beta", "size": 7, "tags": ["blue"]},
        {"name": "gamma", "size": 11, "tags": []}
    ]
}"#;

/// 执行查询并返回匹配项的 `name` 字段，便于失败时直接定位是哪个函数失效。
fn matched_names(path: &str) -> Vec<String> {
    let result = query_json_path(DOC, path)
        .unwrap_or_else(|err| panic!("JSONPath 查询失败 `{path}`：{err}"));
    result
        .matches
        .iter()
        .map(|m| {
            m.value
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_owned()
        })
        .collect()
}

#[test]
fn match_function_is_registered_under_release_lto() {
    // `[ab].*`：alpha / beta 命中，gamma 不命中。
    let names = matched_names("$.items[?match(@.name, '[ab].*')]");
    assert_eq!(names, ["alpha", "beta"]);
}

#[test]
fn search_function_is_registered_under_release_lto() {
    // `amm`：仅 gamma 包含该子串。
    let names = matched_names("$.items[?search(@.name, 'amm')]");
    assert_eq!(names, ["gamma"]);
}

#[test]
fn length_function_is_registered_under_release_lto() {
    // alpha / gamma 长度为 5；beta 为 4。
    let names = matched_names("$.items[?length(@.name) == 5]");
    assert_eq!(names, ["alpha", "gamma"]);
}

#[test]
fn count_function_is_registered_under_release_lto() {
    // tags 节点数：alpha 2、beta 1、gamma 0。
    let names = matched_names("$.items[?count(@.tags[*]) == 1]");
    assert_eq!(names, ["beta"]);
}

#[test]
fn value_function_is_registered_under_release_lto() {
    // size > 6：beta / gamma。
    let names = matched_names("$.items[?value(@.size) > 6]");
    assert_eq!(names, ["beta", "gamma"]);
}

#[test]
fn all_five_functions_resolve_in_one_expression() {
    // 单条表达式串联全部 5 个函数：任一注册丢失都会在此触发 `Undefined`。
    let names = matched_names(
        "$.items[?match(@.name, '[a-z]+') && search(@.name, 'a') \
         && length(@.name) == 5 && count(@.tags[*]) >= 0 && value(@.size) > 0]",
    );
    assert_eq!(names, ["alpha", "gamma"]);
}

#[test]
fn unclosed_unicode_escape_is_rejected_without_panicking() {
    // 上游 serde_json_path #124：未闭合 `\u` 会让解析器 panic。
    // Release 下必须命中前置校验并返回 Err，而不是 abort。
    for path in [r#"$["\u12"]"#, r#"$["\uZZZZ"]"#, r#"$["\u1"]"#, r#"$["\u"#] {
        let err = match query_json_path(DOC, path) {
            Ok(result) => panic!("未闭合转义未被拦截：{path} → {result:?}"),
            Err(err) => err,
        };
        assert_eq!(
            err.kind,
            devtools_tools::ToolErrorKind::InvalidSyntax,
            "{path}"
        );
    }
}

#[test]
fn zero_match_and_syntax_error_stay_distinct_under_release_lto() {
    let empty = query_json_path(DOC, "$.items[?value(@.size) > 100]").unwrap();
    assert!(empty.matches.is_empty());
    assert!(empty.empty_result);

    let err = query_json_path(DOC, "$.items[").unwrap_err();
    assert_eq!(err.kind, devtools_tools::ToolErrorKind::InvalidSyntax);
}
