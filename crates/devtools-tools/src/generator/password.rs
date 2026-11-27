//! #15 密码与随机 Token 生成器。
//!
//! 密码长度 4–128、自定义符号、排除易混字符、每个已启用字符类至少保底 1 位；
//! Token 支持 16/32/64 字节与 Hex / Base64URL / Base32 编码；批量 1–100。
//! 输出默认标记 `sensitive_output = true`。

use std::collections::HashMap;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::seq::{IndexedRandom, SliceRandom};

use crate::util::{check_cancel, unsupported};

use devtools_core::{CancelFlag, ToolError};

/// 批量上限。
pub const MAX_BATCH: usize = 100;
/// 密码长度上限。
pub const MAX_PASSWORD_LENGTH: usize = 128;
/// 密码长度下限。
pub const MIN_PASSWORD_LENGTH: usize = 4;

const LOWERCASE: &str = "abcdefghijklmnopqrstuvwxyz";
const UPPERCASE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
const DIGITS: &str = "0123456789";
const DEFAULT_SYMBOLS: &str = "!@#$%^&*()-_=+[]{};:,.?/~";
const AMBIGUOUS: &str = "0Oo1lI|";

/// 密码选项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordOptions {
    pub length: usize,
    pub lowercase: bool,
    pub uppercase: bool,
    pub digits: bool,
    pub symbols: bool,
    /// 自定义符号集（`symbols = true` 时生效）。
    pub custom_symbols: Option<String>,
    /// 排除易混字符 `0 O o 1 l I |`。
    pub exclude_ambiguous: bool,
}

impl Default for PasswordOptions {
    fn default() -> Self {
        Self {
            length: 16,
            lowercase: true,
            uppercase: true,
            digits: true,
            symbols: true,
            custom_symbols: None,
            exclude_ambiguous: true,
        }
    }
}

/// Token 编码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenEncoding {
    Hex,
    Base64Url,
    Base32,
}

/// 生成种类。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretKind {
    Password(PasswordOptions),
    Token {
        /// 仅支持 16 / 32 / 64 字节。
        bytes: usize,
        encoding: TokenEncoding,
    },
}

/// 输出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretOutput {
    Plain,
    Json,
}

/// 单次请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasswordTokenRequest {
    pub kind: SecretKind,
    /// 1–100。
    pub count: usize,
    pub output: SecretOutput,
}

/// 强度报告（仅密码）。
#[derive(Debug, Clone, PartialEq)]
pub struct StrengthReport {
    /// 字符分布香农熵（总比特）。
    pub shannon_bits: f64,
    /// zxcvbn 评分 0–4。
    pub zxcvbn_score: u8,
    /// zxcvbn 离线快速破解时间描述。
    pub crack_time: String,
}

/// 生成结果。
#[derive(Debug, Clone, PartialEq)]
pub struct PasswordTokenResponse {
    pub values: Vec<String>,
    pub output: String,
    /// 恒为 `true`：输出需由 UI 标记为敏感内容。
    pub sensitive_output: bool,
    pub strength: Vec<StrengthReport>,
}

/// 执行密码 / Token 生成。
pub fn run(
    req: &PasswordTokenRequest,
    cancel: Option<&CancelFlag>,
) -> Result<PasswordTokenResponse, ToolError> {
    if !(1..=MAX_BATCH).contains(&req.count) {
        return Err(unsupported(format!(
            "批量数量必须在 1–{MAX_BATCH} 之间，实际为 {}",
            req.count
        )));
    }
    let mut values = Vec::with_capacity(req.count);
    let mut strength = Vec::new();
    for _ in 0..req.count {
        check_cancel(cancel)?;
        match &req.kind {
            SecretKind::Password(options) => {
                let password = generate_password(options)?;
                strength.push(strength_report(&password));
                values.push(password);
            }
            SecretKind::Token { bytes, encoding } => {
                values.push(generate_token(*bytes, *encoding)?)
            }
        }
    }
    let output = match req.output {
        SecretOutput::Plain => values.join("\n"),
        SecretOutput::Json => serde_json::to_string(&values)
            .map_err(|err| crate::util::internal(format!("JSON 序列化失败：{err}")))?,
    };
    Ok(PasswordTokenResponse {
        values,
        output,
        sensitive_output: true,
        strength,
    })
}

fn generate_password(options: &PasswordOptions) -> Result<String, ToolError> {
    if !(MIN_PASSWORD_LENGTH..=MAX_PASSWORD_LENGTH).contains(&options.length) {
        return Err(unsupported(format!(
            "密码长度必须在 {MIN_PASSWORD_LENGTH}–{MAX_PASSWORD_LENGTH} 之间，实际为 {}",
            options.length
        )));
    }
    let mut classes: Vec<Vec<char>> = Vec::new();
    if options.lowercase {
        classes.push(charset(LOWERCASE, options.exclude_ambiguous));
    }
    if options.uppercase {
        classes.push(charset(UPPERCASE, options.exclude_ambiguous));
    }
    if options.digits {
        classes.push(charset(DIGITS, options.exclude_ambiguous));
    }
    if options.symbols {
        let symbols = options.custom_symbols.as_deref().unwrap_or(DEFAULT_SYMBOLS);
        if symbols.is_empty() {
            return Err(unsupported("自定义符号集不能为空"));
        }
        let set = charset(symbols, options.exclude_ambiguous);
        if set.is_empty() {
            return Err(unsupported("排除易混字符后没有可用符号"));
        }
        classes.push(set);
    }
    if classes.is_empty() {
        return Err(
            unsupported("至少需要启用一个字符类").with_hint("请勾选小写/大写/数字/符号之一")
        );
    }
    if options.length < classes.len() {
        return Err(unsupported(format!(
            "长度 {} 无法保证每个字符类至少 1 位（已启用 {} 类）",
            options.length,
            classes.len()
        ))
        .with_hint("请增加长度或减少字符类"));
    }

    let mut rng = rand::rng();
    let pool: Vec<char> = classes.iter().flatten().copied().collect();
    let mut password: Vec<char> = Vec::with_capacity(options.length);
    for class in &classes {
        if let Some(ch) = class.choose(&mut rng) {
            password.push(*ch);
        }
    }
    while password.len() < options.length {
        if let Some(ch) = pool.choose(&mut rng) {
            password.push(*ch);
        }
    }
    password.shuffle(&mut rng);
    Ok(password.into_iter().collect())
}

fn charset(source: &str, exclude_ambiguous: bool) -> Vec<char> {
    source
        .chars()
        .filter(|ch| !exclude_ambiguous || !AMBIGUOUS.contains(*ch))
        .collect()
}

fn generate_token(bytes: usize, encoding: TokenEncoding) -> Result<String, ToolError> {
    if !matches!(bytes, 16 | 32 | 64) {
        return Err(unsupported(format!(
            "Token 长度仅支持 16/32/64 字节，实际为 {bytes}"
        )));
    }
    let mut buffer = vec![0u8; bytes];
    getrandom::fill(&mut buffer).map_err(|err| unsupported(format!("系统随机源不可用：{err}")))?;
    Ok(match encoding {
        TokenEncoding::Hex => buffer.iter().map(|byte| format!("{byte:02x}")).collect(),
        TokenEncoding::Base64Url => URL_SAFE_NO_PAD.encode(&buffer),
        TokenEncoding::Base32 => base32_encode(&buffer),
    })
}

fn base32_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::with_capacity(data.len().div_ceil(5) * 8);
    let mut value = 0u32;
    let mut bits = 0u32;
    for byte in data {
        value = (value << 8) | u32::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(char::from(ALPHABET[((value >> bits) & 31) as usize]));
        }
    }
    if bits > 0 {
        out.push(char::from(ALPHABET[((value << (5 - bits)) & 31) as usize]));
    }
    out
}

fn strength_report(password: &str) -> StrengthReport {
    let entropy = zxcvbn::zxcvbn(password, &[]);
    StrengthReport {
        shannon_bits: (shannon_bits(password) * 100.0).round() / 100.0,
        zxcvbn_score: u8::from(entropy.score()),
        crack_time: entropy
            .crack_times()
            .offline_fast_hashing_1e10_per_second()
            .to_string(),
    }
}

fn shannon_bits(text: &str) -> f64 {
    let mut counts: HashMap<char, usize> = HashMap::new();
    for ch in text.chars() {
        *counts.entry(ch).or_insert(0) += 1;
    }
    let total = text.chars().count() as f64;
    if total == 0.0 {
        return 0.0;
    }
    let per_char: f64 = counts
        .values()
        .map(|count| {
            let probability = *count as f64 / total;
            -probability * probability.log2()
        })
        .sum();
    per_char * total
}

#[cfg(test)]
mod tests {
    use super::*;
    use devtools_core::ToolErrorKind;

    fn password_request(options: PasswordOptions, count: usize) -> PasswordTokenRequest {
        PasswordTokenRequest {
            kind: SecretKind::Password(options),
            count,
            output: SecretOutput::Plain,
        }
    }

    fn token_request(bytes: usize, encoding: TokenEncoding) -> PasswordTokenRequest {
        PasswordTokenRequest {
            kind: SecretKind::Token { bytes, encoding },
            count: 1,
            output: SecretOutput::Plain,
        }
    }

    #[test]
    fn every_enabled_class_is_represented() {
        let options = PasswordOptions {
            length: 8,
            ..PasswordOptions::default()
        };
        for _ in 0..20 {
            let resp = run(&password_request(options.clone(), 1), None).unwrap();
            let password = &resp.values[0];
            assert_eq!(password.chars().count(), 8);
            assert!(password.chars().any(|c| c.is_ascii_lowercase()));
            assert!(password.chars().any(|c| c.is_ascii_uppercase()));
            assert!(password.chars().any(|c| c.is_ascii_digit()));
            assert!(password.chars().any(|c| DEFAULT_SYMBOLS.contains(c)));
        }
    }

    #[test]
    fn minimum_length_still_covers_four_classes() {
        let options = PasswordOptions {
            length: 4,
            ..PasswordOptions::default()
        };
        let resp = run(&password_request(options, 1), None).unwrap();
        assert_eq!(resp.values[0].len(), 4);
    }

    #[test]
    fn ambiguous_characters_are_excluded() {
        let options = PasswordOptions {
            length: 64,
            exclude_ambiguous: true,
            symbols: false,
            ..PasswordOptions::default()
        };
        for _ in 0..10 {
            let resp = run(&password_request(options.clone(), 1), None).unwrap();
            assert!(resp.values[0].chars().all(|c| !AMBIGUOUS.contains(c)));
        }
    }

    #[test]
    fn custom_symbols_are_used() {
        let options = PasswordOptions {
            length: 10,
            lowercase: false,
            uppercase: false,
            digits: false,
            symbols: true,
            custom_symbols: Some("!@".to_string()),
            exclude_ambiguous: false,
        };
        let resp = run(&password_request(options, 1), None).unwrap();
        assert!(resp.values[0].chars().all(|c| c == '!' || c == '@'));
    }

    #[test]
    fn no_enabled_class_is_rejected() {
        let options = PasswordOptions {
            lowercase: false,
            uppercase: false,
            digits: false,
            symbols: false,
            ..PasswordOptions::default()
        };
        let err = run(&password_request(options, 1), None).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
    }

    #[test]
    fn length_bounds_are_enforced() {
        for length in [3, 129] {
            let options = PasswordOptions {
                length,
                ..PasswordOptions::default()
            };
            let err = run(&password_request(options, 1), None).unwrap_err();
            assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
        }
    }

    #[test]
    fn token_encodings_have_expected_shapes() {
        let hex = run(&token_request(16, TokenEncoding::Hex), None).unwrap();
        assert_eq!(hex.values[0].len(), 32);
        assert!(hex.values[0].chars().all(|c| c.is_ascii_hexdigit()));

        let b64 = run(&token_request(32, TokenEncoding::Base64Url), None).unwrap();
        assert_eq!(b64.values[0].len(), 43);
        assert!(!b64.values[0].contains('='));
        assert!(!b64.values[0].contains('+') && !b64.values[0].contains('/'));

        let b32 = run(&token_request(16, TokenEncoding::Base32), None).unwrap();
        assert_eq!(b32.values[0].len(), 26);
        assert!(
            b32.values[0]
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        );
    }

    #[test]
    fn invalid_token_length_is_rejected() {
        let err = run(&token_request(24, TokenEncoding::Hex), None).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
    }

    #[test]
    fn sensitive_output_flag_is_always_true() {
        let resp = run(&token_request(16, TokenEncoding::Hex), None).unwrap();
        assert!(resp.sensitive_output);
    }

    #[test]
    fn strength_reports_accompany_passwords() {
        let resp = run(&password_request(PasswordOptions::default(), 3), None).unwrap();
        assert_eq!(resp.strength.len(), 3);
        for report in &resp.strength {
            assert!(report.shannon_bits > 0.0);
            assert!(report.zxcvbn_score <= 4);
            assert!(!report.crack_time.is_empty());
        }
    }

    #[test]
    fn json_output_is_valid() {
        let mut req = password_request(
            PasswordOptions {
                length: 12,
                ..PasswordOptions::default()
            },
            2,
        );
        req.output = SecretOutput::Json;
        let resp = run(&req, None).unwrap();
        let parsed: Vec<String> = serde_json::from_str(&resp.output).unwrap();
        assert_eq!(parsed, resp.values);
    }

    #[test]
    fn batch_limits_and_cancellation() {
        let err = run(&password_request(PasswordOptions::default(), 101), None).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);

        let flag = CancelFlag::new();
        flag.cancel();
        let err = run(
            &password_request(PasswordOptions::default(), 5),
            Some(&flag),
        )
        .unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::Cancelled);
    }
}
