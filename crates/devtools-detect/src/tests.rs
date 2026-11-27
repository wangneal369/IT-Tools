use crate::*;
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
};
use devtools_core::{Confidence, DataType as D, ImagePayload, RawPayload, ToolId as T};
use static_assertions::assert_not_impl_any;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

assert_not_impl_any!(DetectionResult: serde::Serialize, serde::de::DeserializeOwned);
assert_not_impl_any!(Recommendation: serde::Serialize, serde::de::DeserializeOwned);
assert_not_impl_any!(ClipboardDeduplicator: serde::Serialize, serde::de::DeserializeOwned);

fn payload(value: &str) -> RawPayload {
    RawPayload::Text(Arc::from(value))
}
fn detect(value: &str) -> DetectionResult {
    DetectionEngine::default().detect(&payload(value), DetectionMode::Strict)
}
fn has(result: &DetectionResult, kind: D) -> bool {
    result
        .matches
        .iter()
        .any(|m| m.leaf().is_some_and(|n| n.data_type == kind))
}
fn check(kind: D, positives: &[&str], negatives: &[&str]) {
    for value in positives {
        assert!(has(&detect(value), kind), "{kind:?}: {value}");
    }
    for value in negatives {
        assert!(!has(&detect(value), kind), "误判 {kind:?}: {value}");
    }
}
fn png() -> Vec<u8> {
    // PNG 签名 + IHDR 宽高；无需解压 IDAT。
    vec![
        137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 2, 0, 0, 0, 3, 8, 6,
        0, 0, 0,
    ]
}
fn jwt() -> String {
    format!(
        "{}.{}.{}",
        URL_SAFE_NO_PAD.encode(r#"{"alg":"HS256","typ":"JWT"}"#),
        URL_SAFE_NO_PAD.encode(r#"{"sub":"123"}"#),
        URL_SAFE_NO_PAD.encode("signature")
    )
}
fn der(tag: u8, body: &[u8]) -> Vec<u8> {
    assert!(body.len() < 128);
    [vec![tag, body.len() as u8], body.to_vec()].concat()
}
fn certificate() -> Vec<u8> {
    let validity = der(
        0x30,
        &[der(0x17, b"750101000000Z"), der(0x17, b"991231000000Z")].concat(),
    );
    let tbs = der(
        0x30,
        &[
            der(2, &[1]),
            der(0x30, &[]),
            der(0x30, &[]),
            validity,
            der(0x30, &[]),
            der(0x30, &[]),
        ]
        .concat(),
    );
    der(0x30, &[tbs, der(0x30, &[]), der(3, &[0])].concat())
}
fn pem() -> String {
    format!(
        "-----BEGIN CERTIFICATE-----\n{}\n-----END CERTIFICATE-----",
        STANDARD.encode(certificate())
    )
}

#[test]
fn jwt_detector() {
    assert!(has(&detect(&jwt()), D::Jwt));
    check(
        D::Jwt,
        &[],
        &["aaa.bbb.ccc", "e30.e30.c2ln", "a.b", "a.b.c.d"],
    );
}
#[test]
fn certificate_pem_detector() {
    assert!(has(&detect(&pem()), D::CertificatePem));
    for marker in [
        "PRIVATE KEY",
        "RSA PRIVATE KEY",
        "ENCRYPTED PRIVATE KEY",
        "CERTIFICATE REQUEST",
        "NEW CERTIFICATE REQUEST",
    ] {
        assert!(!has(
            &detect(&format!("{}\n-----BEGIN {marker}-----", pem())),
            D::CertificatePem
        ));
    }
    check(
        D::CertificatePem,
        &[],
        &["-----BEGIN CERTIFICATE-----\nbad\n-----END CERTIFICATE-----"],
    );
}
#[test]
fn base64_image_detector() {
    assert!(has(
        &detect(&format!("data:image/png;base64,{}", STANDARD.encode(png()))),
        D::Base64Image
    ));
    check(
        D::Base64Image,
        &[],
        &[
            "data:text/plain;base64,aGVsbG8=",
            "data:image/png;base64,aGVsbG8=",
        ],
    );
}
#[test]
fn malformed_unicode_magic_prefix_does_not_panic() {
    let value = format!("data:image/png;base64,{}", "中".repeat(FULL_TEXT_LIMIT));
    let result = detect(&value);
    assert_eq!(result.status, DetectionStatus::PrefixOnly);
    assert!(!has(&result, D::Base64Image));
}
#[test]
fn gzip_base64_detector() {
    assert!(has(
        &detect(&STANDARD.encode([0x1f, 0x8b, 8, 0, 0, 0, 0, 0, 0, 3])),
        D::GZipBase64
    ));
    check(D::GZipBase64, &[], &["H4sI", "aGVsbG8="]);
}
#[test]
fn json_detector_rejects_scalars() {
    check(
        D::Json,
        &[r#"{"a":1}"#, "[]", "[1,2]"],
        &["42", r#""hello""#, "true", "null", "{bad}"],
    );
}
#[test]
fn homogeneous_json_array_detector_and_formatter_exception() {
    check(
        D::JsonArray,
        &[
            r#"[{"a":1},{"b":2}]"#,
            "[1,2,3]",
            r#"["a","b"]"#,
            "[true,false]",
            "[[1],[2]]",
        ],
        &["[]", "[1,true]", r#"[{"a":1},2]"#],
    );
    let result = detect(r#"[{"a":1}]"#);
    assert_eq!(
        result
            .recommendations
            .iter()
            .map(|r| r.tool_id)
            .collect::<Vec<_>>(),
        vec![T::JsonToTable, T::JsonFormatter]
    );
    assert_eq!(
        result.recommendations[0]
            .chain
            .nodes
            .iter()
            .map(|n| n.data_type)
            .collect::<Vec<_>>(),
        vec![D::Text, D::Json, D::JsonArray]
    );
}
#[test]
fn xml_html_detector() {
    check(
        D::XmlHtml,
        &[
            "<root><x>1</x></root>",
            "<br/>",
            "<!DOCTYPE html><html><body>hi</body></html>",
        ],
        &["1 < 2", "<not a tag", "<>text"],
    );
}
#[test]
fn yaml_detector() {
    check(
        D::Yaml,
        &["name: alice\ncount: 2", "- one\n- two"],
        &["plain sentence", "42", "x: ["],
    );
}
#[test]
fn toml_detector() {
    check(
        D::Toml,
        &["name = 'alice'\ncount = 2", "[server]\nport=80"],
        &["plain", "name = ["],
    );
}
#[test]
fn url_and_percent_detector() {
    check(
        D::Url,
        &[
            "https://example.com/a?q=1&x=2",
            "file:///C:/tmp/a",
            "mailto:a@example.com",
        ],
        &["example.com", "http://", "hello world"],
    );
    check(
        D::UrlEncoded,
        &["hello%20world", "%E4%B8%AD"],
        &["50%", "%GG", "hello world"],
    );
}
#[test]
fn datetime_and_timestamp_detector() {
    check(
        D::DateTime,
        &[
            "2026-11-27T12:30:00Z",
            "2026-11-27",
            "2026-11-27 12:30:00",
            "2026/11/27",
        ],
        &["2026-02-30", "tomorrow"],
    );
    check(
        D::UnixTimestamp,
        &["1700000000", "1700000000000", "0157766400", "4133980799999"],
        &[
            "0157766399",
            "4133980800",
            "9999999999999",
            "123456789",
            "170000000000",
        ],
    );
}
#[test]
fn cron_detector_including_quartz() {
    check(
        D::Cron,
        &[
            "*/5 * * * *",
            "0 */5 * * * *",
            "0 0 12 ? * MON#2",
            "0 0 0 L * ?",
            "0 0 9 15W * ?",
        ],
        &["90 * * * *", "* * * *", "a b c d e"],
    );
}
#[test]
fn ipv4_detector_all_four_forms() {
    check(
        D::Ipv4Cidr,
        &[
            "192.168.1.1",
            "10.0.0.0/8",
            "192.168.1.1 255.255.255.0",
            "10.0.0.1 - 10.0.0.254",
            "0.0.0.0 0.0.0.0",
        ],
        &[
            "999.0.0.1",
            "10.0.0.0/33",
            "10.0.0.1 255.0.255.0",
            "10.0.0.5-10.0.0.1",
        ],
    );
}
#[test]
fn color_detector_requires_explicit_css_form() {
    check(
        D::Color,
        &[
            "#fff",
            "#12345678",
            "rgb(10, 20, 30)",
            "hsl(120, 50%, 50%)",
            "oklch(0.5 0.2 30)",
            "hwb(90 10% 20%)",
        ],
        &["red", "deadbeef", "fff", "rgb(no)"],
    );
}
#[test]
fn base64_text_detector() {
    check(
        D::Base64Text,
        &["aGVsbG8=", "aGVsbG8gd29ybGQ=", "5Lit5paH"],
        &["YQ==", "========", "////////", "aGVsbG8!"],
    );
}
#[test]
fn html_unicode_detector_all_forms() {
    check(
        D::HtmlUnicode,
        &[
            "&amp;",
            "&#65;",
            "&#x4e2d;",
            r"\u4e2d",
            "U+4E2D",
            r"\004e2d ",
        ],
        &["&bad", "U+XX", r"\ugggg"],
    );
}
#[test]
fn escaped_text_exempts_windows_and_unc_paths() {
    check(
        D::EscapedText,
        &[r"hello\nworld", r"a\tb", r#"hello\"world"#],
        &[
            r"C:\Users\neal\new\test",
            r"c:\temp\new.txt",
            r"\\server\share\new\test",
            "hello",
        ],
    );
}
#[test]
fn sql_detector() {
    check(
        D::Sql,
        &[
            "SELECT a FROM table1",
            "insert into t values (1)",
            "update t set a = 1",
            "delete from t",
            "CREATE TABLE t (a int)",
        ],
        &["select a nice day", "ordinary prose"],
    );
}
#[test]
fn chmod_detector() {
    check(
        D::Chmod,
        &[
            "755",
            "4755",
            "rwxr-xr-x",
            "rwsr-xr-t",
            "u+rwx",
            "u=rw,g=r,o=r",
        ],
        &["888", "12345", "rwxrwxrwz"],
    );
}
#[test]
fn number_base_detector_confidence() {
    check(
        D::NumberBase,
        &["0xFF", "0b1010", "0o755", "42"],
        &["0xGG", "0b2", "0o9", "0x"],
    );
    assert_eq!(detect("42").recommendations[0].confidence, Confidence::Low);
    assert_eq!(
        detect("0x42").recommendations[0].confidence,
        Confidence::High
    );
    assert_eq!(detect("42").banner().count(), 0);
}
#[test]
fn hash_digest_detector_lengths() {
    for size in [32, 40, 64, 128] {
        assert!(has(&detect(&"a".repeat(size)), D::HashDigest));
    }
    for size in [31, 33, 39, 41, 63, 65, 127, 129] {
        assert!(!has(&detect(&"a".repeat(size)), D::HashDigest));
    }
    assert!(!has(&detect(&"g".repeat(64)), D::HashDigest));
}
#[test]
fn all_six_suppression_rules() {
    use crate::engine::{chain, suppress};
    for (winner, losers) in [
        (D::Jwt, vec![D::Base64Text]),
        (D::GZipBase64, vec![D::Base64Text]),
        (D::Base64Image, vec![D::Base64Text]),
        (D::CertificatePem, vec![D::Base64Text]),
        (D::Json, vec![D::Yaml, D::EscapedText]),
        (D::JsonArray, vec![D::Yaml, D::EscapedText]),
        (D::Url, vec![D::HtmlUnicode, D::Base64Text]),
        (D::UnixTimestamp, vec![D::NumberBase]),
        (D::HashDigest, vec![D::Base64Text, D::NumberBase]),
    ] {
        let mut matches = std::iter::once(winner)
            .chain(losers)
            .map(|t| chain(D::Text, &[t], Confidence::High))
            .collect();
        suppress(&mut matches);
        assert_eq!(matches.len(), 1, "{winner:?}");
    }
    let strict = detect(r#"{"x":"a\\nb"}"#);
    assert!(!has(&strict, D::Yaml));
    assert!(!has(&strict, D::EscapedText));
    assert!(
        !strict
            .recommendations
            .iter()
            .any(|r| r.tool_id == T::TextAnalysis)
    );
    assert!(!has(&detect("1700000000"), D::NumberBase));
    assert!(!has(&detect(&"1".repeat(32)), D::NumberBase));
}
#[test]
fn relay_excludes_self_and_appends_exactly_one_parent_level() {
    let value = payload(r#"[{"a":1}]"#);
    let result = DetectionEngine::default().detect(
        &value,
        DetectionMode::Relay {
            source_tool: T::JsonToTable,
        },
    );
    assert_eq!(result.recommendations[0].tool_id, T::JsonFormatter);
    assert!(
        !result
            .recommendations
            .iter()
            .any(|r| r.tool_id == T::JsonToTable || r.tool_id == T::TextAnalysis)
    );
    assert!(
        result
            .recommendations
            .iter()
            .any(|r| r.tool_id == T::JsonPathTester && r.parent_fallback)
    );
    let result = DetectionEngine::default().detect(
        &payload("0xFF"),
        DetectionMode::Relay {
            source_tool: T::NumberBaseConverter,
        },
    );
    assert!(
        result
            .recommendations
            .iter()
            .any(|r| r.tool_id == T::TextAnalysis && r.parent_fallback)
    );
}
#[test]
fn banner_palette_thresholds_limits_and_launch_chain() {
    let raw = payload("0xFF");
    let result = DetectionEngine::default().detect(
        &raw,
        DetectionMode::Relay {
            source_tool: T::SqlFormatter,
        },
    );
    assert_eq!(result.banner().count(), 3);
    assert_eq!(result.palette().count(), 5);
    let input = result.recommendations[0].launch_input(&raw);
    assert_eq!(input.payload, raw);
    assert_eq!(input.chain.leaf().unwrap().data_type, D::NumberBase);
}
#[test]
fn private_keys_and_cloud_secrets_are_suppressed_and_redacted() {
    for value in [
        "-----BEGIN PRIVATE KEY-----",
        "-----BEGIN RSA PRIVATE KEY-----",
        "-----BEGIN EC PRIVATE KEY-----",
        "-----BEGIN OPENSSH PRIVATE KEY-----",
        "-----BEGIN ENCRYPTED PRIVATE KEY-----",
        "AKIA1234567890ABCDEF",
        "ghp_1234567890abcdefghij",
        "sk-proj-1234567890abcdef",
        "xoxb-1234567890-abcdefghij",
        "github_pat_1234567890abcdef",
    ] {
        let result = detect(value);
        assert!(result.suppress_auto_prompt, "{value}");
        assert_eq!(result.preview, "[敏感内容已隐藏]");
        assert_eq!(result.banner().count(), 0);
        assert_eq!(result.palette().count(), 0);
    }
    let embedded = detect(r#"{"key":"ghp_1234567890abcdef"}"#);
    assert!(has(&embedded, D::Json));
    assert!(embedded.suppress_auto_prompt);
    assert_eq!(redacted_preview(&"中".repeat(200)).chars().count(), 120);
}
#[test]
fn payload_thresholds_are_inclusive_and_prefix_only_is_restricted() {
    for (size, status) in [
        (FULL_TEXT_LIMIT, DetectionStatus::Complete),
        (FULL_TEXT_LIMIT + 1, DetectionStatus::PrefixOnly),
        (TEXT_LIMIT, DetectionStatus::PrefixOnly),
        (TEXT_LIMIT + 1, DetectionStatus::Skipped),
    ] {
        let mut value = String::from("{\"x\":1}");
        value.push_str(&" ".repeat(size - value.len()));
        let result = detect(&value);
        assert_eq!(result.status, status);
        assert_eq!(has(&result, D::Json), size == FULL_TEXT_LIMIT);
    }
    let mut value = pem();
    value.push_str(&" ".repeat(FULL_TEXT_LIMIT));
    assert!(has(&detect(&value), D::CertificatePem));
    let unicode = format!(
        "{}{}",
        " ".repeat(MAGIC_PREFIX_LIMIT - 1),
        "中".repeat(FULL_TEXT_LIMIT)
    );
    assert_eq!(detect(&unicode).status, DetectionStatus::PrefixOnly);
}
#[test]
fn malformed_deep_and_alias_payloads_are_bounded() {
    for value in [
        format!("{}0{}", "[".repeat(10000), "]".repeat(10000)),
        "a: &a [*a, *a, *a]".into(),
        format!("x: {}1{}", "[".repeat(10000), "]".repeat(10000)),
    ] {
        let result = detect(&value);
        assert!(!has(&result, D::Json));
        assert!(!has(&result, D::Yaml));
    }
    assert_eq!(
        DetectionEngine::with_budget(Duration::ZERO)
            .detect(&payload("{}"), DetectionMode::Strict)
            .status,
        DetectionStatus::TimedOut
    );
}
#[test]
fn clipboard_roots_obey_files_image_text_priority() {
    let engine = DetectionEngine::default();
    let image = RawPayload::Image(ImagePayload {
        width: 2,
        height: 3,
        bytes: png().into(),
    });
    let file = RawPayload::Files(vec![fixture("txt", b"hello", None)]);
    let text = payload("0xFF");
    let result =
        engine.detect_preferred(&[text.clone(), image.clone(), file], DetectionMode::Strict);
    assert!(has(&result, D::File));
    assert!(!has(&result, D::Image));
    assert!(!has(&result, D::NumberBase));
    let result = engine.detect_preferred(
        &[text.clone(), image, RawPayload::Files(vec![])],
        DetectionMode::Strict,
    );
    assert!(has(&result, D::Image));
    assert!(has(
        &engine.detect_preferred(&[text], DetectionMode::Strict),
        D::NumberBase
    ));
    assert!(
        engine
            .detect_preferred(&[], DetectionMode::Strict)
            .matches
            .is_empty()
    );
}
#[test]
fn image_header_and_deferred_limit() {
    for (size, expected) in [
        (png().len(), DetectionStatus::Complete),
        (IMAGE_LIMIT, DetectionStatus::Complete),
        (IMAGE_LIMIT + 1, DetectionStatus::Deferred),
    ] {
        let mut bytes = png();
        bytes.resize(size, 0);
        let raw = RawPayload::Image(ImagePayload {
            width: 999,
            height: 999,
            bytes: bytes.into(),
        });
        let result = DetectionEngine::default().detect(&raw, DetectionMode::Strict);
        assert_eq!(result.status, expected);
        assert_eq!(has(&result, D::Image), size <= IMAGE_LIMIT);
    }
}
fn fixture(extension: &str, bytes: &[u8], size: Option<u64>) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "devtools-m3-{}-{}.{extension}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    // 不删除测试文件；唯一临时路径保证并发测试互不覆盖。
    std::fs::write(&path, bytes).unwrap();
    if let Some(size) = size {
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(size)
            .unwrap();
    }
    path
}
fn detect_files(paths: Vec<PathBuf>) -> DetectionResult {
    DetectionEngine::default().detect(&RawPayload::Files(paths), DetectionMode::Strict)
}
#[test]
fn image_file_detector_and_file_parent_suppression() {
    let result = detect_files(vec![fixture("png", &png(), None)]);
    assert!(has(&result, D::ImageFile));
    assert!(
        !result
            .recommendations
            .iter()
            .any(|r| r.tool_id == T::HashHmac)
    );
    let result = detect_files(vec![fixture("png", &png(), Some(IMAGE_LIMIT as u64 + 1))]);
    assert!(!has(&result, D::ImageFile));
    assert_eq!(result.recommendations[0].tool_id, T::HashHmac);
}
#[test]
fn mixed_files_keep_generic_hash_and_relay_file_parent() {
    let image = fixture("png", &png(), None);
    let file = fixture("txt", b"hello", None);
    let result = detect_files(vec![image.clone(), file]);
    assert!(
        result
            .recommendations
            .iter()
            .any(|r| r.tool_id == T::HashHmac)
    );
    assert!(
        result
            .recommendations
            .iter()
            .any(|r| r.tool_id == T::Base64Codec)
    );
    let relay = DetectionEngine::default().detect(
        &RawPayload::Files(vec![image]),
        DetectionMode::Relay {
            source_tool: T::Base64Codec,
        },
    );
    assert!(
        relay
            .recommendations
            .iter()
            .any(|r| r.tool_id == T::HashHmac && r.parent_fallback)
    );
}
#[test]
fn certificate_file_detector_rejects_private_keys_and_csrs() {
    for extension in ["pem", "crt", "cer"] {
        assert!(has(
            &detect_files(vec![fixture(extension, pem().as_bytes(), None)]),
            D::CertificateFile
        ));
        assert!(!has(
            &detect_files(vec![fixture(
                extension,
                b"-----BEGIN RSA PRIVATE KEY-----",
                None
            )]),
            D::CertificateFile
        ));
        assert!(!has(
            &detect_files(vec![fixture(
                extension,
                b"-----BEGIN CERTIFICATE REQUEST-----",
                None
            )]),
            D::CertificateFile
        ));
    }
    for extension in ["der", "pfx", "p12"] {
        assert!(has(
            &detect_files(vec![fixture(extension, &certificate(), None)]),
            D::CertificateFile
        ));
        // PKCS#8 和 CSR 不能靠扩展名伪装成证书。
        let private_key = der(
            0x30,
            &[der(2, &[0]), der(0x30, &[]), der(4, b"secret")].concat(),
        );
        assert!(!has(
            &detect_files(vec![fixture(extension, &private_key, None)]),
            D::CertificateFile
        ));
        let csr = der(
            0x30,
            &[
                der(
                    0x30,
                    &[der(2, &[0]), der(0x30, &[]), der(0x30, &[])].concat(),
                ),
                der(0x30, &[]),
                der(3, &[0]),
            ]
            .concat(),
        );
        assert!(!has(
            &detect_files(vec![fixture(extension, &csr, None)]),
            D::CertificateFile
        ));
    }
    let pfx = der(
        0x30,
        &[
            der(2, &[3]),
            der(
                0x30,
                &der(6, &[0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 1, 7, 1]),
            ),
        ]
        .concat(),
    );
    assert!(has(
        &detect_files(vec![fixture("pfx", &pfx, None)]),
        D::CertificateFile
    ));
}
#[test]
fn files_over_100mb_are_streaming_hash_only() {
    let result = detect_files(vec![fixture(
        "crt",
        pem().as_bytes(),
        Some(STREAMING_FILE_LIMIT + 1),
    )]);
    assert_eq!(result.recommendations.len(), 1);
    assert_eq!(result.recommendations[0].tool_id, T::HashHmac);
    assert!(!has(&result, D::CertificateFile));
    assert!(
        detect_files(vec![PathBuf::from("nonexistent-m3-file")])
            .recommendations
            .is_empty()
    );
}
#[test]
fn fingerprints_deduplicate_content_and_self_copy_without_persistence() {
    let mut dedup = ClipboardDeduplicator::default();
    let a = payload("a");
    let b = payload("b");
    assert_eq!(dedup.observe(1, &a), DedupDecision::Detect);
    assert_eq!(dedup.observe(2, &a), DedupDecision::Unchanged);
    dedup.mark_self_copy(&b);
    assert_eq!(dedup.observe(3, &b), DedupDecision::SelfCopy);
    assert_eq!(dedup.observe(4, &b), DedupDecision::SelfCopy);
    assert_eq!(dedup.observe(5, &a), DedupDecision::Detect);
    assert_eq!(dedup.observe(6, &b), DedupDecision::Detect);
    assert_ne!(
        dedup.fingerprint(0, &a).digest,
        dedup.fingerprint(0, &b).digest
    );
    let files = RawPayload::Files(vec![PathBuf::from("a.png")]);
    assert_eq!(
        dedup.fingerprint(2, &files).file_hint.as_deref(),
        Some(&PathBuf::from("a.png"))
    );
}
