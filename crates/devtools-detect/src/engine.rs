use std::{
    fs::File,
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

use crate::{assess_privacy, redacted_preview, text};
use devtools_core::{
    Confidence, DataType, MatchedDataChain, MatchedDataNode, ParsedHint, RawPayload, ToolId,
    ToolLaunchInput,
};

pub const SOFT_TARGET: Duration = Duration::from_millis(30);
pub const HARD_TIMEOUT: Duration = Duration::from_millis(300);
pub const FULL_TEXT_LIMIT: usize = 256 * 1024;
pub const TEXT_LIMIT: usize = 2 * 1024 * 1024;
pub const MAGIC_PREFIX_LIMIT: usize = 64 * 1024;
pub const IMAGE_LIMIT: usize = 20 * 1024 * 1024;
pub const STREAMING_FILE_LIMIT: u64 = 100 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DetectionMode {
    #[default]
    Strict,
    Relay {
        source_tool: ToolId,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DetectionStatus {
    #[default]
    Complete,
    PrefixOnly,
    Skipped,
    Deferred,
    TimedOut,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recommendation {
    pub tool_id: ToolId,
    pub data_type: DataType,
    pub confidence: Confidence,
    pub chain: MatchedDataChain,
    pub parent_fallback: bool,
}

impl Recommendation {
    /// 保留同一共享原始快照与完整匹配路径，供工具提取准确层级。
    pub fn launch_input(&self, payload: &RawPayload) -> ToolLaunchInput {
        ToolLaunchInput {
            payload: payload.clone(),
            chain: self.chain.clone(),
            hint: Some(ParsedHint::new(self.data_type, self.confidence)),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DetectionResult {
    pub matches: Vec<MatchedDataChain>,
    pub recommendations: Vec<Recommendation>,
    pub status: DetectionStatus,
    pub suppress_auto_prompt: bool,
    pub preview: String,
    pub elapsed: Duration,
}

impl DetectionResult {
    pub fn banner(&self) -> impl Iterator<Item = &Recommendation> {
        self.visible().take(3)
    }

    pub fn palette(&self) -> impl Iterator<Item = &Recommendation> {
        self.visible().take(5)
    }

    fn visible(&self) -> impl Iterator<Item = &Recommendation> {
        self.recommendations
            .iter()
            .filter(|r| !self.suppress_auto_prompt && r.confidence.is_banner())
    }
}

/// 静态 DAG 依赖声明，JsonArray 是唯一多层文本叶子。
#[derive(Debug, Clone, Copy)]
pub struct DetectorSpec {
    pub output: DataType,
    pub parent: DataType,
}

pub const DETECTORS: &[DetectorSpec] = &[
    DetectorSpec {
        output: DataType::File,
        parent: DataType::Files,
    },
    DetectorSpec {
        output: DataType::ImageFile,
        parent: DataType::File,
    },
    DetectorSpec {
        output: DataType::CertificateFile,
        parent: DataType::File,
    },
    DetectorSpec {
        output: DataType::Jwt,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::CertificatePem,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::Base64Image,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::GZipBase64,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::Json,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::JsonArray,
        parent: DataType::Json,
    },
    DetectorSpec {
        output: DataType::XmlHtml,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::Yaml,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::Toml,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::Url,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::UrlEncoded,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::DateTime,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::UnixTimestamp,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::Cron,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::Ipv4Cidr,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::Color,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::Base64Text,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::HtmlUnicode,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::EscapedText,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::Sql,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::Chmod,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::NumberBase,
        parent: DataType::Text,
    },
    DetectorSpec {
        output: DataType::HashDigest,
        parent: DataType::Text,
    },
];

#[derive(Debug, Clone)]
pub struct DetectionEngine {
    budget: Duration,
}

impl Default for DetectionEngine {
    fn default() -> Self {
        Self {
            budget: HARD_TIMEOUT,
        }
    }
}

impl DetectionEngine {
    /// 仅允许收紧硬预算，便于调用方取消以及确定性超时测试。
    pub fn with_budget(budget: Duration) -> Self {
        Self {
            budget: budget.min(HARD_TIMEOUT),
        }
    }

    /// 已由平台读取的多格式快照按 Files > Image > Text 选择，空文件列表不遮蔽其他格式。
    pub fn detect_preferred(
        &self,
        payloads: &[RawPayload],
        mode: DetectionMode,
    ) -> DetectionResult {
        let preferred = payloads
            .iter()
            .filter(|p| !matches!(p, RawPayload::Files(paths) if paths.is_empty()))
            .min_by_key(|p| match p {
                RawPayload::Files(_) => 0,
                RawPayload::Image(_) => 1,
                RawPayload::Text(_) => 2,
            });
        preferred.map_or_else(DetectionResult::default, |p| self.detect(p, mode))
    }

    pub fn detect(&self, payload: &RawPayload, mode: DetectionMode) -> DetectionResult {
        let started = Instant::now();
        let deadline = started + self.budget;
        let mut result = DetectionResult::default();
        if Instant::now() >= deadline {
            result.status = DetectionStatus::TimedOut;
        } else {
            match payload {
                RawPayload::Text(value) => self.detect_text(value, deadline, &mut result),
                RawPayload::Image(image) => {
                    if image.bytes.len() > IMAGE_LIMIT {
                        result.status = DetectionStatus::Deferred;
                    } else if imagesize::blob_size(
                        &image.bytes[..image.bytes.len().min(MAGIC_PREFIX_LIMIT)],
                    )
                    .is_ok()
                    {
                        result
                            .matches
                            .push(chain(DataType::Image, &[], Confidence::Exact));
                    }
                }
                RawPayload::Files(paths) => {
                    // 限定文件数量与读取字节数；不递归、不解码图像、不读取大文件正文。
                    for path in paths.iter().take(64) {
                        if Instant::now() >= deadline {
                            break;
                        }
                        detect_file(path, &mut result);
                    }
                    if paths.len() > 64 {
                        result.status = DetectionStatus::Deferred;
                    }
                }
            }
            if Instant::now() >= deadline {
                result.status = DetectionStatus::TimedOut;
            }
        }
        suppress(&mut result.matches);
        result.recommendations = recommend(&result.matches, mode);
        result.elapsed = started.elapsed();
        result
    }

    fn detect_text(&self, value: &str, deadline: Instant, result: &mut DetectionResult) {
        if value.len() > TEXT_LIMIT {
            result.status = DetectionStatus::Skipped;
            return;
        }
        let privacy = assess_privacy(value);
        result.suppress_auto_prompt = privacy.suppress_auto_prompt;
        result.preview = redacted_preview(value);
        if privacy.private_key {
            return;
        }
        let prefix_only = value.len() > FULL_TEXT_LIMIT;
        let value = if prefix_only {
            result.status = DetectionStatus::PrefixOnly;
            &value[..value.floor_char_boundary(MAGIC_PREFIX_LIMIT)]
        } else {
            value
        };
        let value = value.trim();
        for spec in DETECTORS.iter().filter(|s| s.parent == DataType::Text) {
            if Instant::now() >= deadline {
                break;
            }
            if prefix_only
                && !matches!(
                    spec.output,
                    DataType::Jwt
                        | DataType::CertificatePem
                        | DataType::Base64Image
                        | DataType::GZipBase64
                )
            {
                continue;
            }
            if let Some(confidence) = text::detect(spec.output, value, prefix_only) {
                result
                    .matches
                    .push(chain(DataType::Text, &[spec.output], confidence));
                if spec.output == DataType::Json && text::json_array(value) {
                    result.matches.push(chain(
                        DataType::Text,
                        &[DataType::Json, DataType::JsonArray],
                        Confidence::Exact,
                    ));
                }
            }
        }
    }
}

pub(crate) fn chain(
    root: DataType,
    children: &[DataType],
    confidence: Confidence,
) -> MatchedDataChain {
    MatchedDataChain::new(
        std::iter::once(root)
            .chain(children.iter().copied())
            .map(|data_type| MatchedDataNode::new(data_type, confidence))
            .collect(),
    )
}

fn detect_file(path: &Path, result: &mut DetectionResult) {
    let Ok(metadata) = path.metadata() else {
        return;
    };
    if !metadata.is_file() {
        return;
    }
    let generic = chain(DataType::Files, &[DataType::File], Confidence::Exact);
    if metadata.len() > STREAMING_FILE_LIMIT {
        result.matches.push(generic);
        return;
    }
    let Ok(file) = File::open(path) else {
        result.matches.push(generic);
        return;
    };
    let mut header = Vec::new();
    if file
        .take(MAGIC_PREFIX_LIMIT as u64)
        .read_to_end(&mut header)
        .is_err()
    {
        result.matches.push(generic);
        return;
    }
    let extension = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let prefix = String::from_utf8_lossy(&header);
    let privacy = assess_privacy(&prefix);
    result.suppress_auto_prompt |= privacy.suppress_auto_prompt;
    let image = metadata.len() <= IMAGE_LIMIT as u64 && imagesize::blob_size(&header).is_ok();
    let certificate = matches!(
        extension.as_str(),
        "crt" | "cer" | "der" | "pem" | "pfx" | "p12"
    ) && !privacy.private_key
        && !privacy.certificate_request
        && (text::certificate_pem(&prefix, metadata.len() > MAGIC_PREFIX_LIMIT as u64)
            || text::certificate_der(&header)
            || (matches!(extension.as_str(), "pfx" | "p12") && text::pkcs12(&header)));
    // 每个文件只产出叶子路径，避免另一个文件的叶子遮蔽普通文件。
    if image {
        result.matches.push(chain(
            DataType::Files,
            &[DataType::File, DataType::ImageFile],
            Confidence::Exact,
        ));
    } else if certificate {
        result.matches.push(chain(
            DataType::Files,
            &[DataType::File, DataType::CertificateFile],
            Confidence::High,
        ));
    } else {
        result.matches.push(generic);
    }
}

fn leaf(chain: &MatchedDataChain) -> DataType {
    chain.leaf().expect("非空匹配路径").data_type
}

pub(crate) fn suppress(matches: &mut Vec<MatchedDataChain>) {
    let has = |t| matches.iter().any(|m| leaf(m) == t);
    let base64 = [
        DataType::Jwt,
        DataType::GZipBase64,
        DataType::Base64Image,
        DataType::CertificatePem,
        DataType::Url,
        DataType::HashDigest,
    ]
    .into_iter()
    .any(has);
    let json = has(DataType::Json) || has(DataType::JsonArray);
    let url = has(DataType::Url);
    let number = has(DataType::UnixTimestamp) || has(DataType::HashDigest);
    matches.retain(|m| match leaf(m) {
        DataType::Base64Text => !base64,
        DataType::Yaml | DataType::EscapedText => !json,
        DataType::HtmlUnicode => !url,
        // HashDigest 仅由无前缀十六进制构成，不会与有前缀进制同时命中。
        DataType::NumberBase => !number,
        _ => true,
    });
}

fn tools(data_type: DataType) -> &'static [ToolId] {
    use DataType as D;
    use ToolId as T;
    match data_type {
        D::Text => &[
            T::TextAnalysis,
            T::TextDiff,
            T::HashHmac,
            T::Base64Codec,
            T::QrCodeGenerator,
        ],
        D::Files | D::File => &[T::HashHmac],
        D::Image | D::ImageFile => &[T::Base64Codec, T::QrCodeGenerator],
        D::CertificateFile | D::CertificatePem => &[T::X509Parser],
        D::Jwt => &[T::JwtTool],
        D::Base64Image => &[T::Base64Codec, T::QrCodeGenerator],
        D::GZipBase64 => &[T::GzipCompress],
        D::Json => &[
            T::JsonFormatter,
            T::DataFormatConverter,
            T::JsonPathTester,
            T::JsonDiff,
        ],
        D::JsonArray => &[T::JsonToTable, T::JsonFormatter],
        D::XmlHtml => &[T::XmlHtmlFormatter, T::DataFormatConverter],
        D::Yaml | D::Toml => &[T::DataFormatConverter],
        D::Url | D::UrlEncoded => &[T::UrlCodec],
        D::DateTime | D::UnixTimestamp => &[T::DateTimeConverter],
        D::Cron => &[T::CronParser],
        D::Ipv4Cidr => &[T::Ipv4CidrCalculator],
        D::Color => &[T::ColorConverter],
        D::Base64Text => &[T::Base64Codec],
        D::HtmlUnicode => &[T::HtmlEntityUnicode],
        D::EscapedText => &[T::StringEscape],
        D::Sql => &[T::SqlFormatter],
        D::Chmod => &[T::ChmodCalculator],
        D::NumberBase => &[T::NumberBaseConverter],
        D::HashDigest => &[T::HashHmac],
    }
}

fn recommend(matches: &[MatchedDataChain], mode: DetectionMode) -> Vec<Recommendation> {
    let mut leaves: Vec<_> = matches
        .iter()
        .filter(|m| {
            !matches.iter().any(|other| {
                leaf(m) != DataType::File
                    && other.nodes.len() > m.nodes.len()
                    && other.nodes[..m.nodes.len()] == m.nodes
            })
        })
        .collect();
    leaves.sort_by_key(|m| std::cmp::Reverse(m.leaf().expect("非空路径").confidence));
    let mut out = Vec::new();
    for m in &leaves {
        append_tools(&mut out, m, false, mode);
    }
    if matches!(mode, DetectionMode::Relay { .. }) {
        for m in leaves {
            if m.nodes.len() > 1 {
                let parent = MatchedDataChain::new(m.nodes[..m.nodes.len() - 1].to_vec());
                append_tools(&mut out, &parent, true, mode);
            }
        }
    }
    out
}

fn append_tools(
    out: &mut Vec<Recommendation>,
    chain: &MatchedDataChain,
    parent_fallback: bool,
    mode: DetectionMode,
) {
    let node = chain.leaf().expect("非空路径");
    for &tool_id in tools(node.data_type) {
        if matches!(mode, DetectionMode::Relay { source_tool } if source_tool == tool_id)
            || out.iter().any(|r| r.tool_id == tool_id)
        {
            continue;
        }
        out.push(Recommendation {
            tool_id,
            data_type: node.data_type,
            confidence: node.confidence,
            chain: chain.clone(),
            parent_fallback,
        });
    }
}
