//! 隐私检查先于解析，预览永不返回敏感正文。

use regex::Regex;
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PrivacyAssessment {
    pub suppress_auto_prompt: bool,
    pub private_key: bool,
    pub certificate_request: bool,
}

pub fn assess_privacy(text: &str) -> PrivacyAssessment {
    static SECRET: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
        r"(?:\b(?:AKIA|ASIA)[A-Z0-9]{16}\b|\bgh[pousr]_[A-Za-z0-9_]{8,}|\bgithub_pat_[A-Za-z0-9_]{8,}|\bsk-(?:proj-)?[A-Za-z0-9_-]{8,}|\bxox[baprs]-[A-Za-z0-9-]{8,}|\bAIza[A-Za-z0-9_-]{20,})"
    ).expect("固定密钥表达式")
    });
    let private_key = text.split("-----BEGIN ").skip(1).any(|part| {
        part.split_once("-----")
            .is_some_and(|(label, _)| label.ends_with("PRIVATE KEY"))
    });
    let certificate_request = text.contains("-----BEGIN CERTIFICATE REQUEST-----")
        || text.contains("-----BEGIN NEW CERTIFICATE REQUEST-----");
    PrivacyAssessment {
        suppress_auto_prompt: private_key || SECRET.is_match(text),
        private_key,
        certificate_request,
    }
}

/// 预览最多 120 字符；敏感输入整体脱敏，避免泄露同一载荷中的其他密钥。
pub fn redacted_preview(text: &str) -> String {
    if assess_privacy(text).suppress_auto_prompt {
        "[敏感内容已隐藏]".into()
    } else {
        text.chars().take(120).collect()
    }
}
