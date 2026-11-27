//! Native tool views sharing scheduling, session synchronization and editor controls.

mod converters_gen;
mod runtime;
mod standard;
mod text_debug_sec;

pub use runtime::{Debounce, ExecutionState, ToolRuntime, debounce};

use devtools_core::{CancelFlag, ToolError, ToolId, UiTrack};
use gpui_kit::*;

use crate::tool_view::{ToolViewContext, ToolViewFactory, ToolViewRegistry};
use crate::workspace::Workbench;

use converters_gen::ConverterOptions;
use runtime::{Inputs, ToolResult, ToolView};
use standard::StandardOptions;
use text_debug_sec::DebugOptions;

#[derive(Clone)]
enum Model {
    Standard(StandardOptions),
    Converter(ConverterOptions),
    Debug(DebugOptions),
}

impl Model {
    fn new(tool: ToolId) -> Self {
        if tool.ui_track() == UiTrack::StandardTransform {
            Self::Standard(StandardOptions::new(tool))
        } else if let Some(options) = ConverterOptions::new(tool) {
            Self::Converter(options)
        } else {
            Self::Debug(DebugOptions::new(tool))
        }
    }

    fn execute(
        &self,
        inputs: &Inputs,
        cancel: &CancelFlag,
        manual: bool,
    ) -> Result<ToolResult, ToolError> {
        if cancel.is_cancelled() {
            return Err(ToolError::new(
                devtools_core::ToolErrorKind::Cancelled,
                "已取消",
            ));
        }
        match self {
            Self::Standard(options) => options.execute(inputs, cancel),
            Self::Converter(options) => options.execute(inputs, cancel),
            Self::Debug(options) => options.execute(inputs, cancel, manual),
        }
    }
}

struct NativeFactory;

impl ToolViewFactory for NativeFactory {
    fn create(
        &self,
        context: ToolViewContext,
        workbench: WeakEntity<Workbench>,
        cx: &mut App,
    ) -> AnyView {
        cx.new(|cx| ToolView::new(context, workbench, cx)).into()
    }
}

pub(crate) fn register_all(registry: &mut ToolViewRegistry) {
    for tool in ToolId::ALL {
        registry.register(tool, Box::new(NativeFactory));
    }
}

pub(crate) fn release_window(view: &AnyView, cx: &mut App) {
    if let Ok(view) = view.clone().downcast::<ToolView>() {
        view.update(cx, |view, _| view.release_window());
    }
}

#[cfg(test)]
mod tests {
    use super::{Inputs, Model};
    use devtools_core::{CancelFlag, ToolErrorKind, ToolId};

    #[test]
    fn every_native_controller_executes_its_typed_kernel() {
        let cases = [
            (ToolId::DataFormatConverter, r#"{"a":1}"#, "", ""),
            (ToolId::DateTimeConverter, "2026-01-01T00:00:00Z", "", ""),
            (ToolId::NumberBaseConverter, "255", "", ""),
            (ToolId::ColorConverter, "#ff0000", "", ""),
            (ToolId::JsonToTable, r#"[{"a":1}]"#, "", ""),
            (ToolId::Base64Codec, "hello world", "", ""),
            (ToolId::UrlCodec, "https://example.com/?a=1&a=2", "", ""),
            (ToolId::HtmlEntityUnicode, "a&b", "", ""),
            (ToolId::StringEscape, "a\"b", "", ""),
            (ToolId::GzipCompress, "hello world", "", ""),
            (ToolId::JsonFormatter, r#"{"a":1}"#, "", ""),
            (ToolId::SqlFormatter, "select 1", "", ""),
            (ToolId::XmlHtmlFormatter, "<a><b/></a>", "", ""),
            (ToolId::UuidUlidGenerator, "", "", ""),
            (ToolId::PasswordTokenGenerator, "", "", ""),
            (ToolId::QrCodeGenerator, "hello", "", ""),
            (ToolId::TextDiff, "a", "b", ""),
            (ToolId::JsonDiff, r#"{"a":1}"#, r#"{"a":2}"#, ""),
            (ToolId::TextAnalysis, "hello world", "", ""),
            (
                ToolId::ListCompare,
                "a
b",
                "b
c",
                "",
            ),
            (ToolId::RegexTester, "hello world", "", r"\w+"),
            (ToolId::JsonPathTester, r#"{"a":1}"#, "", "$.a"),
            (ToolId::CronParser, "*/5 * * * *", "", ""),
            (ToolId::ChmodCalculator, "755", "", ""),
            (ToolId::HashHmac, "abc", "", ""),
            (ToolId::Ipv4CidrCalculator, "192.168.1.1/24", "", ""),
        ];
        for (tool, text, second, query) in cases {
            let inputs = Inputs {
                text: text.into(),
                second: second.into(),
                query: query.into(),
                ..Inputs::default()
            };
            let out = Model::new(tool)
                .execute(&inputs, &CancelFlag::new(), true)
                .unwrap_or_else(|err| panic!("{tool:?}: {err}"));
            assert!(!out.output.is_empty(), "{tool:?} output empty");
        }
        let token = devtools_tools::security::sign_jwt(
            r#"{"sub":"42"}"#,
            b"secret",
            "HS256".parse().unwrap(),
        )
        .unwrap();
        let jwt = Model::new(ToolId::JwtTool)
            .execute(
                &Inputs {
                    text: token,
                    ..Inputs::default()
                },
                &CancelFlag::new(),
                true,
            )
            .unwrap();
        assert!(jwt.output.contains("42"));
        let cert = Model::new(ToolId::X509Parser).execute(
            &Inputs {
                text: "-----BEGIN PRIVATE KEY-----".into(),
                ..Inputs::default()
            },
            &CancelFlag::new(),
            true,
        );
        assert_eq!(cert.err().unwrap().kind, ToolErrorKind::UnsupportedInput);
        assert_eq!(cases.len() + 2, ToolId::ALL.len());
    }

    #[test]
    fn query_table_preserves_duplicate_keys_and_full_export() {
        let out = Model::new(ToolId::UrlCodec)
            .execute(
                &Inputs {
                    text: "https://example.com/".into(),
                    query: "a=1&a=2&z=3".into(),
                    ..Inputs::default()
                },
                &CancelFlag::new(),
                false,
            )
            .unwrap();
        let (_, rows) = out.table.unwrap();
        assert_eq!(rows, vec![vec!["a", "1"], vec!["a", "2"], vec!["z", "3"]]);
        assert!(out.output.contains("a=1&a=2"));
    }

    #[test]
    fn valid_zero_match_has_distinct_state_from_jsonpath_syntax_error() {
        let model = Model::new(ToolId::JsonPathTester);
        let input = Inputs {
            text: r#"{"a":1}"#.into(),
            query: "$.missing".into(),
            ..Inputs::default()
        };
        let out = model.execute(&input, &CancelFlag::new(), false).unwrap();
        assert!(out.footer.contains("0 命中"));
        assert!(
            model
                .execute(
                    &Inputs {
                        query: "$[".into(),
                        ..input
                    },
                    &CancelFlag::new(),
                    false
                )
                .is_err()
        );
    }

    #[test]
    fn qr_generation_can_relay_into_decoder_without_disk_io() {
        use super::converters_gen::ConverterOptions;
        use devtools_core::{ImagePayload, RawPayload};
        let generated = Model::new(ToolId::QrCodeGenerator)
            .execute(
                &Inputs {
                    text: "offline payload".into(),
                    ..Inputs::default()
                },
                &CancelFlag::new(),
                true,
            )
            .unwrap();
        let bytes = generated.image.unwrap();
        let decoded = Model::Converter(ConverterOptions::Qr {
            mode: 3,
            ec: 1,
            security: 0,
            hidden: false,
        })
        .execute(
            &Inputs {
                payload: Some(RawPayload::Image(ImagePayload {
                    width: 256,
                    height: 256,
                    bytes,
                })),
                ..Inputs::default()
            },
            &CancelFlag::new(),
            true,
        )
        .unwrap();
        assert_eq!(decoded.output, "offline payload");
    }

    #[test]
    fn semantic_and_text_diff_options_change_real_results() {
        use super::text_debug_sec::DebugOptions;
        let input = Inputs {
            text: "Hello ".into(),
            second: "hello".into(),
            ..Inputs::default()
        };
        let out = Model::Debug(DebugOptions::Diff {
            mode: 0,
            whitespace: true,
            case: true,
        })
        .execute(&input, &CancelFlag::new(), false)
        .unwrap();
        assert!(out.footer.contains("+0 / −0"));
        let input = Inputs {
            text: "[1,2]".into(),
            second: "[2,1]".into(),
            ..Inputs::default()
        };
        let out = Model::Debug(DebugOptions::JsonDiff {
            unordered: true,
            canonical: false,
        })
        .execute(&input, &CancelFlag::new(), false)
        .unwrap();
        assert!(out.footer.starts_with("0 处"));
    }

    #[test]
    fn table_preview_is_bounded_while_export_keeps_every_row() {
        let text = format!(
            "[{}]",
            (0..50_001)
                .map(|n| format!("{{\"n\":{n}}}"))
                .collect::<Vec<_>>()
                .join(",")
        );
        let out = Model::new(ToolId::JsonToTable)
            .execute(
                &Inputs {
                    text,
                    ..Inputs::default()
                },
                &CancelFlag::new(),
                true,
            )
            .unwrap();
        assert_eq!(out.table.unwrap().1.len(), 50_000);
        assert!(out.output.contains("50000"));
        assert!(out.footer.contains("截断"));
    }
}
