//! #27 X.509 证书解析（X509Parser）。
//!
//! 解析 PEM / `.crt` / `.cer` / `.der` / `.pfx` / `.p12`，100% 离线；
//! PFX 口令包装在 [`Zeroizing`] 中；**严格拒绝私钥**（`PRIVATE KEY` PEM 块
//! 或 PFX 中任何私钥材料的明文提取）；返回证书链、subject/issuer、SAN、
//! 时效徽章与 SHA-256 / SHA-1 指纹。

use p12_keystore::Pkcs12Archive;
use sha1::Sha1;
use sha2::{Digest, Sha256};
use x509_parser::certificate::{X509Certificate, X509CertificateParser};
use x509_parser::extensions::GeneralName;
use x509_parser::nom::Parser;
use zeroize::Zeroizing;

use crate::{ToolError, ToolErrorKind};

/// 时效徽章。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidityStatus {
    Valid,
    Expired,
    NotYetValid,
}

impl ValidityStatus {
    pub const fn label_zh(self) -> &'static str {
        match self {
            Self::Valid => "有效",
            Self::Expired => "已过期",
            Self::NotYetValid => "尚未生效",
        }
    }
}

/// 单张证书解析结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCertificate {
    pub subject: String,
    pub issuer: String,
    pub san: Vec<String>,
    pub not_before: i64,
    pub not_after: i64,
    pub validity: ValidityStatus,
    pub fingerprint_sha256: String,
    pub fingerprint_sha1: String,
}

/// 解析结果（可能有整条证书链）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct X509Result {
    pub certificates: Vec<ParsedCertificate>,
    /// `true` 表示输入来自 PKCS#12（PFX）。
    pub from_pfx: bool,
}

fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn summarize(cert: &X509Certificate<'_>) -> ParsedCertificate {
    let validity = cert.validity();
    let not_before = validity.not_before.timestamp();
    let not_after = validity.not_after.timestamp();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let status = if now < not_before {
        ValidityStatus::NotYetValid
    } else if now > not_after {
        ValidityStatus::Expired
    } else {
        ValidityStatus::Valid
    };

    let san = cert
        .subject_alternative_name()
        .ok()
        .flatten()
        .map(|ext| {
            ext.value
                .general_names
                .iter()
                .map(|name| match name {
                    GeneralName::DNSName(v) => format!("DNS:{v}"),
                    GeneralName::RFC822Name(v) => format!("email:{v}"),
                    GeneralName::URI(v) => format!("URI:{v}"),
                    GeneralName::IPAddress(v) => format!(
                        "IP:{}",
                        v.iter()
                            .map(|b| b.to_string())
                            .collect::<Vec<_>>()
                            .join(".")
                    ),
                    GeneralName::DirectoryName(v) => format!("dir:{}", v),
                    other => format!("{other:?}"),
                })
                .collect()
        })
        .unwrap_or_default();

    let der = cert.as_raw();
    ParsedCertificate {
        subject: cert.subject().to_string(),
        issuer: cert.issuer().to_string(),
        san,
        not_before,
        not_after,
        validity: status,
        fingerprint_sha256: hex(&Sha256::digest(der)),
        fingerprint_sha1: hex(&Sha1::digest(der)),
    }
}

fn reject_private_key_pem(input: &[u8]) -> Result<(), ToolError> {
    let text = String::from_utf8_lossy(input);
    for block in pem::parse_many(text.as_bytes()).unwrap_or_default() {
        let tag = block.tag().to_ascii_uppercase();
        if tag.contains("PRIVATE KEY")
            || tag == "EC PRIVATE KEY"
            || tag == "RSA PRIVATE KEY"
            || tag == "DSA PRIVATE KEY"
        {
            return Err(ToolError::new(
                ToolErrorKind::UnsupportedInput,
                format!(
                    "检测到私钥 PEM 块（`{}`），出于安全考虑拒绝解析",
                    block.tag()
                ),
            )
            .with_hint("仅提供 CERTIFICATE 公钥证书"));
        }
    }
    Ok(())
}

/// 解析 PEM / DER 证书输入。
pub fn parse_certificate(input: &[u8]) -> Result<X509Result, ToolError> {
    reject_private_key_pem(input)?;

    // 尝试 PEM（可能包含证书链）。
    if let Ok(pems) = pem::parse_many(input) {
        let cert_pems: Vec<_> = pems
            .into_iter()
            .filter(|p| {
                let tag = p.tag().to_ascii_uppercase();
                tag == "CERTIFICATE" || tag == "X509 CERTIFICATE" || tag == "TRUSTED CERTIFICATE"
            })
            .collect();
        if !cert_pems.is_empty() {
            let mut certificates = Vec::with_capacity(cert_pems.len());
            for p in cert_pems {
                let parsed = x509_parser::pem::Pem {
                    label: p.tag().to_owned(),
                    contents: p.contents().to_vec(),
                };
                let cert = parsed.parse_x509().map_err(|e| {
                    ToolError::new(
                        ToolErrorKind::InvalidSyntax,
                        format!("PEM 证书解析失败：{e}"),
                    )
                })?;
                certificates.push(summarize(&cert));
            }
            return Ok(X509Result {
                certificates,
                from_pfx: false,
            });
        }
    }

    // DER（.crt / .cer / .der）。
    if !input.is_empty() && input[0] == 0x30 {
        let mut parser = X509CertificateParser::new().with_deep_parse_extensions(true);
        let (_, cert) = parser.parse(input).map_err(|e| {
            ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("DER 证书解析失败：{e}"),
            )
        })?;
        return Ok(X509Result {
            certificates: vec![summarize(&cert)],
            from_pfx: false,
        });
    }

    Err(ToolError::new(
        ToolErrorKind::UnsupportedInput,
        "无法识别的证书格式（需 PEM CERTIFICATE 或 DER）",
    ))
}

/// 解析 PKCS#12（`.pfx` / `.p12`），仅提取证书，**绝不提取或展示私钥**。
///
/// 口令以 [`Zeroizing`] 包装，函数返回后立即清零。
pub fn parse_pkcs12(input: &[u8], password: &str) -> Result<X509Result, ToolError> {
    let password = Zeroizing::new(password.to_owned());
    let archive = Pkcs12Archive::from_pkcs12(input, &password).map_err(|e| {
        ToolError::new(
            ToolErrorKind::InvalidSyntax,
            format!("PFX/P12 解析失败：{e}"),
        )
    })?;

    // 显式检测私钥袋的存在；即便存在也只报告"检测到私钥，已忽略"，绝不输出其内容。
    let has_private_key = !archive.keys.is_empty();
    let mut certificates = Vec::with_capacity(archive.certs.len());
    for bag in &archive.certs {
        let der = bag.cert.as_der();
        let mut parser = X509CertificateParser::new().with_deep_parse_extensions(true);
        let (_, cert) = parser.parse(der).map_err(|e| {
            ToolError::new(
                ToolErrorKind::InvalidSyntax,
                format!("PFX 内证书解析失败：{e}"),
            )
        })?;
        certificates.push(summarize(&cert));
    }

    if certificates.is_empty() {
        return Err(ToolError::new(
            ToolErrorKind::UnsupportedInput,
            if has_private_key {
                "PFX 仅包含私钥，不包含可展示的证书，已拒绝提取私钥材料"
            } else {
                "PFX 中未找到证书"
            },
        ));
    }
    Ok(X509Result {
        certificates,
        from_pfx: true,
    })
}

/// 统一入口：按内容自动选择 PEM / DER / PFX 解析路径。
pub fn parse_x509_input(input: &[u8], password: Option<&str>) -> Result<X509Result, ToolError> {
    if let Some(pw) = password {
        return parse_pkcs12(input, pw);
    }
    parse_certificate(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_private_key_pem_blocks() {
        let pk = b"-----BEGIN PRIVATE KEY-----\nMIIB\n-----END PRIVATE KEY-----\n";
        let err = parse_certificate(pk).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
        assert!(err.message.contains("私钥"));

        let rsa = b"-----BEGIN RSA PRIVATE KEY-----\nxxx\n-----END RSA PRIVATE KEY-----\n";
        assert_eq!(
            parse_certificate(rsa).unwrap_err().kind,
            ToolErrorKind::UnsupportedInput
        );
    }

    #[test]
    fn rejects_unrecognized_format() {
        let err = parse_certificate(b"just plain text").unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::UnsupportedInput);
    }

    #[test]
    fn validity_status_labels() {
        assert_eq!(ValidityStatus::Valid.label_zh(), "有效");
        assert_eq!(ValidityStatus::Expired.label_zh(), "已过期");
    }

    /// 测试用自签名证书（CN=devtools-test, O=DevTools, C=CN），有效期 2026–2036。
    const TEST_CERT_PEM: &str = "-----BEGIN CERTIFICATE-----\nMIIDITCCAgmgAwIBAgIUQubP1nFXA5rnkdvl8SLNlYKA+ikwDQYJKoZIhvcNAQEL\nBQAwIDERMA8GA1UECgwIRGV2VG9vbHMxCzAJBgNVBAYTAkNOMB4XDTI2MTEyNzEw\nMjAyMloXDTM2MTEyNDEwMjAyMlowIDERMA8GA1UECgwIRGV2VG9vbHMxCzAJBgNV\nBAYTAkNOMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA2saT7x3nGHyT\nFYKKhxE98bGky+0HqdwWKTDK06C8RxyKd711zPgZITfgVtH578dxSm0uEG3bGcw/\nrMOfwO0P3GWp311ft29DbjEABFUbiikEM1GN2ITooKPMleA3nb3s9HjvHDJDEzHQ\n3xmwh/ds9wL+rHxjYCvWEmvlq4bJiKEW91IkgJQGDmHRiQVh7YIiVFCw7c/vZeRL\nkP9ONEe/b9wnoROnjt/dDOYk02nu+1dsfV1zfPdpVlU3im7WRqa/2OYtDGA+8Uew\neADfGtgl45On1S/JuYqRmwfuyqVbleV/qet1N67WHmk1udpB0LmZpl3yiCexEfEF\nlznVp48BhwIDAQABo1MwUTAdBgNVHQ4EFgQUbgfzyQ2hoeOx46DhhZE0h/qlmPsw\nHwYDVR0jBBgwFoAUbgfzyQ2hoeOx46DhhZE0h/qlmPswDwYDVR0TAQH/BAUwAwEB\n/zANBgkqhkiG9w0BAQsFAAOCAQEARVcTuMJZYVlv8viSRrDgBTndnCmUu3SlyweZ\n31UtuPnbn10G2oeD3LumcIYxeAkByCcyGraq5Xm8aZieFFehvzxVT22q2eCXOuoa\nePtX0ndlw6eCJ34hzzLUZZWepv9iHJl4grXNkCHo/JY2d0uRpk0X2F5NCkVcyaUh\nRmoHrVhXPlirncx8vTumQ5KG2h6HlY0/jFIYDfCwI4usuIMD9j5uqR4rwSroGmHV\ntKZsZLAkCFd845Klz2jpCFRJ8cNdOElX83tpPwI+v6IubMFlHee7uj9cRWGWvLMA\nAsy/FUZdKD12QjNpEkKBa9GmGFPTFm37C0r84PXCSsIHv6gqng==\n-----END CERTIFICATE-----\n";

    #[test]
    fn parses_real_pem_certificate_with_all_fields() {
        let result = parse_certificate(TEST_CERT_PEM.as_bytes()).unwrap();
        assert!(!result.from_pfx);
        assert_eq!(result.certificates.len(), 1);
        let cert = &result.certificates[0];
        assert!(
            cert.subject.contains("DevTools"),
            "subject={}",
            cert.subject
        );
        assert!(cert.subject.contains("CN"), "subject={}", cert.subject);
        assert!(!cert.issuer.is_empty());
        assert_eq!(cert.fingerprint_sha256.len(), 64);
        assert_eq!(cert.fingerprint_sha1.len(), 40);
        assert_eq!(cert.validity, ValidityStatus::Valid);
        assert!(cert.not_after > cert.not_before);
        assert!(cert.san.is_empty(), "测试证书未配置 SAN");
    }

    #[test]
    fn unified_entry_routes_to_pfx_when_password_given() {
        let err = parse_x509_input(b"garbage", Some("pw")).unwrap_err();
        assert_eq!(err.kind, ToolErrorKind::InvalidSyntax);
    }
}
