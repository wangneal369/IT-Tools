//! Realistic, offline sample inputs for all 28 V1 tools.

use devtools_core::ToolId;

#[derive(Debug, Clone, Default)]
pub struct ToolSample {
    pub text: String,
    pub second: String,
    pub query: String,
    pub replacement: String,
    pub secret: String,
    pub expected: String,
}

/// Self-signed test certificate (CN=DevTools, C=CN, valid 2026–2036), zero private key material.
const SAMPLE_CERT_PEM: &str = "-----BEGIN CERTIFICATE-----\n\
MIIDITCCAgmgAwIBAgIUQubP1nFXA5rnkdvl8SLNlYKA+ikwDQYJKoZIhvcNAQEL\n\
BQAwIDERMA8GA1UECgwIRGV2VG9vbHMxCzAJBgNVBAYTAkNOMB4XDTI2MTEyNzEw\n\
MjAyMloXDTM2MTEyNDEwMjAyMlowIDERMA8GA1UECgwIRGV2VG9vbHMxCzAJBgNV\n\
BAYTAkNOMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA2saT7x3nGHyT\n\
FYKKhxE98bGky+0HqdwWKTDK06C8RxyKd711zPgZITfgVtH578dxSm0uEG3bGcw/\n\
rMOfwO0P3GWp311ft29DbjEABFUbiikEM1GN2ITooKPMleA3nb3s9HjvHDJDEzHQ\n\
3xmwh/ds9wL+rHxjYCvWEmvlq4bJiKEW91IkgJQGDmHRiQVh7YIiVFCw7c/vZeRL\n\
kP9ONEe/b9wnoROnjt/dDOYk02nu+1dsfV1zfPdpVlU3im7WRqa/2OYtDGA+8Uew\n\
eADfGtgl45On1S/JuYqRmwfuyqVbleV/qet1N67WHmk1udpB0LmZpl3yiCexEfEF\n\
lznVp48BhwIDAQABo1MwUTAdBgNVHQ4EFgQUbgfzyQ2hoeOx46DhhZE0h/qlmPsw\n\
HwYDVR0jBBgwFoAUbgfzyQ2hoeOx46DhhZE0h/qlmPswDwYDVR0TAQH/BAUwAwEB\n\
/zANBgkqhkiG9w0BAQsFAAOCAQEARVcTuMJZYVlv8viSRrDgBTndnCmUu3SlyweZ\n\
31UtuPnbn10G2oeD3LumcIYxeAkByCcyGraq5Xm8aZieFFehvzxVT22q2eCXOuoa\n\
ePtX0ndlw6eCJ34hzzLUZZWepv9iHJl4grXNkCHo/JY2d0uRpk0X2F5NCkVcyaUh\n\
RmoHrVhXPlirncx8vTumQ5KG2h6HlY0/jFIYDfCwI4usuIMD9j5uqR4rwSroGmHV\n\
tKZsZLAkCFd845Klz2jpCFRJ8cNdOElX83tpPwI+v6IubMFlHee7uj9cRWGWvLMA\n\
Asy/FUZdKD12QjNpEkKBa9GmGFPTFm37C0r84PXCSsIHv6gqng==\n\
-----END CERTIFICATE-----\n";

/// Build a valid sample payload for `tool`.
pub fn sample_for(tool: ToolId) -> ToolSample {
    let text_only = |s: &str| ToolSample {
        text: s.to_owned(),
        ..ToolSample::default()
    };

    match tool {
        ToolId::DataFormatConverter => text_only(
            "{\n  \"service\": \"devtools\",\n  \"version\": 1,\n  \"offline\": true,\n  \"platforms\": [\"windows\", \"macos\", \"linux\"],\n  \"limits\": {\n    \"gzip_max_mb\": 32,\n    \"regex_steps\": 200000\n  }\n}",
        ),
        ToolId::DateTimeConverter => text_only("2026-11-27T21:30:00+08:00"),
        ToolId::NumberBaseConverter => text_only("0xDEADBEEF"),
        ToolId::ColorConverter => text_only("#3b82f6"),
        ToolId::JsonToTable => text_only(
            "[\n  {\"id\": 101, \"module\": \"devtools-core\", \"offline\": true, \"ms\": 0.4},\n  {\"id\": 102, \"module\": \"devtools-detect\", \"offline\": true, \"ms\": 2.1},\n  {\"id\": 103, \"module\": \"devtools-tools\", \"offline\": true, \"ms\": 1.3},\n  {\"id\": 104, \"module\": \"devtools-ui\", \"offline\": true, \"ms\": 3.8}\n]",
        ),
        ToolId::Base64Codec => text_only("DevTools 离线开发效率工作台 · 100% Pure Rust"),
        ToolId::UrlCodec => text_only(
            "https://api.devtools.local:8443/v1/tools/search?q=json+format&tag=offline&tag=rust#top",
        ),
        ToolId::HtmlEntityUnicode => {
            text_only("<section data-app=\"DevTools\">纯离线 &amp; 零遥测 ⚡</section>")
        }
        ToolId::StringEscape => {
            text_only("title = \"DevTools V1\"\npath = C:\\work\\devtools\\config.toml\ttab = true")
        }
        ToolId::GzipCompress => text_only(
            "{\"cluster\":\"prod-cn\",\"nodes\":[\"n1\",\"n2\",\"n3\"],\"healthy\":true,\"interval_ms\":250}",
        ),
        ToolId::JsonFormatter => text_only(
            "{\"app\":\"DevTools\",\"schema_version\":1,\"offline\":true,\"features\":{\"smart_detect\":true,\"single_hop_relay\":true,\"residency\":[\"active\",\"warm-standby\",\"deep-sleep\"]}}",
        ),
        ToolId::SqlFormatter => text_only(
            "select u.id, u.username, count(s.id) as active_sessions from users u inner join sessions s on s.user_id = u.id where u.status = 'active' and s.expires_at > 1764250000 group by u.id, u.username order by active_sessions desc limit 25;",
        ),
        ToolId::XmlHtmlFormatter => text_only(
            "<workbench version=\"1.0\"><sidebar collapsed=\"false\"/><tools count=\"28\"><tool id=\"json-formatter\" track=\"standard\"/><tool id=\"color-converter\" track=\"custom\"/></tools></workbench>",
        ),
        ToolId::UuidUlidGenerator => text_only("01936d9a-7c40-7000-8000-010203040506"),
        ToolId::PasswordTokenGenerator => ToolSample::default(),
        ToolId::QrCodeGenerator => text_only("https://github.com/wangneal369/IT-Tools"),
        ToolId::TextDiff => ToolSample {
            text: "host = \"127.0.0.1\"\nport = 8080\ntimeout_ms = 3000\ndebug = true\n".into(),
            second: "host = \"0.0.0.0\"\nport = 8443\ntimeout_ms = 3000\ntls = true\n".into(),
            ..ToolSample::default()
        },
        ToolId::JsonDiff => ToolSample {
            text: "{\n  \"name\": \"devtools\",\n  \"port\": 8080,\n  \"tags\": [\"rust\", \"gpui\"]\n}"
                .into(),
            second: "{\n  \"port\": 8443,\n  \"name\": \"devtools\",\n  \"tags\": [\"rust\", \"gpui\", \"offline\"]\n}"
                .into(),
            ..ToolSample::default()
        },
        ToolId::TextAnalysis => {
            text_only("userProfileConfig\nhttp_request_timeout\nUserSessionToken\nhttp_request_timeout")
        }
        ToolId::ListCompare => ToolSample {
            text: "devtools-core\ndevtools-detect\ndevtools-tools\ndevtools-ui".into(),
            second: "devtools-tools\ndevtools-search\ndevtools-platform\ndevtools-ui".into(),
            ..ToolSample::default()
        },
        ToolId::RegexTester => ToolSample {
            query: r"(?P<crate>devtools-[a-z]+)\s+v(?P<ver>\d+\.\d+\.\d+)".into(),
            text: "Loaded devtools-core v0.1.0, devtools-tools v0.1.0, and devtools-ui v0.1.0"
                .into(),
            replacement: "[$crate@$ver]".into(),
            ..ToolSample::default()
        },
        ToolId::JsonPathTester => ToolSample {
            query: "$.tools[?@.offline == true].name".into(),
            text: "{\n  \"tools\": [\n    {\"name\": \"JSON 格式化\", \"offline\": true},\n    {\"name\": \"进制转换\", \"offline\": true},\n    {\"name\": \"远程探测\", \"offline\": false}\n  ]\n}".into(),
            ..ToolSample::default()
        },
        ToolId::CronParser => text_only("0 */15 9-18 * * 1-5"),
        ToolId::ChmodCalculator => ToolSample {
            text: "0755".into(),
            query: "/usr/local/bin/devtools".into(),
            ..ToolSample::default()
        },
        ToolId::JwtTool => {
            let secret = "devtools-local-hmac-secret";
            let claims =
                r#"{"sub":"dev-user-42","iss":"io.github.wangneal369.devtools","role":"admin","exp":4102444800}"#;
            let token = devtools_tools::security::sign_jwt(
                claims,
                secret.as_bytes(),
                "HS256".parse().expect("valid alg"),
            )
            .unwrap_or_default();
            ToolSample {
                text: token,
                secret: secret.into(),
                ..ToolSample::default()
            }
        }
        ToolId::HashHmac => {
            let payload = "io.github.wangneal369.devtools@v1.0.0";
            let digest = devtools_tools::security::hash_text(
                payload.as_bytes(),
                devtools_tools::security::HashAlgorithm::Sha256,
                None,
            )
            .hex;
            ToolSample {
                text: payload.into(),
                expected: digest,
                ..ToolSample::default()
            }
        }
        ToolId::X509Parser => text_only(SAMPLE_CERT_PEM),
        ToolId::Ipv4CidrCalculator => text_only("192.168.10.42/26"),
    }
}
