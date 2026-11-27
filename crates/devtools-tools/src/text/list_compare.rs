//! #20 列表集合对比（ListCompare）。
//!
//! 双输入 A/B，分隔符可配，支持修剪 / 去重 / 忽略大小写，使用 `IndexMap`
//! 保序产出交集 / 并集 / 仅 A / 仅 B 四视图与计数。

use indexmap::IndexMap;

/// 列表对比选项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListCompareOptions {
    /// 分隔符（默认换行）。
    pub delimiter: String,
    pub trim: bool,
    pub dedupe: bool,
    pub ignore_case: bool,
}

impl Default for ListCompareOptions {
    fn default() -> Self {
        Self {
            delimiter: "\n".to_owned(),
            trim: true,
            dedupe: true,
            ignore_case: false,
        }
    }
}

/// 四视图结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListCompareResult {
    pub intersection: Vec<String>,
    pub union: Vec<String>,
    pub only_a: Vec<String>,
    pub only_b: Vec<String>,
}

impl ListCompareResult {
    pub fn intersection_count(&self) -> usize {
        self.intersection.len()
    }
    pub fn union_count(&self) -> usize {
        self.union.len()
    }
    pub fn only_a_count(&self) -> usize {
        self.only_a.len()
    }
    pub fn only_b_count(&self) -> usize {
        self.only_b.len()
    }
}

/// 解析输入为规范化条目序列。
pub fn parse_items(input: &str, options: &ListCompareOptions) -> Vec<String> {
    let mut items: Vec<String> = input
        .split(&options.delimiter)
        .map(|s| {
            let item = if options.trim { s.trim() } else { s };
            if options.ignore_case {
                item.to_lowercase()
            } else {
                item.to_owned()
            }
        })
        .filter(|s| !s.is_empty())
        .collect();
    if options.dedupe {
        let mut seen = std::collections::HashSet::new();
        items.retain(|s| seen.insert(s.clone()));
    }
    items
}

/// 保序集合对比。
pub fn compare_lists(a: &str, b: &str, options: &ListCompareOptions) -> ListCompareResult {
    let items_a = parse_items(a, options);
    let items_b = parse_items(b, options);

    let set_a: IndexMap<&str, ()> = items_a.iter().map(|s| (s.as_str(), ())).collect();
    let set_b: IndexMap<&str, ()> = items_b.iter().map(|s| (s.as_str(), ())).collect();

    let intersection = items_a
        .iter()
        .filter(|s| set_b.contains_key(s.as_str()))
        .cloned()
        .collect();
    let only_a = items_a
        .iter()
        .filter(|s| !set_b.contains_key(s.as_str()))
        .cloned()
        .collect();
    let only_b = items_b
        .iter()
        .filter(|s| !set_a.contains_key(s.as_str()))
        .cloned()
        .collect();

    // 并集：先 A 全部，再追加 B 中未在 A 出现者（保序）。
    let mut union = items_a.clone();
    union.extend(
        items_b
            .iter()
            .filter(|s| !set_a.contains_key(s.as_str()))
            .cloned(),
    );

    ListCompareResult {
        intersection,
        union,
        only_a,
        only_b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn four_views_preserve_order() {
        let result = compare_lists("b\na\nc\n", "c\nb\nd\n", &ListCompareOptions::default());
        assert_eq!(result.intersection, vec!["b", "c"]);
        assert_eq!(result.union, vec!["b", "a", "c", "d"]);
        assert_eq!(result.only_a, vec!["a"]);
        assert_eq!(result.only_b, vec!["d"]);
        assert_eq!(result.only_a_count(), 1);
    }

    #[test]
    fn configurable_delimiter_trim_dedupe_ignore_case() {
        let options = ListCompareOptions {
            delimiter: ",".to_owned(),
            trim: true,
            dedupe: true,
            ignore_case: true,
        };
        let result = compare_lists(" Apple, banana ,APPLE ", "apple,cherry", &options);
        assert_eq!(result.intersection, vec!["apple"]);
        assert_eq!(result.only_a, vec!["banana"]);
        assert_eq!(result.only_b, vec!["cherry"]);
    }

    #[test]
    fn empty_inputs() {
        let result = compare_lists("", "", &ListCompareOptions::default());
        assert!(result.union.is_empty());
        assert_eq!(result.union_count(), 0);
    }
}
