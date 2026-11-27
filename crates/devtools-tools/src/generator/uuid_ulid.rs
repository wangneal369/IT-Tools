//! #14 UUID / ULID 生成器。
//!
//! v4（默认）/ v7 / v1 / NIL / ULID，批量 1–1000；大小写、连字符、
//! 换行、JSON、引号输出，并提供单行版本与时间戳反解。

use uuid::Uuid;

use crate::util::{check_cancel, invalid_syntax, unsupported};

use devtools_core::{CancelFlag, ToolError};

/// 生成数量上限。
pub const MAX_BATCH: usize = 1000;

/// 标识类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdKind {
    V4,
    V7,
    V1,
    Nil,
    Ulid,
}

/// 输出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdOutputFormat {
    /// 每个值一行（`one_per_line` 为假时用 `, ` 连接）。
    Plain,
    /// JSON 字符串数组。
    Json,
    /// 每个值带双引号。
    Quoted,
}

/// 单次生成请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UuidUlidRequest {
    pub kind: IdKind,
    /// 1–1000。
    pub count: usize,
    pub uppercase: bool,
    /// UUID 是否保留连字符（ULID 恒为 26 字符）。
    pub hyphens: bool,
    pub format: IdOutputFormat,
    pub one_per_line: bool,
}

/// 生成结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UuidUlidResponse {
    pub values: Vec<String>,
    pub output: String,
}

/// 标识反解信息。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdInfo {
    /// `"uuid"` 或 `"ulid"`。
    pub format: String,
    pub version: Option<u8>,
    pub variant: Option<String>,
    pub is_nil: bool,
    /// RFC 3339 UTC 时间戳（v1 / v7 / ULID）。
    pub timestamp: Option<String>,
    pub timestamp_unix_ms: Option<i64>,
}

/// 生成 UUID / ULID。
pub fn generate(
    req: &UuidUlidRequest,
    cancel: Option<&CancelFlag>,
) -> Result<UuidUlidResponse, ToolError> {
    if !(1..=MAX_BATCH).contains(&req.count) {
        return Err(unsupported(format!(
            "批量数量必须在 1–{MAX_BATCH} 之间，实际为 {}",
            req.count
        )));
    }
    let mut values = Vec::with_capacity(req.count);
    for _ in 0..req.count {
        check_cancel(cancel)?;
        values.push(generate_one(req)?);
    }
    let output = render_output(&values, req.format, req.one_per_line);
    Ok(UuidUlidResponse { values, output })
}

fn generate_one(req: &UuidUlidRequest) -> Result<String, ToolError> {
    match req.kind {
        IdKind::Ulid => {
            let text = ulid::Ulid::generate().to_string();
            Ok(if req.uppercase {
                text.to_uppercase()
            } else {
                text.to_lowercase()
            })
        }
        kind => {
            let uuid = match kind {
                IdKind::V4 => Uuid::new_v4(),
                IdKind::V7 => Uuid::now_v7(),
                IdKind::V1 => {
                    let mut node = [0u8; 6];
                    getrandom::fill(&mut node)
                        .map_err(|err| unsupported(format!("系统随机源不可用：{err}")))?;
                    // RFC 4122：多播位必须置 1，避免与真实 MAC 冲突。
                    node[0] |= 0x01;
                    Uuid::now_v1(&node)
                }
                IdKind::Nil => Uuid::nil(),
                IdKind::Ulid => unreachable!("handled above"),
            };
            let text = if req.hyphens {
                uuid.hyphenated().to_string()
            } else {
                uuid.simple().to_string()
            };
            Ok(if req.uppercase {
                text.to_uppercase()
            } else {
                text
            })
        }
    }
}

fn render_output(values: &[String], format: IdOutputFormat, one_per_line: bool) -> String {
    let separator = if one_per_line { "\n" } else { ", " };
    match format {
        IdOutputFormat::Plain => values.join(separator),
        IdOutputFormat::Quoted => values
            .iter()
            .map(|value| format!("\"{value}\""))
            .collect::<Vec<_>>()
            .join(separator),
        IdOutputFormat::Json => serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string()),
    }
}

/// 单行反解 UUID / ULID 的版本与时间戳。
pub fn inspect(input: &str) -> Result<IdInfo, ToolError> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err(invalid_syntax("标识输入为空"));
    }
    if trimmed.len() == 26
        && let Ok(ulid) = ulid::Ulid::from_string(&trimmed.to_uppercase())
    {
        let milliseconds = i64::try_from(ulid.timestamp_ms()).ok();
        return Ok(IdInfo {
            format: "ulid".to_string(),
            version: None,
            variant: None,
            is_nil: ulid == ulid::Ulid::nil(),
            timestamp: milliseconds.and_then(format_timestamp),
            timestamp_unix_ms: milliseconds,
        });
    }
    let uuid = Uuid::parse_str(trimmed)
        .map_err(|err| invalid_syntax(format!("无法解析 UUID / ULID：{err}")))?;
    let version = Some(uuid.get_version_num() as u8);
    let timestamp_unix_ms = match version {
        Some(1) => uuid.get_timestamp().map(|timestamp| {
            let (seconds, nanos) = timestamp.to_unix();
            (seconds as i64) * 1000 + i64::from(nanos) / 1_000_000
        }),
        Some(7) => Some(((uuid.as_u128() >> 80) & 0xFFFF_FFFF_FFFF) as i64),
        _ => None,
    };
    Ok(IdInfo {
        format: "uuid".to_string(),
        version,
        variant: Some(format!("{:?}", uuid.get_variant())),
        is_nil: uuid.is_nil(),
        timestamp: timestamp_unix_ms.and_then(format_timestamp),
        timestamp_unix_ms,
    })
}

fn format_timestamp(milliseconds: i64) -> Option<String> {
    jiff::Timestamp::from_millisecond(milliseconds)
        .ok()
        .map(|timestamp| timestamp.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use devtools_core::ToolErrorKind;

    fn request(kind: IdKind, count: usize) -> UuidUlidRequest {
        UuidUlidRequest {
            kind,
            count,
            uppercase: false,
            hyphens: true,
            format: IdOutputFormat::Plain,
            one_per_line: true,
        }
    }

    #[test]
    fn v4_is_default_shape() {
        let resp = generate(&request(IdKind::V4, 3), None).unwrap();
        assert_eq!(resp.values.len(), 3);
        for value in &resp.values {
            assert_eq!(value.len(), 36);
            assert!(value.contains('-'));
            assert_eq!(inspect(value).unwrap().version, Some(4));
        }
    }

    #[test]
    fn v7_has_version_and_timestamp() {
        let resp = generate(&request(IdKind::V7, 1), None).unwrap();
        let info = inspect(&resp.values[0]).unwrap();
        assert_eq!(info.version, Some(7));
        assert!(info.timestamp_unix_ms.unwrap() > 1_600_000_000_000);
        assert!(info.timestamp.is_some());
    }

    #[test]
    fn v1_has_version_and_timestamp() {
        let resp = generate(&request(IdKind::V1, 1), None).unwrap();
        let info = inspect(&resp.values[0]).unwrap();
        assert_eq!(info.version, Some(1));
        assert!(info.timestamp_unix_ms.unwrap() > 1_600_000_000_000);
    }

    #[test]
    fn nil_uuid_is_all_zero() {
        let resp = generate(&request(IdKind::Nil, 1), None).unwrap();
        assert_eq!(resp.values[0], "00000000-0000-0000-0000-000000000000");
        assert!(inspect(&resp.values[0]).unwrap().is_nil);
    }

    #[test]
    fn ulid_is_26_crockford_chars_with_timestamp() {
        let resp = generate(&request(IdKind::Ulid, 2), None).unwrap();
        assert_eq!(resp.values[0].len(), 26);
        let info = inspect(&resp.values[0]).unwrap();
        assert_eq!(info.format, "ulid");
        assert!(info.timestamp_unix_ms.unwrap() > 1_600_000_000_000);
    }

    #[test]
    fn uppercase_and_hyphenless_formatting() {
        let mut req = request(IdKind::V4, 1);
        req.uppercase = true;
        req.hyphens = false;
        let resp = generate(&req, None).unwrap();
        let value = &resp.values[0];
        assert_eq!(value.len(), 32);
        assert!(value.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(value, &value.to_uppercase());
        let info = inspect(value).unwrap();
        assert_eq!(info.version, Some(4));
    }

    #[test]
    fn json_and_quoted_outputs() {
        let mut req = request(IdKind::V4, 2);
        req.format = IdOutputFormat::Json;
        let resp = generate(&req, None).unwrap();
        let parsed: Vec<String> = serde_json::from_str(&resp.output).unwrap();
        assert_eq!(parsed, resp.values);

        let mut req = request(IdKind::V4, 2);
        req.format = IdOutputFormat::Quoted;
        req.one_per_line = false;
        let resp = generate(&req, None).unwrap();
        assert_eq!(resp.output.matches('"').count(), 4);
        assert!(resp.output.contains(", "));
    }

    #[test]
    fn batch_limits_are_enforced() {
        let err = generate(&request(IdKind::V4, 0), None).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
        let err = generate(&request(IdKind::V4, MAX_BATCH + 1), None).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
    }

    #[test]
    fn cancellation_stops_batch() {
        let flag = CancelFlag::new();
        flag.cancel();
        let err = generate(&request(IdKind::V7, 10), Some(&flag)).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Cancelled);
    }

    #[test]
    fn inspect_rejects_garbage() {
        let err = inspect("not-an-id").unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
    }
}
