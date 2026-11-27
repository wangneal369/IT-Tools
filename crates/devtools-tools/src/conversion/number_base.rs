//! #3 进制转换（`num-bigint`，硬上限 4096 位）。
//!
//! 支持 2 / 8 / 10 / 16 四联同步与自定义 2–36 进制、大小写、前缀、
//! 四位分组空格；默认数学带符号，另可选用 8/16/32/64 位二进制补码解释。

use num_bigint::{BigInt, BigUint, Sign};
use num_traits::{One, Signed, Zero};

use crate::util::{invalid_syntax, size_limit, unsupported};

use devtools_core::ToolError;

/// 有符号语义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignedMode {
    /// 数学带符号整数（默认，支持任意位数到 4096 位）。
    Mathematical,
    /// 把输入视为指定位宽的无符号位模式，按二进制补码解释。
    Bits,
}

/// 单次转换请求。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberBaseConverterRequest {
    pub input: String,
    /// `0` 表示按 `0b` / `0o` / `0x` 前缀或默认十进制自动识别。
    pub from_base: u8,
    pub signed_mode: SignedMode,
    /// 补码位宽（8/16/32/64），`Bits` 模式必填。
    pub bit_width: Option<u8>,
    /// 额外输出的自定义进制（2–36）。
    pub custom_base: Option<u8>,
    pub uppercase: bool,
    pub prefixes: bool,
    pub group: bool,
}

/// 转换结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NumberBaseConverterResponse {
    pub binary: String,
    pub octal: String,
    pub decimal: String,
    pub hexadecimal: String,
    pub custom: Option<String>,
    /// 解析值的有效位数。
    pub bits: u64,
    /// 有符号十进制值。
    pub signed_value: String,
    /// 指定位宽下的补码十六进制位模式（含 `0x`）。
    pub two_complement: Option<String>,
    pub bit_width: Option<u8>,
}

/// 4096 位硬上限。
pub const MAX_BITS: u64 = 4096;

/// 执行进制转换。
pub fn convert(req: &NumberBaseConverterRequest) -> Result<NumberBaseConverterResponse, ToolError> {
    let cleaned = clean_input(&req.input);
    if cleaned.is_empty() {
        return Err(invalid_syntax("输入为空"));
    }
    if let Some(base) = req.custom_base
        && !(2..=36).contains(&base)
    {
        return Err(unsupported(format!(
            "自定义进制必须在 2–36 之间，实际为 {base}"
        )));
    }
    if let Some(width) = req.bit_width
        && !matches!(width, 8 | 16 | 32 | 64)
    {
        return Err(unsupported(format!(
            "补码位宽仅支持 8/16/32/64，实际为 {width}"
        )));
    }

    let (negative, digits, prefixed_base) = split_sign_and_prefix(&cleaned);
    let radix = resolve_radix(req.from_base, prefixed_base)?;
    if digits.is_empty() {
        return Err(invalid_syntax("缺少有效数字"));
    }
    // 任何进制的 4096 位都至少需要 4096 个字符以内的表示，先做长度拦截避免超大分配。
    if digits.len() > MAX_BITS as usize {
        return Err(size_limit(format!(
            "输入超过 {MAX_BITS} 位硬上限（{} 个字符）",
            digits.len()
        )));
    }

    let (value, bits, bit_width) = match req.signed_mode {
        SignedMode::Mathematical => {
            let magnitude = BigUint::parse_bytes(digits.as_bytes(), radix as u32)
                .ok_or_else(|| invalid_syntax(format!("存在不属于 {radix} 进制的字符")))?;
            let value = if negative {
                BigInt::from_biguint(Sign::Minus, magnitude)
            } else {
                BigInt::from_biguint(Sign::Plus, magnitude)
            };
            if value.bits() > MAX_BITS {
                return Err(size_limit(format!(
                    "数值超过 {MAX_BITS} 位硬上限（当前 {} 位）",
                    value.bits()
                )));
            }
            let bits = value.bits();
            if let Some(width) = req.bit_width {
                check_signed_range(&value, width)?;
            }
            (value, bits, req.bit_width)
        }
        SignedMode::Bits => {
            let width = req
                .bit_width
                .ok_or_else(|| unsupported("位模式解释必须指定程序位宽（8/16/32/64）"))?;
            let magnitude = BigUint::parse_bytes(digits.as_bytes(), radix as u32)
                .ok_or_else(|| invalid_syntax(format!("存在不属于 {radix} 进制的字符")))?;
            let modulus = BigUint::one() << (width as usize);
            if magnitude >= modulus {
                return Err(unsupported(format!(
                    "位数超出 {width} 位无符号范围（输入值过大）"
                )));
            }
            let signed = if magnitude.bits() == u64::from(width)
                && magnitude >= (BigUint::one() << (width as usize - 1))
            {
                BigInt::from_biguint(Sign::Plus, magnitude.clone())
                    - BigInt::from_biguint(Sign::Plus, modulus)
            } else {
                BigInt::from_biguint(Sign::Plus, magnitude)
            };
            (signed, u64::from(width), Some(width))
        }
    };

    let format = |radix: u32| format_number(&value, radix, req.uppercase, req.prefixes, req.group);

    let two_complement = bit_width.map(|width| {
        let modulus = BigInt::one() << (width as usize);
        let wrapped = ((&value % &modulus) + &modulus) % &modulus;
        let digits = wrapped
            .to_biguint()
            .expect("wrapped value is non-negative")
            .to_str_radix(16);
        let padded = format!("{digits:0>width$}", width = usize::from(width) / 4);
        let hex = if req.uppercase {
            padded.to_uppercase()
        } else {
            padded
        };
        format!("0x{hex}")
    });

    Ok(NumberBaseConverterResponse {
        binary: format(2),
        octal: format(8),
        decimal: format(10),
        hexadecimal: format(16),
        custom: req.custom_base.map(|base| format(u32::from(base))),
        bits,
        signed_value: value.to_string(),
        two_complement,
        bit_width,
    })
}

fn clean_input(input: &str) -> String {
    input
        .trim()
        .chars()
        .filter(|c| !matches!(c, ' ' | '_' | '\'' | ',' | '\t'))
        .collect()
}

fn split_sign_and_prefix(cleaned: &str) -> (bool, &str, Option<u8>) {
    let (negative, rest) = match cleaned.as_bytes().first() {
        Some(b'-') => (true, &cleaned[1..]),
        Some(b'+') => (false, &cleaned[1..]),
        _ => (false, cleaned),
    };
    if rest.len() >= 2 {
        let (head, tail) = rest.split_at(2);
        if head.eq_ignore_ascii_case("0b") {
            return (negative, tail, Some(2));
        }
        if head.eq_ignore_ascii_case("0o") {
            return (negative, tail, Some(8));
        }
        if head.eq_ignore_ascii_case("0x") {
            return (negative, tail, Some(16));
        }
    }
    (negative, rest, None)
}

fn resolve_radix(explicit: u8, prefixed: Option<u8>) -> Result<u8, ToolError> {
    match (explicit, prefixed) {
        (0, Some(base)) => Ok(base),
        (0, None) => Ok(10),
        (base, Some(prefixed)) if base != prefixed => Err(invalid_syntax(format!(
            "前缀表示 {prefixed} 进制，与指定输入进制 {base} 不一致"
        ))),
        (base, _) if (2..=36).contains(&base) => Ok(base),
        (base, _) => Err(unsupported(format!("进制必须在 2–36 之间，实际为 {base}"))),
    }
}

fn check_signed_range(value: &BigInt, width: u8) -> Result<(), ToolError> {
    let half = BigInt::one() << (width as usize - 1);
    let min = -&half;
    let max = &half - BigInt::one();
    if value < &min || value > &max {
        return Err(unsupported(format!(
            "数值 {value} 超出 {width} 位有符号范围 [{min}, {max}]"
        ))
        .with_hint("请改用 `Bits` 位模式解释，或选择更宽的位宽"));
    }
    Ok(())
}

fn format_number(value: &BigInt, radix: u32, uppercase: bool, prefix: bool, group: bool) -> String {
    let negative = value.is_negative();
    let digits = value.magnitude().to_str_radix(radix);
    let digits = if uppercase {
        digits.to_uppercase()
    } else {
        digits
    };
    let grouped = if group {
        group_digits(&digits, if radix == 10 { 3 } else { 4 })
    } else {
        digits
    };
    let prefix = if prefix {
        match radix {
            2 => "0b",
            8 => "0o",
            16 => "0x",
            _ => "",
        }
    } else {
        ""
    };
    let mut out = String::new();
    if negative && !value.is_zero() {
        out.push('-');
    }
    out.push_str(prefix);
    out.push_str(&grouped);
    out
}

fn group_digits(digits: &str, size: usize) -> String {
    let chars: Vec<char> = digits.chars().collect();
    let mut out = String::with_capacity(chars.len() + chars.len() / size);
    for (index, ch) in chars.iter().enumerate() {
        if index > 0 && (chars.len() - index).is_multiple_of(size) {
            out.push(' ');
        }
        out.push(*ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use devtools_core::ToolErrorKind;

    fn request(input: &str) -> NumberBaseConverterRequest {
        NumberBaseConverterRequest {
            input: input.to_string(),
            from_base: 0,
            signed_mode: SignedMode::Mathematical,
            bit_width: None,
            custom_base: None,
            uppercase: false,
            prefixes: true,
            group: false,
        }
    }

    #[test]
    fn decimal_to_all_four_bases() {
        let resp = convert(&request("255")).unwrap();
        assert_eq!(resp.binary, "0b11111111");
        assert_eq!(resp.octal, "0o377");
        assert_eq!(resp.decimal, "255");
        assert_eq!(resp.hexadecimal, "0xff");
        assert_eq!(resp.bits, 8);
        assert_eq!(resp.signed_value, "255");
    }

    #[test]
    fn prefixes_are_auto_detected() {
        assert_eq!(convert(&request("0xFF")).unwrap().decimal, "255");
        assert_eq!(convert(&request("0b1010")).unwrap().decimal, "10");
        assert_eq!(convert(&request("0o755")).unwrap().decimal, "493");
    }

    #[test]
    fn uppercase_and_grouping_apply() {
        let mut req = request("0xdeadbeef");
        req.uppercase = true;
        req.group = true;
        let resp = convert(&req).unwrap();
        assert_eq!(resp.hexadecimal, "0xDEAD BEEF");
        assert_eq!(resp.decimal, "3 735 928 559");
    }

    #[test]
    fn custom_base_36() {
        let mut req = request("1295");
        req.custom_base = Some(36);
        let resp = convert(&req).unwrap();
        assert_eq!(resp.custom.as_deref(), Some("zz"));
    }

    #[test]
    fn signed_math_keeps_negative_values() {
        let resp = convert(&request("-255")).unwrap();
        assert_eq!(resp.signed_value, "-255");
        assert_eq!(resp.hexadecimal, "-0xff");
        assert_eq!(resp.bits, 8);
    }

    #[test]
    fn bits_mode_interprets_twos_complement() {
        let mut req = request("0xFF");
        req.signed_mode = SignedMode::Bits;
        req.bit_width = Some(8);
        let resp = convert(&req).unwrap();
        assert_eq!(resp.signed_value, "-1");
        assert_eq!(resp.decimal, "-1");
        assert_eq!(resp.two_complement.as_deref(), Some("0xff"));
    }

    #[test]
    fn bits_mode_rejects_value_larger_than_width() {
        let mut req = request("0x1FF");
        req.signed_mode = SignedMode::Bits;
        req.bit_width = Some(8);
        let err = convert(&req).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
    }

    #[test]
    fn bits_mode_requires_bit_width() {
        let mut req = request("0xFF");
        req.signed_mode = SignedMode::Bits;
        let err = convert(&req).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
    }

    #[test]
    fn mathematical_with_bit_width_checks_range_and_emits_twos_complement() {
        let mut req = request("-1");
        req.bit_width = Some(8);
        let resp = convert(&req).unwrap();
        assert_eq!(resp.two_complement.as_deref(), Some("0xff"));

        let mut req = request("128");
        req.bit_width = Some(8);
        let err = convert(&req).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
        assert!(err.hint.is_some());
    }

    #[test]
    fn hard_4096_bit_cap_rejects_larger_input() {
        let over = format!("1{}", "0".repeat(4096));
        let mut req = request(&over);
        req.from_base = 2;
        let err = convert(&req).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::SizeLimitExceeded);

        // 4096 位本身允许。
        let exact = format!("1{}", "0".repeat(4095));
        let mut req = request(&exact);
        req.from_base = 2;
        let resp = convert(&req).unwrap();
        assert_eq!(resp.bits, 4096);
    }

    #[test]
    fn invalid_digits_are_rejected() {
        let mut req = request("0b102");
        req.from_base = 2;
        let err = convert(&req).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
    }

    #[test]
    fn conflicting_prefix_and_explicit_base_is_rejected() {
        let mut req = request("0xFF");
        req.from_base = 10;
        let err = convert(&req).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
    }

    #[test]
    fn grouping_groups_from_the_right() {
        assert_eq!(group_digits("11111111", 4), "1111 1111");
        assert_eq!(group_digits("12345", 3), "12 345");
    }
}
