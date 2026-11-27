//! Tool view seam (ADR-0004 dual-track UI).
//!
//! M3/M4 tool UIs plug into [`ToolViewRegistry`] without touching the shell.
//! Until then every tool renders [`PlaceholderToolView`], which already carries
//! the per-instance header (name + instance number, split mode, multi-open,
//! favorite, relay placeholder, copy-and-dismiss) the real views will reuse.

use std::collections::HashMap;

use devtools_core::ToolId;
use gpui_kit::*;

use crate::labels::tool_name;
use crate::theme::Palette;
use crate::workspace::{TabInstanceId, Workbench};

/// Identity of one tool view instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolViewContext {
    pub tool_id: ToolId,
    pub instance: u32,
    pub tab_id: TabInstanceId,
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
                .new(|_| PlaceholderToolView {
                    context,
                    workbench,
                    split: SplitMode::SideBySide,
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
                    .child("发送到 ▸（M3）")
                    .on_click(move |_, _, cx| {
                        wb.update(cx, |state, cx| state.set_status("工具接力将在 M3 接入", cx));
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
                    .child("复制并收起（M4）")
                    .on_click(move |_, _, cx| {
                        wb.update(cx, |state, cx| {
                            state.set_status("M2 壳层占位：工具输出将在 M4 接入", cx)
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

        let body = match split {
            SplitMode::Single => {
                div()
                    .flex_1()
                    .p_3()
                    .child(single_pane(palette, "输入 / 输出", font_size))
            }
            SplitMode::SideBySide => div()
                .flex_1()
                .flex()
                .gap_2()
                .p_3()
                .child(
                    div()
                        .flex_1()
                        .child(single_pane(palette, "输入", font_size)),
                )
                .child(
                    div()
                        .flex_1()
                        .child(single_pane(palette, "输出", font_size)),
                ),
            SplitMode::Stacked => div()
                .flex_1()
                .flex()
                .flex_col()
                .gap_2()
                .p_3()
                .child(
                    div()
                        .flex_1()
                        .child(single_pane(palette, "输入", font_size)),
                )
                .child(
                    div()
                        .flex_1()
                        .child(single_pane(palette, "输出", font_size)),
                ),
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

fn single_pane(palette: Palette, label: &'static str, font_size: f32) -> impl IntoElement {
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
                .child("此处为 M4 工具视图预留区域。M2 只交付壳层、实例会话与生命周期。"),
        )
}
