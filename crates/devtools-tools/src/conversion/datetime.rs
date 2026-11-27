//! #2 日期与时间戳转换（`jiff 1.0`，禁用 `chrono`）。
//!
//! 单入口解析 ISO / RFC 2822 / 常见日期 / 纯数字时间戳，并同时输出
//! UTC、本机时区与指定 IANA 时区的表示。纯数字时间戳按位数自动判定
//! 秒 / 毫秒 / 微秒 / 纳秒，也可显式覆写。

use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;
use jiff::{Timestamp, ZonedDateTime};

use crate::util::{invalid_syntax, size_limit, unsupported};

use devtools_core::ToolError;

/// 纯数字时间戳的单位覆写。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampUnit {
    /// 按位数自动判定。
    Auto,
    Seconds,
    Millis,
    Micros,
    Nanos,
}

impl TimestampUnit {
    pub const fn label(self) -> &'static str {
        match self {
            TimestampUnit::Auto => "auto",
            TimestampUnit::Seconds => "seconds",
            TimestampUnit::Millis => "milliseconds",
            TimestampUnit::Micros => "microseconds",
            TimestampUnit::Nanos => "nanoseconds",
        }
    }
}

/// 单次转换请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateTimeConverterRequest {
    pub input: String,
    /// `"Local"`、`"UTC"` 或 IANA 名称（如 `"Asia/Shanghai"`）。
    pub timezone: String,
    pub timestamp_unit: TimestampUnit,
}

/// 转换结果（同一时刻的多时区表示）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateTimeConverterResponse {
    /// 输入是否为纯数字时间戳。
    pub is_timestamp_input: bool,
    /// 实际采用的单位（纯数字输入时非空）。
    pub timestamp_unit: Option<&'static str>,
    /// RFC 3339（UTC）。
    pub utc: String,
    /// 本机时区 RFC 3339。
    pub local: String,
    /// 请求的时区名称。
    pub timezone_name: String,
    /// 请求时区的 RFC 3339。
    pub timezone: String,
    pub unix_seconds: i64,
    pub unix_millis: i64,
    pub unix_micros: i128,
    pub unix_nanos: i128,
    /// 英文星期名。
    pub weekday: String,
    /// ISO 周，例如 `2024-W01-2`。
    pub iso_week: String,
}

/// 解析输入并生成多时区表示。
pub fn convert(req: &DateTimeConverterRequest) -> Result<DateTimeConverterResponse, ToolError> {
    const MAX_INPUT: usize = 1024;
    if req.input.len() > MAX_INPUT {
        return Err(size_limit("日期时间输入超过 1 KB 上限"));
    }
    let target_tz = resolve_timezone(&req.timezone)?;
    let parsed = parse_input(req.input.trim(), req.timestamp_unit, &target_tz)?;
    let timestamp = parsed.timestamp;

    let utc_zoned = timestamp.to_zoned_datetime(TimeZone::UTC);
    let local_zoned = timestamp.to_zoned_datetime(TimeZone::system());
    let target_zoned = timestamp.to_zoned_datetime(target_tz);

    Ok(DateTimeConverterResponse {
        is_timestamp_input: parsed.is_timestamp,
        timestamp_unit: parsed.unit,
        utc: format_zoned(&utc_zoned),
        local: format_zoned(&local_zoned),
        timezone_name: req.timezone.clone(),
        timezone: format_zoned(&target_zoned),
        unix_seconds: timestamp.as_second(),
        unix_millis: timestamp.as_millisecond(),
        unix_micros: i128::from(timestamp.as_microsecond()),
        unix_nanos: timestamp.as_nanosecond(),
        weekday: target_zoned.strftime("%A").to_string(),
        iso_week: target_zoned.strftime("%G-W%V-%u").to_string(),
    })
}

struct Parsed {
    timestamp: Timestamp,
    is_timestamp: bool,
    unit: Option<&'static str>,
}

fn format_zoned(zoned: &ZonedDateTime) -> String {
    zoned.strftime("%Y-%m-%dT%H:%M:%S%.f%:z").to_string()
}

fn resolve_timezone(name: &str) -> Result<TimeZone, ToolError> {
    match name {
        "UTC" | "utc" | "Z" => Ok(TimeZone::UTC),
        "Local" | "local" => Ok(TimeZone::system()),
        other => TimeZone::get(other).map_err(|err| {
            unsupported(format!("未知时区 `{other}`：{err}"))
                .with_hint("请使用 IANA 时区名，例如 Asia/Shanghai、America/New_York")
        }),
    }
}

fn parse_input(
    input: &str,
    unit: TimestampUnit,
    target_tz: &TimeZone,
) -> Result<Parsed, ToolError> {
    if input.is_empty() {
        return Err(invalid_syntax("输入为空"));
    }
    if is_integer(input) {
        return parse_numeric_timestamp(input, unit);
    }
    if let Ok(timestamp) = input.parse::<Timestamp>() {
        return Ok(Parsed {
            timestamp,
            is_timestamp: false,
            unit: None,
        });
    }
    if let Ok(zoned) = jiff::fmt::rfc2822::parse(input) {
        return Ok(Parsed {
            timestamp: zoned.timestamp(),
            is_timestamp: false,
            unit: None,
        });
    }
    if let Some(timestamp) = parse_civil(input, target_tz)? {
        return Ok(Parsed {
            timestamp,
            is_timestamp: false,
            unit: None,
        });
    }
    Err(invalid_syntax(format!("无法识别的日期时间：`{input}`"))
        .with_hint("支持 ISO 8601（2024-01-02T03:04:05Z）、RFC 2822、常见日期格式与纯数字时间戳"))
}

fn is_integer(input: &str) -> bool {
    let digits = input
        .strip_prefix('-')
        .or_else(|| input.strip_prefix('+'))
        .unwrap_or(input);
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

fn parse_numeric_timestamp(input: &str, unit: TimestampUnit) -> Result<Parsed, ToolError> {
    let digits = input.trim_start_matches(['-', '+']);
    let resolved = match unit {
        TimestampUnit::Auto => match digits.len() {
            0..=10 => TimestampUnit::Seconds,
            11..=13 => TimestampUnit::Millis,
            14..=16 => TimestampUnit::Micros,
            _ => TimestampUnit::Nanos,
        },
        explicit => explicit,
    };
    let timestamp = match resolved {
        TimestampUnit::Auto => unreachable!("auto is resolved above"),
        TimestampUnit::Seconds => {
            let value: i64 = input
                .parse()
                .map_err(|_| unsupported("时间戳超出 64 位秒范围"))?;
            Timestamp::from_second(value)
        }
        TimestampUnit::Millis => {
            let value: i64 = input
                .parse()
                .map_err(|_| unsupported("时间戳超出 64 位毫秒范围"))?;
            Timestamp::from_millisecond(value)
        }
        TimestampUnit::Micros => {
            let value: i64 = input
                .parse()
                .map_err(|_| unsupported("时间戳超出 64 位微秒范围"))?;
            Timestamp::from_microsecond(value)
        }
        TimestampUnit::Nanos => {
            let value: i128 = input
                .parse()
                .map_err(|_| unsupported("时间戳超出 128 位纳秒范围"))?;
            Timestamp::from_nanosecond(value)
        }
    }
    .map_err(|err| {
        unsupported(format!("时间戳超出可表示范围：{err}"))
            .with_hint("请检查时间戳单位，或改用更小的数值")
    })?;
    Ok(Parsed {
        timestamp,
        is_timestamp: true,
        unit: Some(resolved.label()),
    })
}

fn parse_civil(input: &str, tz: &TimeZone) -> Result<Option<Timestamp>, ToolError> {
    const DATETIME_FORMATS: &[&str] = &[
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y/%m/%d %H:%M:%S",
        "%Y/%m/%d %H:%M",
        "%B %d, %Y %H:%M:%S",
        "%B %d, %Y",
        "%b %d, %Y %H:%M:%S",
        "%b %d, %Y",
        "%d %B %Y",
    ];
    const DATE_FORMATS: &[&str] = &["%Y-%m-%d", "%Y/%m/%d"];

    for format in DATETIME_FORMATS {
        if let Ok(datetime) = DateTime::strptime(format, input) {
            return Ok(Some(interpret_civil(datetime, tz)?));
        }
    }
    for format in DATE_FORMATS {
        if let Ok(date) = Date::strptime(format, input) {
            return Ok(Some(interpret_civil(date.at(0, 0, 0, 0), tz)?));
        }
    }
    Ok(None)
}

fn interpret_civil(datetime: DateTime, tz: &TimeZone) -> Result<Timestamp, ToolError> {
    tz.to_zoned_datetime(datetime)
        .map(|zoned| zoned.timestamp())
        .map_err(|err| {
            unsupported(format!("该本地时间在指定时区不存在或存在歧义：{err}"))
                .with_hint("请改用带偏移量的 ISO 8601 形式，例如 2024-01-02T03:04:05+08:00")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use devtools_core::ToolErrorKind;

    fn request(input: &str, timezone: &str, unit: TimestampUnit) -> DateTimeConverterRequest {
        DateTimeConverterRequest {
            input: input.to_string(),
            timezone: timezone.to_string(),
            timestamp_unit: unit,
        }
    }

    const SAMPLE_SECONDS: i64 = 1_704_164_645; // 2024-01-02T03:04:05Z
    const SAMPLE_MILLIS: i64 = 1_704_164_645_123;

    #[test]
    fn seconds_timestamp_auto_detected() {
        let resp = convert(&request("1704164645", "UTC", TimestampUnit::Auto)).unwrap();
        assert!(resp.is_timestamp_input);
        assert_eq!(resp.timestamp_unit, Some("seconds"));
        assert_eq!(resp.unix_seconds, SAMPLE_SECONDS);
        assert!(resp.utc.starts_with("2024-01-02T03:04:05"), "{}", resp.utc);
        assert_eq!(resp.weekday, "Tuesday");
        assert_eq!(resp.iso_week, "2024-W01-2");
    }

    #[test]
    fn millis_micros_nanos_auto_detected() {
        let millis = convert(&request("1704164645123", "UTC", TimestampUnit::Auto)).unwrap();
        assert_eq!(millis.timestamp_unit, Some("milliseconds"));
        assert_eq!(millis.unix_millis, SAMPLE_MILLIS);

        let micros = convert(&request("1704164645123456", "UTC", TimestampUnit::Auto)).unwrap();
        assert_eq!(micros.timestamp_unit, Some("microseconds"));
        assert_eq!(micros.unix_micros, 1_704_164_645_123_456);

        let nanos = convert(&request("1704164645123456789", "UTC", TimestampUnit::Auto)).unwrap();
        assert_eq!(nanos.timestamp_unit, Some("nanoseconds"));
        assert_eq!(nanos.unix_nanos, 1_704_164_645_123_456_789);
    }

    #[test]
    fn explicit_unit_overrides_auto_detection() {
        let resp = convert(&request("1704164645123", "UTC", TimestampUnit::Millis)).unwrap();
        assert_eq!(resp.unix_millis, SAMPLE_MILLIS);
        let resp = convert(&request("1704164645", "UTC", TimestampUnit::Seconds)).unwrap();
        assert_eq!(resp.unix_seconds, SAMPLE_SECONDS);
    }

    #[test]
    fn iso_rfc3339_parses_with_offset() {
        let resp = convert(&request(
            "2024-01-02T11:04:05+08:00",
            "UTC",
            TimestampUnit::Auto,
        ))
        .unwrap();
        assert!(!resp.is_timestamp_input);
        assert_eq!(resp.unix_seconds, SAMPLE_SECONDS);
    }

    #[test]
    fn rfc2822_parses() {
        let resp = convert(&request(
            "Tue, 02 Jan 2024 03:04:05 +0000",
            "UTC",
            TimestampUnit::Auto,
        ))
        .unwrap();
        assert_eq!(resp.unix_seconds, SAMPLE_SECONDS);
    }

    #[test]
    fn naive_datetime_is_interpreted_in_target_zone() {
        let resp = convert(&request(
            "2024-01-02 11:04:05",
            "Asia/Shanghai",
            TimestampUnit::Auto,
        ))
        .unwrap();
        assert_eq!(resp.unix_seconds, SAMPLE_SECONDS);
        assert!(resp.timezone.contains("11:04:05"), "{}", resp.timezone);
        assert!(resp.utc.contains("03:04:05"), "{}", resp.utc);
    }

    #[test]
    fn iana_timezone_conversion_matches_offset() {
        let resp = convert(&request(
            "1704164645",
            "America/New_York",
            TimestampUnit::Auto,
        ))
        .unwrap();
        assert!(resp.timezone.contains("22:04:05"), "{}", resp.timezone);
        assert!(resp.timezone.ends_with("-05:00"), "{}", resp.timezone);
    }

    #[test]
    fn common_date_formats_parse() {
        for input in [
            "2024-01-02",
            "2024/01/02",
            "January 2, 2024",
            "Jan 2, 2024",
            "02 January 2024",
        ] {
            let resp = convert(&request(input, "UTC", TimestampUnit::Auto)).unwrap();
            assert_eq!(resp.utc, "2024-01-02T00:00:00+00:00", "input {input}");
        }
    }

    #[test]
    fn unknown_timezone_is_rejected() {
        let err = convert(&request("1704164645", "Mars/Olympus", TimestampUnit::Auto)).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
        assert!(err.hint.is_some());
    }

    #[test]
    fn garbage_input_is_rejected() {
        let err = convert(&request("not a date", "UTC", TimestampUnit::Auto)).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
    }

    #[test]
    fn negative_timestamp_supports_pre_epoch() {
        let resp = convert(&request("-1", "UTC", TimestampUnit::Auto)).unwrap();
        assert_eq!(resp.utc, "1969-12-31T23:59:59+00:00");
    }

    #[test]
    fn oversized_input_is_rejected() {
        let huge = "1".repeat(2048);
        let err = convert(&request(&huge, "UTC", TimestampUnit::Auto)).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::SizeLimitExceeded);
    }
}
