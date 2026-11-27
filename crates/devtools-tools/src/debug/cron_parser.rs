//! #23 Cron 表达式解析（CronParser）。
//!
//! 基于 `croner 5.0 (std, jiff)`（零 `chrono`）支持 5/6 段表达式，含 Quartz
//! `L`/`W`/`#`/`?`；通过 `jiff` 切换时区；返回未来 5/10/20/50 次触发（默认 10）
//! 与中文主 / 英文副自然语言描述。

use std::str::FromStr;

use croner::Cron;
use croner::describe::English;
use jiff::ZonedDateTime;
use jiff::tz::TimeZone;

use crate::{ToolError, ToolErrorKind};

/// 未来触发次数选项。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TriggerCount {
    Five,
    #[default]
    Ten,
    Twenty,
    Fifty,
}

impl TriggerCount {
    pub const fn value(self) -> usize {
        match self {
            Self::Five => 5,
            Self::Ten => 10,
            Self::Twenty => 20,
            Self::Fifty => 50,
        }
    }
}

/// Cron 解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronResult {
    pub expression: String,
    pub field_count: usize,
    /// 中文主描述。
    pub description_zh: String,
    /// 英文副描述（croner `English`）。
    pub description_en: String,
    /// 未来触发时刻（ISO-8601 带时区偏移）。
    pub next_triggers: Vec<String>,
    pub time_zone: String,
}

/// 将 5 段表达式补齐为 6 段（秒字段前置 `0`）。
fn normalize_expression(expression: &str) -> Result<(String, usize), ToolError> {
    let trimmed = expression.trim();
    let fields: Vec<&str> = trimmed.split_whitespace().collect();
    match fields.len() {
        5 => Ok((format!("0 {}", fields.join(" ")), 5)),
        6 => Ok((trimmed.to_owned(), 6)),
        other => Err(ToolError::new(
            ToolErrorKind::InvalidSyntax,
            format!("Cron 表达式必须为 5 或 6 段，实际为 {other} 段"),
        )
        .with_hint("例如 0 0 * * *（5 段）或 0 0 0 * * *（6 段）")),
    }
}

/// 生成中文主描述（基于字段逐项解读，覆盖常用表达式与 Quartz 特殊符号）。
fn describe_zh(expression: &str, cron: &Cron) -> String {
    let pattern = &cron.pattern;
    let fields: Vec<&str> = expression.split_whitespace().collect();
    // 6 段：秒 分 时 日 月 周；5 段已补秒，统一按 6 段处理。
    let (second, minute, hour, day, month, weekday) = match fields.as_slice() {
        [s, m, h, d, mo, w] => (*s, *m, *h, *d, *mo, *w),
        _ => return pattern.to_string(),
    };
    let time = if hour == "*" && minute == "*" {
        "每分钟".to_owned()
    } else if hour == "*" {
        format!("每小时的第 {minute} 分")
    } else if minute == "*" {
        format!("每天 {hour} 点整")
    } else {
        format!("每天 {hour}:{minute}:{second}")
    };
    let day_part = match day {
        "*" | "?" => String::new(),
        d if d.contains('L') => format!("，每月最后一日（{d}）"),
        d if d.contains('W') => format!("，最近工作日（{d}）"),
        d => format!("，每月 {d} 日"),
    };
    let week_part = match weekday {
        "*" | "?" => String::new(),
        w if w.contains('#') => format!("，{w}（第 n 个星期几）"),
        w => format!("，每周 {w}"),
    };
    let month_part = if month == "*" {
        String::new()
    } else {
        format!("，仅在 {month} 月")
    };
    format!(
        "{time}{day_part}{week_part}{month_part}（表达式：{}）",
        pattern
    )
}

/// 解析 Cron 表达式并计算未来触发时刻。
///
/// `time_zone` 为 IANA 时区名（如 `Asia/Shanghai`），无效时区返回
/// [`ToolErrorKind::InvalidSyntax`]。
pub fn parse_cron(
    expression: &str,
    time_zone: &str,
    count: TriggerCount,
) -> Result<CronResult, ToolError> {
    let (normalized, field_count) = normalize_expression(expression)?;
    let cron = Cron::from_str(&normalized).map_err(|e| {
        ToolError::new(
            ToolErrorKind::InvalidSyntax,
            format!("Cron 表达式无效：{e}"),
        )
        .with_hint("支持 5/6 段与 Quartz L/W/#/? 特殊符号")
    })?;

    let tz = TimeZone::get(time_zone).map_err(|e| {
        ToolError::new(
            ToolErrorKind::InvalidSyntax,
            format!("无效 IANA 时区 `{time_zone}`：{e}"),
        )
    })?;
    let now = ZonedDateTime::now().with_time_zone(tz);
    let mut cursor = now;
    let mut next_triggers = Vec::with_capacity(count.value());
    for _ in 0..count.value() {
        let next: ZonedDateTime = cron.find_next_occurrence(&cursor, false).map_err(|e| {
            ToolError::new(ToolErrorKind::Internal, format!("计算下次触发失败：{e}"))
        })?;
        next_triggers.push(next.to_string());
        cursor = next;
    }

    let description_zh = describe_zh(&normalized, &cron);
    let description_en = cron.describe_lang(English);
    Ok(CronResult {
        expression: normalized,
        field_count,
        description_zh,
        description_en,
        next_triggers,
        time_zone: time_zone.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn five_and_six_field_expressions() {
        let five = parse_cron("0 9 * * 1-5", "Asia/Shanghai", TriggerCount::Five).unwrap();
        assert_eq!(five.field_count, 5);
        assert_eq!(five.next_triggers.len(), 5);
        assert!(five.description_zh.contains("每周"));
        assert!(!five.description_en.is_empty());

        let six = parse_cron("0 30 9 * * 1-5", "Asia/Shanghai", TriggerCount::Five).unwrap();
        assert_eq!(six.field_count, 6);
        assert_eq!(six.next_triggers.len(), 5);
    }

    #[test]
    fn quartz_special_characters() {
        let quartz = parse_cron("0 0 0 L * ?", "UTC", TriggerCount::Five).unwrap();
        assert!(quartz.description_zh.contains("最后一日"));
        let nth = parse_cron("0 0 0 ? * 2#3", "UTC", TriggerCount::Five).unwrap();
        assert!(nth.description_zh.contains('#'));
    }

    #[test]
    fn trigger_counts() {
        for (count, expected) in [
            (TriggerCount::Five, 5),
            (TriggerCount::Ten, 10),
            (TriggerCount::Twenty, 20),
            (TriggerCount::Fifty, 50),
        ] {
            let result = parse_cron("* * * * *", "UTC", count).unwrap();
            assert_eq!(result.next_triggers.len(), expected);
        }
    }

    #[test]
    fn timezone_and_syntax_errors() {
        let err = parse_cron("* * * *", "UTC", TriggerCount::Five).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
        let err = parse_cron("* * * * *", "Not/AZone", TriggerCount::Five).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
    }
}
