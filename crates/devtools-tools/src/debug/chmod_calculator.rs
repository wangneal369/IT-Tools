//! #24 Chmod 权限计算器（ChmodCalculator）。
//!
//! 3×3 `r/w/x` 矩阵 + 特殊位（SUID/SGID/Sticky）；全 0 输出 3 位八进制，
//! 非 0 输出 4 位；八进制 ↔ 符号（`rwxr-xr-x` / `u+rwx`）双向转换与
//! `chmod` 命令构建。

use crate::{ToolError, ToolErrorKind};

/// 三类主体的权限位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PermissionTriplet {
    pub read: bool,
    pub write: bool,
    pub execute: bool,
}

impl PermissionTriplet {
    pub const fn new(read: bool, write: bool, execute: bool) -> Self {
        Self {
            read,
            write,
            execute,
        }
    }

    pub const fn from_octal(digit: u8) -> Self {
        Self {
            read: digit & 4 != 0,
            write: digit & 2 != 0,
            execute: digit & 1 != 0,
        }
    }

    pub const fn to_octal(self) -> u8 {
        (self.read as u8) * 4 + (self.write as u8) * 2 + (self.execute as u8)
    }

    pub fn to_symbolic(self) -> String {
        format!(
            "{}{}{}",
            if self.read { 'r' } else { '-' },
            if self.write { 'w' } else { '-' },
            if self.execute { 'x' } else { '-' },
        )
    }

    pub fn from_symbolic(s: &str) -> Result<Self, ToolError> {
        let bytes = s.as_bytes();
        if bytes.len() != 3 {
            return Err(ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("权限三元组必须为 3 个字符，实际为 `{}`", s),
            ));
        }
        let parse = |b: u8, ch: char| match b {
            x if x == ch as u8 => Ok(true),
            b'-' => Ok(false),
            _ => Err(ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("无效权限字符 `{}`（应为 {} 或 -）", b as char, ch),
            )),
        };
        Ok(Self::new(
            parse(bytes[0], 'r')?,
            parse(bytes[1], 'w')?,
            parse(bytes[2], 'x')?,
        ))
    }
}

/// 完整权限（三类主体 + 3 个特殊位）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Permissions {
    pub owner: PermissionTriplet,
    pub group: PermissionTriplet,
    pub others: PermissionTriplet,
    pub suid: bool,
    pub sgid: bool,
    pub sticky: bool,
}

impl Permissions {
    pub fn from_octal(value: u16) -> Result<Self, ToolError> {
        if value > 0o7777 {
            return Err(ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("八进制权限 `{value:o}` 超出 07777 范围"),
            ));
        }
        let special = value >> 9;
        Ok(Self {
            owner: PermissionTriplet::from_octal(((value >> 6) & 0o7) as u8),
            group: PermissionTriplet::from_octal(((value >> 3) & 0o7) as u8),
            others: PermissionTriplet::from_octal((value & 0o7) as u8),
            suid: special & 0o4 != 0,
            sgid: special & 0o2 != 0,
            sticky: special & 0o1 != 0,
        })
    }

    /// 特殊位全 0 → 3 位八进制；非 0 → 4 位八进制。
    pub fn to_octal(self) -> String {
        let base = (self.owner.to_octal() as u16) << 6
            | (self.group.to_octal() as u16) << 3
            | self.others.to_octal() as u16;
        let special = (self.suid as u16) << 2 | (self.sgid as u16) << 1 | self.sticky as u16;
        if special == 0 {
            format!("{:03o}", base)
        } else {
            format!("{:04o}", (special << 9) | base)
        }
    }

    /// 符号表示，如 `rwxr-xr-x`（特殊位在对应执行位上以 `s`/`t` 表示）。
    pub fn to_symbolic(self) -> String {
        let special_char = |triplet: PermissionTriplet, enabled: bool, ch: char| -> char {
            if triplet.execute {
                if enabled {
                    ch.to_ascii_lowercase()
                } else {
                    'x'
                }
            } else if enabled {
                ch.to_ascii_uppercase()
            } else {
                '-'
            }
        };
        let owner = self.owner.to_symbolic();
        let group = self.group.to_symbolic();
        let others = self.others.to_symbolic();
        let mut chars: Vec<char> = owner
            .chars()
            .chain(group.chars())
            .chain(others.chars())
            .collect();
        if self.suid {
            chars[2] = special_char(self.owner, true, 's');
        }
        if self.sgid {
            chars[5] = special_char(self.group, true, 's');
        }
        if self.sticky {
            chars[8] = special_char(self.others, true, 't');
        }
        chars.into_iter().collect()
    }

    /// 从 9/10 字符符号串解析（支持 `s`/`S`/`t`/`T` 特殊位）。
    pub fn from_symbolic(s: &str) -> Result<Self, ToolError> {
        let chars: Vec<char> = s.chars().collect();
        if chars.len() != 9 && chars.len() != 10 {
            return Err(ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("符号权限必须为 9 或 10 个字符，实际为 `{}`", s),
            ));
        }
        let s = if chars.len() == 10 {
            // 首字符为文件类型（如 `-` / `d`），忽略但校验。
            if !matches!(chars[0], '-' | 'd' | 'l' | 'b' | 'c' | 'p' | 's') {
                return Err(ToolError::new(
                    ToolErrorKind::InvalidSyntax,
                    format!("无效文件类型字符 `{}`", chars[0]),
                ));
            }
            chars[1..].iter().collect::<String>()
        } else {
            s.to_owned()
        };
        let c = s.as_bytes();
        let suid = c[2] == b's' || c[2] == b'S';
        let sgid = c[5] == b's' || c[5] == b'S';
        let sticky = c[8] == b't' || c[8] == b'T';
        let normalize = |b: u8, special: bool| -> u8 {
            if special && matches!(b, b's' | b'S' | b't' | b'T') {
                b'x'
            } else {
                b
            }
        };
        let seg = |range: std::ops::Range<usize>| -> Result<PermissionTriplet, ToolError> {
            let mut part = String::new();
            for i in range {
                part.push(normalize(c[i], suid || sgid || sticky) as char);
            }
            PermissionTriplet::from_symbolic(&part)
        };
        Ok(Self {
            owner: seg(0..3)?,
            group: seg(3..6)?,
            others: seg(6..9)?,
            suid,
            sgid,
            sticky,
        })
    }

    /// 构建 `chmod` 命令。
    pub fn to_command(self, path: &str) -> String {
        format!("chmod {} {}", self.to_octal(), path)
    }
}

/// 从符号加法表达式（如 `u+rwx,g-w,o=x`）修改权限。
pub fn apply_symbolic_ops(base: Permissions, ops: &str) -> Result<Permissions, ToolError> {
    let mut result = base;
    for clause in ops.split(',') {
        let clause = clause.trim();
        if clause.is_empty() {
            continue;
        }
        let (who, rest) = clause.split_at(clause.find(['+', '-', '=']).ok_or_else(|| {
            ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("缺少操作符：`{clause}`"),
            )
        })?);
        let (op, perms) = rest.split_at(1);
        let targets = parse_who(who)?;
        let mut triplet = PermissionTriplet::new(
            perms.contains('r'),
            perms.contains('w'),
            perms.contains('x'),
        );
        for target in targets {
            let slot = match target {
                'u' => &mut result.owner,
                'g' => &mut result.group,
                'o' => &mut result.others,
                'a' => {
                    result.owner = apply_one(result.owner, op, triplet);
                    result.group = apply_one(result.group, op, triplet);
                    result.others = apply_one(result.others, op, triplet);
                    triplet = PermissionTriplet::new(
                        perms.contains('r'),
                        perms.contains('w'),
                        perms.contains('x'),
                    );
                    continue;
                }
                _ => unreachable!(),
            };
            *slot = apply_one(*slot, op, triplet);
        }
        if perms.contains('s') || perms.contains('t') {
            match op {
                "+" => {
                    if perms.contains('s') {
                        result.suid = true;
                        result.sgid = true;
                    }
                    if perms.contains('t') {
                        result.sticky = true;
                    }
                }
                "-" => {
                    if perms.contains('s') {
                        result.suid = false;
                        result.sgid = false;
                    }
                    if perms.contains('t') {
                        result.sticky = false;
                    }
                }
                _ => {}
            }
        }
    }
    Ok(result)
}

fn parse_who(who: &str) -> Result<Vec<char>, ToolError> {
    if who.is_empty() {
        return Ok(vec!['a']);
    }
    let mut seen = Vec::new();
    for ch in who.chars() {
        if !matches!(ch, 'u' | 'g' | 'o' | 'a') {
            return Err(ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("无效权限主体 `{ch}`"),
            ));
        }
        if !seen.contains(&ch) {
            seen.push(ch);
        }
    }
    Ok(seen)
}

fn apply_one(base: PermissionTriplet, op: &str, value: PermissionTriplet) -> PermissionTriplet {
    match op {
        "+" => PermissionTriplet::new(
            base.read || value.read,
            base.write || value.write,
            base.execute || value.execute,
        ),
        "-" => PermissionTriplet::new(
            base.read && !value.read,
            base.write && !value.write,
            base.execute && !value.execute,
        ),
        "=" => value,
        _ => base,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn octal_three_digit_when_no_special_bits() {
        let p = Permissions::from_octal(0o755).unwrap();
        assert_eq!(p.to_octal(), "755");
        assert_eq!(p.to_symbolic(), "rwxr-xr-x");
        assert_eq!(p.to_command("app.sh"), "chmod 755 app.sh");
    }

    #[test]
    fn octal_four_digit_with_special_bits() {
        let p = Permissions::from_octal(0o4755).unwrap();
        assert!(p.suid);
        assert_eq!(p.to_octal(), "4755");
        assert_eq!(p.to_symbolic(), "rwsr-xr-x");
        let sticky = Permissions::from_octal(0o1777).unwrap();
        assert_eq!(sticky.to_octal(), "1777");
        assert_eq!(sticky.to_symbolic(), "rwxrwxrwt");
        let sgid_no_x = Permissions::from_octal(0o2644).unwrap();
        assert_eq!(sgid_no_x.to_symbolic(), "rw-r-Sr--");
    }

    #[test]
    fn symbolic_roundtrip() {
        for octal in [0o644, 0o755, 0o4755, 0o2775, 0o1777] {
            let p = Permissions::from_octal(octal).unwrap();
            let symbol = p.to_symbolic();
            let back = Permissions::from_symbolic(&symbol).unwrap();
            assert_eq!(back, p, "八进制 {octal:o} / 符号 {symbol}");
            let expected = if octal > 0o777 {
                format!("{octal:04o}")
            } else {
                format!("{octal:03o}")
            };
            assert_eq!(back.to_octal(), expected);
        }
        let with_type = Permissions::from_symbolic("-rwxr-xr-x").unwrap();
        assert_eq!(with_type, Permissions::from_octal(0o755).unwrap());
    }

    #[test]
    fn symbolic_ops() {
        let base = Permissions::from_octal(0o644).unwrap();
        let added = apply_symbolic_ops(base, "u+x,g+w").unwrap();
        assert_eq!(added.to_octal(), "764");
        let removed = apply_symbolic_ops(base, "o-r").unwrap();
        assert_eq!(removed.to_octal(), "640");
        let assigned = apply_symbolic_ops(base, "a=r").unwrap();
        assert_eq!(assigned.to_octal(), "444");
    }

    #[test]
    fn invalid_inputs() {
        assert!(Permissions::from_octal(0o10000).is_err());
        assert!(Permissions::from_symbolic("rwx").is_err());
        assert!(apply_symbolic_ops(Permissions::default(), "u?rwx").is_err());
    }
}
