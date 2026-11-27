//! #19 文本分析与实用工具（TextAnalysis）。
//!
//! 12 种命名大小写转换、行操作（去重 / 排序 / 反转 / 修剪 / `LF<->CRLF`）、
//! Unicode 字素 / 字节 / 词 / 行 / 段落计数与 Top-20 词频。

use std::collections::HashMap;

use convert_case::{Boundary, Case, Casing, Pattern};
use unicode_segmentation::UnicodeSegmentation;

/// 12 种命名大小写风格。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamingCase {
    Camel,
    Pascal,
    Snake,
    ScreamingSnake,
    Kebab,
    Train,
    Title,
    Sentence,
    Flat,
    UpperFlat,
    Dot,
    Path,
}

impl NamingCase {
    pub const ALL: [Self; 12] = [
        Self::Camel,
        Self::Pascal,
        Self::Snake,
        Self::ScreamingSnake,
        Self::Kebab,
        Self::Train,
        Self::Title,
        Self::Sentence,
        Self::Flat,
        Self::UpperFlat,
        Self::Dot,
        Self::Path,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Camel => "camelCase",
            Self::Pascal => "PascalCase",
            Self::Snake => "snake_case",
            Self::ScreamingSnake => "SCREAMING_SNAKE_CASE",
            Self::Kebab => "kebab-case",
            Self::Train => "Train-Case",
            Self::Title => "Title Case",
            Self::Sentence => "Sentence case",
            Self::Flat => "flatcase",
            Self::UpperFlat => "UPPERFLATCASE",
            Self::Dot => "dot.case",
            Self::Path => "path/case",
        }
    }

    /// 将任意输入转换为该命名风格。
    pub fn convert(self, input: &str) -> String {
        match self {
            Self::Camel => input.to_case(Case::Camel),
            Self::Pascal => input.to_case(Case::Pascal),
            Self::Snake => input.to_case(Case::Snake),
            Self::ScreamingSnake => input.to_case(Case::UpperSnake),
            Self::Kebab => input.to_case(Case::Kebab),
            Self::Train => input.to_case(Case::Train),
            Self::Title => input.to_case(Case::Title),
            Self::Sentence => input.to_case(Case::Sentence),
            Self::Flat => input.to_case(Case::Flat),
            Self::UpperFlat => input.to_case(Case::UpperFlat),
            Self::Dot => input.to_case(Case::Custom {
                boundaries: &[
                    Boundary::Underscore,
                    Boundary::Hyphen,
                    Boundary::Space,
                    Boundary::LowerUpper,
                    Boundary::Acronym,
                    Boundary::DigitUpper,
                    Boundary::UpperDigit,
                    Boundary::DigitLower,
                    Boundary::LowerDigit,
                ],
                pattern: Pattern::Lowercase,
                delimiter: ".",
            }),
            Self::Path => input.to_case(Case::Custom {
                boundaries: &[
                    Boundary::Underscore,
                    Boundary::Hyphen,
                    Boundary::Space,
                    Boundary::LowerUpper,
                    Boundary::Acronym,
                    Boundary::DigitUpper,
                    Boundary::UpperDigit,
                    Boundary::DigitLower,
                    Boundary::LowerDigit,
                ],
                pattern: Pattern::Lowercase,
                delimiter: "/",
            }),
        }
    }
}

/// 一次性返回全部 12 种转换结果（顺序与 [`NamingCase::ALL`] 一致）。
pub fn all_naming_cases(input: &str) -> Vec<(NamingCase, String)> {
    NamingCase::ALL
        .into_iter()
        .map(|case| (case, case.convert(input)))
        .collect()
}

/// 行操作选项。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LineOptions {
    pub dedupe: bool,
    pub sort: bool,
    pub reverse: bool,
    pub trim: bool,
    /// 将行尾统一为 LF；为 `false` 时统一为 CRLF。
    pub lf: bool,
}

/// 执行行操作。
pub fn transform_lines(input: &str, options: &LineOptions) -> String {
    let mut lines: Vec<String> = input.lines().map(str::to_owned).collect();
    if options.trim {
        for line in &mut lines {
            *line = line.trim().to_owned();
        }
    }
    if options.dedupe {
        let mut seen = std::collections::HashSet::new();
        lines.retain(|line| seen.insert(line.clone()));
    }
    if options.sort {
        lines.sort();
    }
    if options.reverse {
        lines.reverse();
    }
    let sep = if options.lf { "\n" } else { "\r\n" };
    let mut out = lines.join(sep);
    if !out.is_empty() {
        out.push_str(sep);
    }
    out
}

/// Unicode 统计结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextStats {
    pub graphemes: usize,
    pub bytes: usize,
    pub words: usize,
    pub lines: usize,
    pub paragraphs: usize,
    /// 按出现频次降序、频次相同按字典序，最多 20 项。
    pub top_words: Vec<(String, usize)>,
}

/// Unicode 感知统计（字素用 `unicode-segmentation`，词与段落遵循 UAX #29）。
pub fn analyze_text(input: &str) -> TextStats {
    let graphemes = input.graphemes(true).count();
    let words = input.unicode_words().count();
    let lines = input.lines().count();
    let paragraphs = input
        .split(['\n', '\r'])
        .filter(|p| !p.trim().is_empty())
        .count();

    let mut freq: HashMap<String, usize> = HashMap::new();
    for word in input.unicode_words() {
        *freq
            .entry(deunicode::deunicode(word).to_lowercase())
            .or_insert(0) += 1;
    }
    let mut top_words: Vec<(String, usize)> = freq.into_iter().collect();
    top_words.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    top_words.truncate(20);

    TextStats {
        graphemes,
        bytes: input.len(),
        words,
        lines,
        paragraphs,
        top_words,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_twelve_naming_cases() {
        let cases = all_naming_cases("myVariable NAME");
        assert_eq!(cases.len(), 12);
        let map: HashMap<_, _> = cases.into_iter().collect();
        assert_eq!(map[&NamingCase::Camel], "myVariableName");
        assert_eq!(map[&NamingCase::Pascal], "MyVariableName");
        assert_eq!(map[&NamingCase::Snake], "my_variable_name");
        assert_eq!(map[&NamingCase::ScreamingSnake], "MY_VARIABLE_NAME");
        assert_eq!(map[&NamingCase::Kebab], "my-variable-name");
        assert_eq!(map[&NamingCase::Train], "My-Variable-Name");
        assert_eq!(map[&NamingCase::Dot], "my.variable.name");
        assert_eq!(map[&NamingCase::Path], "my/variable/name");
        assert_eq!(map[&NamingCase::UpperFlat], "MYVARIABLENAME");
    }

    #[test]
    fn line_operations() {
        let input = " b \na\nb\n a \n";
        let out = transform_lines(
            input,
            &LineOptions {
                dedupe: true,
                sort: true,
                trim: true,
                lf: true,
                ..Default::default()
            },
        );
        assert_eq!(out, "a\nb\n");

        let reversed = transform_lines(
            "a\nb\n",
            &LineOptions {
                reverse: true,
                lf: false,
                ..Default::default()
            },
        );
        assert_eq!(reversed, "b\r\na\r\n");
    }

    #[test]
    fn unicode_stats_and_word_frequency() {
        let stats = analyze_text("Hello hello 世界\n第二行");
        assert_eq!(
            stats.graphemes,
            "Hello hello 世界\n第二行".graphemes(true).count()
        );
        assert_eq!(stats.bytes, "Hello hello 世界\n第二行".len());
        // Unicode 词划分将 CJK 逐字计数：Hello + hello + 世 + 界 + 第 + 二 + 行。
        assert_eq!(stats.words, 7);
        assert_eq!(stats.lines, 2);
        assert_eq!(stats.paragraphs, 2);
        assert_eq!(stats.top_words[0], ("hello".to_owned(), 2));
        assert!(stats.top_words.len() <= 20);
    }
}
