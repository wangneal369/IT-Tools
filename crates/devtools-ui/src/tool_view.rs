//! Tool view seam (ADR-0004 dual-track UI).
//!
//! All 28 V1 tools are explicitly registered by [`ToolViewRegistry::with_all_v1_tools`].
//! An empty registry retains the shell-only fallback for external factory users.

use std::collections::HashMap;

use devtools_core::ToolId;
use gpui_kit::*;

use crate::labels::tool_name;
use crate::theme::Palette;
use crate::workspace::{TabInstanceId, Workbench};

/// Identity of one tool view instance.
#[derive(Clone)]
pub struct ToolViewContext {
    pub tool_id: ToolId,
    pub instance: u32,
    pub tab_id: TabInstanceId,
    /// Shared memory-only session. Observe it to receive subsequent injections;
    /// publish edits/output here so anti-overwrite and relay see current state.
    pub session: Entity<crate::workspace::ToolSession>,
}

/// Factory contract implemented by M4 tool views.
pub trait ToolViewFactory: 'static {
    fn create(
        &self,
        context: ToolViewContext,
        workbench: WeakEntity<Workbench>,
        cx: &mut App,
    ) -> AnyView;
}

/// Maps tools to view factories; everything defaults to the placeholder.
#[derive(Default)]
pub struct ToolViewRegistry {
    overrides: HashMap<ToolId, Box<dyn ToolViewFactory>>,
}

impl ToolViewRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register every V1 tool with its native, kernel-backed view.
    pub fn with_all_v1_tools() -> Self {
        let mut registry = Self::new();
        crate::views::register_all(&mut registry);
        registry
    }

    pub fn is_registered(&self, tool: ToolId) -> bool {
        self.overrides.contains_key(&tool)
    }

    pub fn register(&mut self, tool: ToolId, factory: Box<dyn ToolViewFactory>) {
        self.overrides.insert(tool, factory);
    }

    pub fn create(
        &self,
        context: ToolViewContext,
        workbench: WeakEntity<Workbench>,
        cx: &mut App,
    ) -> AnyView {
        match self.overrides.get(&context.tool_id) {
            Some(factory) => factory.create(context, workbench, cx),
            None => cx
                .new(|cx| {
                    let subscription = cx.observe(&context.session, |_, _, cx| cx.notify());
                    PlaceholderToolView {
                        context,
                        workbench,
                        split: SplitMode::SideBySide,
                        _session_subscription: subscription,
                    }
                })
                .into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitMode {
    SideBySide,
    Stacked,
    Single,
}

impl SplitMode {
    fn label(self) -> &'static str {
        match self {
            Self::SideBySide => "左右",
            Self::Stacked => "上下",
            Self::Single => "单栏",
        }
    }
}

/// Honest M2 placeholder: shell, header and per-instance state only.
pub struct PlaceholderToolView {
    context: ToolViewContext,
    workbench: WeakEntity<Workbench>,
    split: SplitMode,
    _session_subscription: Subscription,
}

impl Render for PlaceholderToolView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(workbench) = self.workbench.upgrade() else {
            return div().child("工作台已关闭").into_any_element();
        };
        let (palette, font_size, favorite) = {
            let state = workbench.read(cx);
            (
                crate::theme::resolve(state.settings().theme, window),
                state.settings().editor.font_size as f32,
                state.is_favorite(self.context.tool_id),
            )
        };

        let tool_id = self.context.tool_id;
        let instance = self.context.instance;
        let split = self.split;
        let tab_key = self.context.tab_id.0;
        let view = cx.entity();

        let mut header_actions = div().flex().items_center().gap_2();
        for (index, mode) in [SplitMode::SideBySide, SplitMode::Stacked, SplitMode::Single]
            .into_iter()
            .enumerate()
        {
            let is_active = mode == split;
            let view = view.clone();
            header_actions = header_actions.child(
                div()
                    .id(format!("split-{tab_key}-{index}"))
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .text_xs()
                    .cursor_pointer()
                    .bg(if is_active {
                        rgb(palette.accent_soft)
                    } else {
                        rgb(palette.panel_alt)
                    })
                    .text_color(if is_active {
                        rgb(palette.accent)
                    } else {
                        rgb(palette.muted)
                    })
                    .child(mode.label())
                    .on_click(move |_, _, cx| {
                        view.update(cx, |this, cx| {
                            this.split = mode;
                            cx.notify();
                        });
                    }),
            );
        }

        {
            let wb = workbench.clone();
            header_actions = header_actions.child(
                div()
                    .id(format!("favorite-{tab_key}"))
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .text_xs()
                    .cursor_pointer()
                    .bg(rgb(palette.panel_alt))
                    .text_color(if favorite {
                        rgb(palette.accent)
                    } else {
                        rgb(palette.muted)
                    })
                    .child(if favorite {
                        "★ 已收藏"
                    } else {
                        "☆ 收藏"
                    })
                    .on_click(move |_, _, cx| {
                        wb.update(cx, |state, cx| state.toggle_favorite(tool_id, cx));
                    }),
            );
        }
        {
            let wb = workbench.clone();
            header_actions = header_actions.child(
                div()
                    .id(format!("multi-open-{tab_key}"))
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .text_xs()
                    .cursor_pointer()
                    .bg(rgb(palette.panel_alt))
                    .text_color(rgb(palette.text))
                    .child("+ 多开")
                    .on_click(move |_, _, cx| {
                        wb.update(cx, |state, cx| state.open_tool(tool_id, cx));
                    }),
            );
        }
        {
            let wb = workbench.clone();
            header_actions = header_actions.child(
                div()
                    .id(format!("relay-{tab_key}"))
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .text_xs()
                    .bg(rgb(palette.panel_alt))
                    .text_color(rgb(palette.muted))
                    .cursor_pointer()
                    .child("发送到 ▸")
                    .on_click(move |_, _, cx| {
                        wb.update(cx, |state, cx| state.open_relay(TabInstanceId(tab_key), cx));
                    }),
            );
        }
        {
            let wb = workbench.clone();
            header_actions = header_actions.child(
                div()
                    .id(format!("copy-dismiss-{tab_key}"))
                    .px_3()
                    .py_1()
                    .rounded_sm()
                    .text_xs()
                    .cursor_pointer()
                    .bg(rgb(palette.accent))
                    .text_color(rgb(0xffffff))
                    .child("复制并收起")
                    .on_click(move |_, _, cx| {
                        wb.update(cx, |state, cx| {
                            state.copy_output(TabInstanceId(tab_key), true, cx)
                        });
                    }),
            );
        }

        let header = div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(44.0))
            .px_3()
            .border_b_1()
            .border_color(rgb(palette.border))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(tool_name(tool_id)),
                    )
                    .child(
                        div()
                            .px_2()
                            .py_1()
                            .rounded_full()
                            .bg(rgb(palette.accent_soft))
                            .text_xs()
                            .text_color(rgb(palette.accent))
                            .child(format!("#{instance}")),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(palette.muted))
                            .child("实例级独立会话（内存态）"),
                    ),
            )
            .child(header_actions);

        let session = self.context.session.read(cx);
        let input_preview = session
            .input
            .as_ref()
            .map(payload_preview)
            .unwrap_or_else(|| "等待输入或智能粘贴".into());
        let output_preview = session
            .output
            .as_ref()
            .map(payload_preview)
            .unwrap_or_else(|| "工具计算内核将在 M4 接入".into());
        let body = match split {
            SplitMode::Single => div().flex_1().p_3().child(single_pane(
                palette,
                "输入 / 输出",
                font_size,
                input_preview.clone(),
            )),
            SplitMode::SideBySide => div()
                .flex_1()
                .flex()
                .gap_2()
                .p_3()
                .child(div().flex_1().child(single_pane(
                    palette,
                    "输入",
                    font_size,
                    input_preview.clone(),
                )))
                .child(div().flex_1().child(single_pane(
                    palette,
                    "输出",
                    font_size,
                    output_preview.clone(),
                ))),
            SplitMode::Stacked => div()
                .flex_1()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .child(div().flex_1().child(single_pane(
                    palette,
                    "输入",
                    font_size,
                    input_preview.clone(),
                )))
                .child(div().flex_1().child(single_pane(
                    palette,
                    "输出",
                    font_size,
                    output_preview.clone(),
                ))),
        };

        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(palette.bg))
            .text_color(rgb(palette.text))
            .child(header)
            .child(body)
            .into_any_element()
    }
}

fn single_pane(
    palette: Palette,
    label: &'static str,
    font_size: f32,
    preview: String,
) -> impl IntoElement {
    div()
        .size_full()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(rgb(palette.border))
        .bg(rgb(palette.panel_alt))
        .child(div().text_xs().text_color(rgb(palette.muted)).child(label))
        .child(
            div()
                .flex_1()
                .text_size(px(font_size))
                .text_color(rgb(palette.muted))
                .child(preview),
        )
}

fn payload_preview(payload: &devtools_core::RawPayload) -> String {
    match payload {
        devtools_core::RawPayload::Text(text) => text.chars().take(4096).collect(),
        devtools_core::RawPayload::Files(paths) => format!("{} 个文件", paths.len()),
        devtools_core::RawPayload::Image(image) => {
            format!("图片 {} × {}", image.width, image.height)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ToolViewRegistry;
    use devtools_core::ToolId;

    #[test]
    fn all_twenty_eight_tools_have_native_factories() {
        let registry = ToolViewRegistry::with_all_v1_tools();
        assert_eq!(registry.overrides.len(), 28);
        for tool in ToolId::ALL {
            assert!(registry.is_registered(tool), "{tool:?} uses fallback");
        }
        assert!(!ToolViewRegistry::new().is_registered(ToolId::JsonFormatter));
    }
}
