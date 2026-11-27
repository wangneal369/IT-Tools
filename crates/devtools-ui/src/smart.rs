//! Memory-only orchestration for event-triggered detection and single-hop relay.
use devtools_core::{RawPayload, ToolId, ToolLaunchInput};
use devtools_detect::{
    ClipboardDeduplicator, DedupDecision, DetectionEngine, DetectionMode, DetectionResult,
};
use devtools_platform::clipboard::{self, ClipboardReadOutcome};
use devtools_search::{DetectionBoost, SearchIndex, SearchResult};
use gpui_kit::*;

use crate::workspace::{TabInstanceId, TabKind, Workbench};

#[derive(Default)]
pub(crate) struct SmartState {
    pub payload: Option<RawPayload>,
    pub result: DetectionResult,
    pub dedup: ClipboardDeduplicator,
    generation: u64,
    task: Option<Task<()>>,
}
pub(crate) struct RelayState {
    pub source: TabInstanceId,
    pub payload: RawPayload,
    pub result: DetectionResult,
}

impl Workbench {
    pub fn open_tool_with_payload(
        &mut self,
        tool: ToolId,
        input: ToolLaunchInput,
        cx: &mut Context<Self>,
    ) {
        let target = self.tabs.iter().position(|tab| {
            matches!(tab.kind, TabKind::Tool { tool_id, .. } if tool_id == tool)
                && tab.session.read(cx).accepts(&input.payload)
        });
        if let Some(index) = target {
            self.active = index;
        } else {
            self.create_tool(tool, Some(input), cx);
            return;
        }
        let session = self.tabs[self.active].session.clone();
        session.update(cx, |session, cx| {
            session.input = Some(input.payload.clone());
            session.launch_input = Some(input);
            session.output = None;
            cx.notify();
        });
        self.recents.push(tool);
        cx.notify();
    }

    /// Explicit paste deliberately bypasses the automatic-detection preference
    /// and unchanged/self-copy suppression, while retaining all privacy guards.
    pub fn smart_paste(&mut self, explicit: bool, cx: &mut Context<Self>) {
        if !explicit && !self.settings.smart_detection.enabled {
            return;
        }
        self.smart.generation += 1;
        let generation = self.smart.generation;
        let read = cx.background_executor().spawn(async { clipboard::read() });
        self.smart.task = Some(cx.spawn(async move |weak, cx| {
            let outcome = read.await;
            let payload = cx.update(|cx| {
                weak.upgrade().and_then(|wb| {
                    wb.update(cx, |state, cx| {
                        if state.smart.generation != generation {
                            return None;
                        }
                        match outcome {
                            Ok(ClipboardReadOutcome::Payload { sequence, payload }) => {
                                let decision = state.smart.dedup.observe(sequence, &payload);
                                if !explicit && decision != DedupDecision::Detect {
                                    if decision == DedupDecision::SelfCopy {
                                        state.clear_detection(cx);
                                    }
                                    return None;
                                }
                                Some(payload)
                            }
                            Ok(ClipboardReadOutcome::ConcealedSkipped) => {
                                state.clear_detection(cx);
                                state.set_status("剪贴板含隐藏标记，已跳过读取", cx);
                                None
                            }
                            Ok(ClipboardReadOutcome::TooLarge) => {
                                state.clear_detection(cx);
                                state.set_status("剪贴板超过检测大小上限", cx);
                                None
                            }
                            Ok(ClipboardReadOutcome::Empty) => {
                                state.clear_detection(cx);
                                None
                            }
                            Err(_) => {
                                state.clear_detection(cx);
                                state.set_status(
                                    "无法读取剪贴板，请使用显式智能粘贴或检查系统权限",
                                    cx,
                                );
                                None
                            }
                        }
                    })
                })
            });
            let Some(payload) = payload else {
                return;
            };
            let snapshot = payload.clone();
            let result = cx
                .background_executor()
                .spawn(async move {
                    DetectionEngine::default().detect_preferred(&[snapshot], DetectionMode::Strict)
                })
                .await;
            cx.update(|cx| {
                if let Some(wb) = weak.upgrade() {
                    wb.update(cx, |state, cx| {
                        if state.smart.generation == generation
                            && (explicit || state.settings.smart_detection.enabled)
                        {
                            state.smart.payload = Some(payload);
                            state.smart.result = result;
                            state.palette_selected = 0;
                            state.set_status("智能识别完成", cx);
                        }
                    });
                }
            });
        }));
    }

    pub fn clear_detection(&mut self, cx: &mut Context<Self>) {
        self.smart.payload = None;
        self.smart.result = DetectionResult::default();
        cx.notify();
    }
    pub fn cancel_detection(&mut self, cx: &mut Context<Self>) {
        self.smart.generation += 1;
        self.smart.task = None;
        self.clear_detection(cx);
    }
    pub fn recommendation_for(&self, tool: ToolId) -> bool {
        self.smart.result.palette().any(|r| r.tool_id == tool)
            || (!self.smart.result.suppress_auto_prompt
                && self
                    .smart
                    .result
                    .recommendations
                    .iter()
                    .any(|r| r.tool_id == tool && r.confidence.is_banner()))
    }
    pub fn launch_recommendation(&mut self, tool: ToolId, cx: &mut Context<Self>) {
        let input = self.smart.payload.as_ref().and_then(|p| {
            self.smart
                .result
                .recommendations
                .iter()
                .find(|r| r.tool_id == tool)
                .map(|r| r.launch_input(p))
        });
        if let Some(input) = input {
            self.open_tool_with_payload(tool, input, cx);
        }
    }
    pub fn open_palette(&mut self, cx: &mut Context<Self>) {
        self.palette_open = true;
        self.palette_query.clear();
        self.palette_selected = 0;
        self.palette_epoch += 1;
        cx.notify();
    }
    pub fn close_palette(&mut self, cx: &mut Context<Self>) {
        self.palette_open = false;
        cx.notify();
    }
    pub fn palette_results(&self) -> Vec<SearchResult> {
        let boosts: Vec<_> = self
            .smart
            .result
            .palette()
            .map(|r| DetectionBoost {
                tool_id: r.tool_id,
                confidence: r.confidence,
            })
            .collect();
        SearchIndex::new().search_with(
            &self.palette_query,
            &boosts,
            &self.settings.navigation.favorites,
        )
    }
    pub fn move_palette(&mut self, delta: isize, cx: &mut Context<Self>) {
        let length = self.palette_results().len();
        self.palette_selected = palette_selection(self.palette_selected, delta, length);
        cx.notify();
    }
    pub fn activate_palette(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(result) = self.palette_results().get(index) {
            let tool = result.descriptor.id;
            let detection = result.group == devtools_search::SearchGroup::Detection;
            self.close_palette(cx);
            if detection {
                self.launch_recommendation(tool, cx);
            } else {
                self.open_tool(tool, cx);
            }
        }
    }
    pub fn open_relay(&mut self, source: TabInstanceId, cx: &mut Context<Self>) {
        self.relay = None;
        let Some(tab) = self.tabs.iter().find(|tab| tab.id == source) else {
            return;
        };
        let TabKind::Tool { tool_id, .. } = tab.kind else {
            return;
        };
        let Some(payload) = tab.session.read(cx).output.clone() else {
            self.set_status("当前工具尚无输出", cx);
            return;
        };
        let snapshot = payload.clone();
        cx.spawn(async move |weak, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    DetectionEngine::default().detect(
                        &snapshot,
                        DetectionMode::Relay {
                            source_tool: tool_id,
                        },
                    )
                })
                .await;
            cx.update(|cx| {
                if let Some(wb) = weak.upgrade() {
                    wb.update(cx, |state, cx| {
                        if state.active_tab().id == source
                            && state.active_tab().session.read(cx).output.as_ref() == Some(&payload)
                        {
                            state.relay = Some(RelayState {
                                source,
                                payload,
                                result,
                            });
                            cx.notify();
                        }
                    });
                }
            });
        })
        .detach();
    }
    pub fn launch_relay(&mut self, tool: ToolId, cx: &mut Context<Self>) {
        if let Some(relay) = self.relay.take() {
            let current = self
                .tabs
                .iter()
                .find(|t| t.id == relay.source)
                .and_then(|t| t.session.read(cx).output.as_ref());
            if current != Some(&relay.payload) {
                return;
            }
            if let Some(recommendation) = relay
                .result
                .recommendations
                .iter()
                .find(|r| r.tool_id == tool)
            {
                self.open_tool_with_payload(tool, recommendation.launch_input(&relay.payload), cx);
            }
        }
        cx.notify();
    }
    pub fn copy_output(&mut self, source: TabInstanceId, dismiss: bool, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.iter().find(|t| t.id == source) else {
            return;
        };
        let session = tab.session.read(cx);
        let Some(RawPayload::Text(text)) = session.output.clone() else {
            self.set_status("当前工具没有可复制的文本输出", cx);
            return;
        };
        let sensitive = session.sensitive_output
            && self
                .settings
                .smart_detection
                .exclude_sensitive_from_clipboard_history;
        let payload = RawPayload::Text(text.clone());
        cx.spawn(async move |weak, cx| {
            let written = cx
                .background_executor()
                .spawn(async move { clipboard::write_text(&text, sensitive) })
                .await;
            cx.update(|cx| {
                if let Some(wb) = weak.upgrade() {
                    wb.update(cx, |state, cx| {
                        if written.is_ok() {
                            state.smart.dedup.mark_self_copy(&payload);
                            state.cancel_detection(cx);
                            state.set_status("已复制输出", cx);
                        } else {
                            state.set_status("复制失败，系统剪贴板不可用", cx);
                        }
                    });
                    if written.is_ok() && dismiss {
                        crate::window::hide_to_tray(&wb, cx);
                    }
                }
            });
        })
        .detach();
    }
}

pub(crate) fn palette_selection(current: usize, delta: isize, length: usize) -> usize {
    if length == 0 {
        0
    } else {
        (current as isize + delta).rem_euclid(length as isize) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::palette_selection;
    use crate::workspace::ToolSession;
    use devtools_core::RawPayload;

    #[test]
    fn palette_navigation_wraps_and_handles_empty_results() {
        assert_eq!(palette_selection(0, -1, 3), 2);
        assert_eq!(palette_selection(2, 1, 3), 0);
        assert_eq!(palette_selection(0, 1, 0), 0);
    }
    #[test]
    fn palette_boosts_detection_before_favorites_and_filters_pinyin() {
        use devtools_core::{Confidence, ToolId};
        use devtools_search::{DetectionBoost, SearchGroup, SearchIndex};
        let mut index = SearchIndex::new();
        let boosts = [DetectionBoost {
            tool_id: ToolId::JsonFormatter,
            confidence: Confidence::High,
        }];
        let results = index.search_with("", &boosts, &[ToolId::Base64Codec]);
        assert_eq!(results[0].group, SearchGroup::Detection);
        assert_eq!(results[1].group, SearchGroup::Favorite);
        assert_eq!(
            index.search_with("shijianchuo", &boosts, &[])[0]
                .descriptor
                .id,
            ToolId::DateTimeConverter
        );
    }

    #[test]
    fn relay_launch_preserves_payload_and_excludes_source() {
        use devtools_core::ToolId;
        use devtools_detect::{DetectionEngine, DetectionMode};
        let payload = RawPayload::Text("[{\"id\":1}]".into());
        let result = DetectionEngine::default().detect(
            &payload,
            DetectionMode::Relay {
                source_tool: ToolId::JsonToTable,
            },
        );
        assert!(
            !result
                .recommendations
                .iter()
                .any(|r| r.tool_id == ToolId::JsonToTable)
        );
        let formatter = result
            .recommendations
            .iter()
            .find(|r| r.tool_id == ToolId::JsonFormatter)
            .expect("parent generalization");
        let input = formatter.launch_input(&payload);
        assert_eq!(input.payload, payload);
        assert!(!input.chain.nodes.is_empty());
    }

    #[test]
    fn injection_reuses_empty_or_identical_and_preserves_edits() {
        let mut session = ToolSession::default();
        let payload = RawPayload::Text("{\"hello\":1}".into());
        assert!(session.accepts(&payload));
        session.input = Some(RawPayload::Text("".into()));
        assert!(session.accepts(&payload));
        session.input = Some(payload.clone());
        assert!(session.accepts(&payload));
        session.input = Some(RawPayload::Text("user edits".into()));
        assert!(!session.accepts(&payload));
    }
}
