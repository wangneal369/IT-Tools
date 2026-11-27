//! Standardized content data types for smart detection (ADR-0003 / spec §4).
//!
//! [`DataType`] covers the three clipboard roots (`Files`, `Image`, `Text`) and
//! every distinct V1 detector output type needed by the hierarchical DAG and
//! its six explicit suppression rules (including `JsonArray` and `UnixTimestamp`).

use serde::{Deserialize, Serialize};

/// The four detection confidence levels and their fixed scores (spec §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Confidence {
    /// 25 分。
    Low,
    /// 50 分。
    Medium,
    /// 75 分。
    High,
    /// 100 分。
    Exact,
}

impl Confidence {
    /// The fixed numeric score for this confidence level.
    pub const fn score(self) -> u8 {
        match self {
            Confidence::Low => 25,
            Confidence::Medium => 50,
            Confidence::High => 75,
            Confidence::Exact => 100,
        }
    }

    /// `Medium` and above surface a light banner hint in the top bar.
    pub const fn is_banner(self) -> bool {
        matches!(
            self,
            Confidence::Medium | Confidence::High | Confidence::Exact
        )
    }
}

/// Standardized semantic data classification emitted by `devtools-detect`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DataType {
    // --- Clipboard roots ---
    Files,
    Image,
    Text,

    // --- Files children ---
    ImageFile,
    CertificateFile,
    File,

    // --- Text detector hierarchy (20 V1 groups, retaining suppression leaves) ---
    Jwt,
    CertificatePem,
    Base64Image,
    GZipBase64,
    Json,
    JsonArray,
    XmlHtml,
    Yaml,
    Toml,
    Url,
    UrlEncoded,
    DateTime,
    UnixTimestamp,
    Cron,
    Ipv4Cidr,
    Color,
    Base64Text,
    HtmlUnicode,
    EscapedText,
    Sql,
    Chmod,
    NumberBase,
    HashDigest,
}

impl DataType {
    /// Closed table of all V1 data types (3 roots + 26 typed detections).
    pub const ALL: [DataType; 29] = [
        DataType::Files,
        DataType::Image,
        DataType::Text,
        DataType::ImageFile,
        DataType::CertificateFile,
        DataType::File,
        DataType::Jwt,
        DataType::CertificatePem,
        DataType::Base64Image,
        DataType::GZipBase64,
        DataType::Json,
        DataType::JsonArray,
        DataType::XmlHtml,
        DataType::Yaml,
        DataType::Toml,
        DataType::Url,
        DataType::UrlEncoded,
        DataType::DateTime,
        DataType::UnixTimestamp,
        DataType::Cron,
        DataType::Ipv4Cidr,
        DataType::Color,
        DataType::Base64Text,
        DataType::HtmlUnicode,
        DataType::EscapedText,
        DataType::Sql,
        DataType::Chmod,
        DataType::NumberBase,
        DataType::HashDigest,
    ];
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn confidence_scores_and_ordering_are_fixed() {
        assert_eq!(Confidence::Low.score(), 25);
        assert_eq!(Confidence::Medium.score(), 50);
        assert_eq!(Confidence::High.score(), 75);
        assert_eq!(Confidence::Exact.score(), 100);
        assert!(Confidence::Low < Confidence::Medium);
        assert!(Confidence::Medium < Confidence::High);
        assert!(Confidence::High < Confidence::Exact);
    }

    #[test]
    fn banner_threshold_starts_at_medium() {
        assert!(!Confidence::Low.is_banner());
        assert!(Confidence::Medium.is_banner());
        assert!(Confidence::High.is_banner());
        assert!(Confidence::Exact.is_banner());
    }

    #[test]
    fn data_types_are_unique_and_cover_suppression_leaves() {
        let unique: HashSet<_> = DataType::ALL.into_iter().collect();
        assert_eq!(unique.len(), DataType::ALL.len());
        assert!(unique.contains(&DataType::JsonArray));
        assert!(unique.contains(&DataType::UrlEncoded));
        assert!(unique.contains(&DataType::UnixTimestamp));
        assert!(unique.contains(&DataType::XmlHtml));
        assert!(unique.contains(&DataType::Toml));
    }
}
