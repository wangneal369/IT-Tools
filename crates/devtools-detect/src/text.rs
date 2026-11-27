use std::{net::Ipv4Addr, sync::LazyLock};

use base64::{
    Engine,
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD},
};
use devtools_core::{Confidence, DataType};
use regex::Regex;
use serde_json::Value;

pub(crate) fn detect(kind: DataType, value: &str, prefix_only: bool) -> Option<Confidence> {
    use DataType as D;
    let matched = match kind {
        D::Jwt => jwt(value, prefix_only),
        D::CertificatePem => certificate_pem(value, prefix_only),
        D::Base64Image => base64_image(value, prefix_only),
        D::GZipBase64 => gzip(value),
        D::Json => json(value).is_some(),
        D::XmlHtml => markup(value),
        D::Yaml => yaml(value),
        D::Toml => value.contains('=') && value.parse::<toml::Table>().is_ok_and(|t| !t.is_empty()),
        D::Url => url::Url::parse(value).is_ok_and(|u| {
            matches!(u.scheme(), "http" | "https" | "ftp" | "file" | "mailto")
                && (u.has_host() || matches!(u.scheme(), "file" | "mailto"))
        }),
        D::UrlEncoded => percent_encoded(value),
        D::DateTime => datetime(value),
        D::UnixTimestamp => timestamp(value),
        D::Cron => cron(value),
        D::Ipv4Cidr => ipv4(value),
        D::Color => {
            (value.starts_with('#') || value.contains('(')) && csscolorparser::parse(value).is_ok()
        }
        D::Base64Text => {
            value.len() >= 8 && decode(value).is_some_and(|bytes| String::from_utf8(bytes).is_ok())
        }
        D::HtmlUnicode => unicode(value),
        D::EscapedText => escaped(value),
        D::Sql => sql(value),
        D::Chmod => chmod(value),
        D::NumberBase => return number(value),
        D::HashDigest => {
            matches!(value.len(), 32 | 40 | 64 | 128)
                && value.bytes().all(|b| b.is_ascii_hexdigit())
        }
        _ => false,
    };
    matched.then_some(match kind {
        D::Json | D::Jwt | D::CertificatePem | D::Base64Image | D::HashDigest => Confidence::Exact,
        D::Yaml | D::EscapedText | D::Sql | D::HtmlUnicode | D::Chmod | D::UrlEncoded => {
            Confidence::Medium
        }
        _ => Confidence::High,
    })
}

fn decode(value: &str) -> Option<Vec<u8>> {
    [STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD]
        .iter()
        .find_map(|engine| engine.decode(value).ok())
}

fn jwt(value: &str, prefix_only: bool) -> bool {
    let parts: Vec<_> = value.split('.').take(4).collect();
    if parts.len() != 3
        || parts[0].len() > 8192
        || parts.iter().any(|p| {
            p.is_empty()
                || !p
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        })
    {
        return false;
    }
    let Ok(header) = URL_SAFE_NO_PAD.decode(parts[0]) else {
        return false;
    };
    let Ok(Value::Object(header)) = serde_json::from_slice::<Value>(&header) else {
        return false;
    };
    if !header.get("alg").is_some_and(Value::is_string)
        && !header.get("typ").is_some_and(Value::is_string)
    {
        return false;
    }
    // 前缀嗅探只验证头；全量模式验证三个 base64url 段与 JSON claims。
    prefix_only
        || (URL_SAFE_NO_PAD.decode(parts[2]).is_ok()
            && URL_SAFE_NO_PAD
                .decode(parts[1])
                .ok()
                .is_some_and(|p| serde_json::from_slice::<Value>(&p).is_ok_and(|v| v.is_object())))
}

pub(crate) fn certificate_pem(value: &str, prefix_only: bool) -> bool {
    let privacy = crate::assess_privacy(value);
    if privacy.private_key || privacy.certificate_request {
        return false;
    }
    let Some(rest) = value.strip_prefix("-----BEGIN CERTIFICATE-----") else {
        return false;
    };
    if prefix_only {
        return true;
    }
    let Some((body, _)) = rest.split_once("-----END CERTIFICATE-----") else {
        return false;
    };
    let body: String = body.chars().filter(|c| !c.is_ascii_whitespace()).collect();
    STANDARD.decode(body).is_ok_and(|b| certificate_der(&b))
}

// DER 只嗅探固定深度 TLV，既不递归也不解密；排除 PKCS#1/PKCS#8/SEC1 私钥和 CSR。
fn tlv(bytes: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, bytes) = bytes.split_first()?;
    let (&length, bytes) = bytes.split_first()?;
    let (length, body) = if length < 128 {
        (usize::from(length), bytes)
    } else {
        let count = usize::from(length & 127);
        if count == 0 || count > 4 || bytes.len() < count {
            return None;
        }
        let length = bytes[..count].iter().try_fold(0usize, |n, b| {
            n.checked_mul(256)?.checked_add(usize::from(*b))
        })?;
        (length, &bytes[count..])
    };
    (length <= body.len()).then(|| (tag, &body[..length], &body[length..]))
}

pub(crate) fn certificate_der(bytes: &[u8]) -> bool {
    let Some((0x30, body, [])) = tlv(bytes) else {
        return false;
    };
    let Some((0x30, tbs, body)) = tlv(body) else {
        return false;
    };
    let Some((0x30, _, body)) = tlv(body) else {
        return false;
    };
    let Some((0x03, _, [])) = tlv(body) else {
        return false;
    };
    let tbs = if tbs.first() == Some(&0xa0) {
        let Some((_, _, rest)) = tlv(tbs) else {
            return false;
        };
        rest
    } else {
        tbs
    };
    let Some((0x02, _, rest)) = tlv(tbs) else {
        return false;
    };
    let Some((0x30, _, rest)) = tlv(rest) else {
        return false;
    };
    let Some((0x30, _, rest)) = tlv(rest) else {
        return false;
    };
    let Some((0x30, validity, rest)) = tlv(rest) else {
        return false;
    };
    let Some((0x17 | 0x18, _, validity)) = tlv(validity) else {
        return false;
    };
    if !matches!(tlv(validity), Some((0x17 | 0x18, _, []))) {
        return false;
    }
    let Some((0x30, _, rest)) = tlv(rest) else {
        return false;
    };
    matches!(tlv(rest), Some((0x30, _, _)))
}

pub(crate) fn pkcs12(bytes: &[u8]) -> bool {
    let Some((0x30, body, [])) = tlv(bytes) else {
        return false;
    };
    let Some((0x02, [3], body)) = tlv(body) else {
        return false;
    };
    let Some((0x30, content, _)) = tlv(body) else {
        return false;
    };
    // PKCS#7 data / signedData ContentInfo，PFX 内封装的密钥不作明文展示。
    matches!(
        tlv(content),
        Some((0x06, [0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 7, 1 | 2], _))
    )
}

fn base64_image(value: &str, prefix_only: bool) -> bool {
    let Some((mime, body)) = value
        .strip_prefix("data:image/")
        .and_then(|v| v.split_once(";base64,"))
    else {
        return false;
    };
    if mime.is_empty()
        || !mime
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b))
    {
        return false;
    }
    let bytes = if prefix_only {
        body.get(..body.len().min(256) / 4 * 4).and_then(decode)
    } else {
        decode(body)
    };
    bytes.is_some_and(|b| imagesize::blob_size(&b).is_ok())
}

fn gzip(value: &str) -> bool {
    if !value.starts_with("H4sI") {
        return false;
    }
    // 不解压，不构建与压缩载荷等大的缓冲区。
    value
        .get(..12)
        .and_then(decode)
        .is_some_and(|b| b.starts_with(&[0x1f, 0x8b, 8]))
}

fn json(value: &str) -> Option<Value> {
    if !value.starts_with(['{', '[']) {
        return None;
    }
    serde_json::from_str(value)
        .ok()
        .filter(|v: &Value| v.is_object() || v.is_array())
}

pub(crate) fn json_array(value: &str) -> bool {
    let Some(Value::Array(items)) = json(value) else {
        return false;
    };
    let Some(first) = items.first() else {
        return false;
    };
    let tag = |v: &Value| match v {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    };
    items.iter().all(|item| tag(item) == tag(first))
}

fn markup(value: &str) -> bool {
    static TAG: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)^\s*(?:<\?xml[^>]*>\s*)?(?:<!doctype\s+html[^>]*>\s*)?<([a-z][a-z0-9:_-]*)(?:\s[^<>]*)?\s*/?>").expect("固定标签表达式")
    });
    // 格式化工具负责完整语法诊断；嗅探必须有明确起始标签和闭合标签/自闭合。
    TAG.captures(value).is_some_and(|caps| {
        value.ends_with("/>")
            || value
                .to_ascii_lowercase()
                .contains(&format!("</{}>", caps[1].to_ascii_lowercase()))
            || value.to_ascii_lowercase().starts_with("<!doctype html")
    })
}

fn yaml(value: &str) -> bool {
    if !(value.contains(": ") || value.starts_with("- ") || value.starts_with("---\n")) {
        return false;
    }
    // 低成本结构上限先于递归解析；禁止别名展开和外部 include。
    if value.lines().count() > 4096
        || value
            .lines()
            .any(|l| l.bytes().take_while(|b| *b == b' ').count() > 64)
        || value.contains(['&', '*', '!'])
    {
        return false;
    }
    let mut options = serde_saphyr::Options::default();
    options.budget = serde_saphyr::budget! { max_depth: 32, max_events: 16384, max_documents: 1 };
    serde_saphyr::from_str_with_options::<Value>(value, options)
        .is_ok_and(|v| v.is_object() || v.is_array())
}

fn percent_encoded(value: &str) -> bool {
    let mut found = false;
    let mut bytes = value.bytes();
    while let Some(b) = bytes.next() {
        if b == b'%' {
            if !bytes.next().is_some_and(|b| b.is_ascii_hexdigit())
                || !bytes.next().is_some_and(|b| b.is_ascii_hexdigit())
            {
                return false;
            }
            found = true;
        }
    }
    found
}

fn datetime(value: &str) -> bool {
    if value.len() > 128 {
        return false;
    }
    value.parse::<jiff::Timestamp>().is_ok()
        || value.parse::<jiff::civil::DateTime>().is_ok()
        || value.parse::<jiff::civil::Date>().is_ok()
        || jiff::civil::DateTime::strptime("%Y-%m-%d %H:%M:%S", value).is_ok()
        || jiff::civil::Date::strptime("%Y/%m/%d", value).is_ok()
        || jiff::Timestamp::strptime("%a, %d %b %Y %H:%M:%S %z", value).is_ok()
}

fn timestamp(value: &str) -> bool {
    if !matches!(value.len(), 10 | 13) || !value.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let Ok(n) = value.parse::<i64>() else {
        return false;
    };
    let seconds = if value.len() == 13 { n / 1000 } else { n };
    (157_766_400..4_133_980_800).contains(&seconds)
}

fn cron(value: &str) -> bool {
    if value.len() > 256 || !matches!(value.split_whitespace().count(), 5 | 6) {
        return false;
    }
    croner::parser::CronParser::builder()
        .seconds(croner::parser::Seconds::Optional)
        .build()
        .parse(value)
        .is_ok()
}

fn ipv4(value: &str) -> bool {
    if let Ok(_ip) = value.parse::<Ipv4Addr>() {
        return true;
    }
    if let Some((ip, prefix)) = value.split_once('/') {
        return ip.parse::<Ipv4Addr>().is_ok() && prefix.parse::<u8>().is_ok_and(|p| p <= 32);
    }
    if let Some((start, end)) = value.split_once('-') {
        return start
            .trim()
            .parse::<Ipv4Addr>()
            .ok()
            .zip(end.trim().parse::<Ipv4Addr>().ok())
            .is_some_and(|(a, b)| a <= b);
    }
    let parts: Vec<_> = value.split_whitespace().collect();
    if parts.len() != 2 || parts[0].parse::<Ipv4Addr>().is_err() {
        return false;
    }
    parts[1].parse::<Ipv4Addr>().is_ok_and(|mask| {
        let inverse = !u32::from(mask);
        inverse & inverse.wrapping_add(1) == 0
    })
}

fn unicode(value: &str) -> bool {
    static UNICODE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:&(?:[A-Za-z][A-Za-z0-9]{1,30}|#[0-9]{1,7}|#x[0-9a-fA-F]{1,6});|\\u[0-9a-fA-F]{4}|U\+[0-9a-fA-F]{4,6}\b|\\[0-9a-fA-F]{1,6}(?:\s|$))").expect("固定 Unicode 表达式")
    });
    UNICODE.is_match(value)
}

fn escaped(value: &str) -> bool {
    let bytes = value.as_bytes();
    if value.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\')
    {
        return false;
    }
    bytes
        .windows(2)
        .any(|pair| pair[0] == b'\\' && b"nrt0\\\"'".contains(&pair[1]))
}

fn sql(value: &str) -> bool {
    static SQL: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?is)^(?:select\s+.+\s+from\s+\S+|insert\s+into\s+\S+|update\s+\S+\s+set\s+|delete\s+from\s+\S+|create\s+(?:table|view|index)\s+\S+|with\s+\w+\s+as\s*\().*").expect("固定 SQL 表达式")
    });
    SQL.is_match(value)
}

fn chmod(value: &str) -> bool {
    static SYMBOLIC: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?:[r-][w-][xSs-][r-][w-][xSs-][r-][w-][xTt-]|[ugoa]*[+\-=][rwxXstugo]+(?:,[ugoa]*[+\-=][rwxXstugo]+)*)$").expect("固定权限表达式")
    });
    (matches!(value.len(), 3 | 4) && value.bytes().all(|b| (b'0'..=b'7').contains(&b)))
        || SYMBOLIC.is_match(value)
}

fn number(value: &str) -> Option<Confidence> {
    for (prefix, radix) in [
        ("0x", 16),
        ("0X", 16),
        ("0b", 2),
        ("0B", 2),
        ("0o", 8),
        ("0O", 8),
    ] {
        if let Some(digits) = value.strip_prefix(prefix) {
            return (!digits.is_empty()
                && digits.len() <= 4096
                && digits.chars().all(|c| c.is_digit(radix)))
            .then_some(Confidence::High);
        }
    }
    let digits = value.strip_prefix(['-', '+']).unwrap_or(value);
    (!digits.is_empty() && digits.len() <= 1234 && digits.bytes().all(|b| b.is_ascii_digit()))
        .then_some(Confidence::Low)
}
