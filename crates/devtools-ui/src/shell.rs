//! A + B fused workbench shell (spec §3.2).
//!
//! Layout: collapsible category rail (with flyout when collapsed), tab bar in
//! multi-row-grid or single-row-scroll mode, the active tool/home canvas, the
//! 26 px status bar and the settings drawer. The shell is a thin projection of
//! the window-independent [`Workbench`] entity.

use devtools_core::{Category, TabBarMode, ToolId};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::*;

use crate::labels::{category_glyph, category_label, tool_hint, tool_name, tools_in};
use crate::settings;
use crate::theme::{self, Palette};
use crate::workspace::{HomeView, Phase, TabKind, Workbench, geometry_from_bounds};

pub struct WorkbenchView {
    workbench: Entity<Workbench>,
    flyout: Option<Category>,
    _bounds_subscription: Subscription,
}

impl WorkbenchView {
    pub fn new(workbench: Entity<Workbench>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let observed = workbench.clone();
        let bounds_subscription = cx.observe_window_bounds(window, move |_view, window, cx| {
            let geometry = geometry_from_bounds(window.window_bounds());
            let maximized = window.is_maximized();
            observed.update(cx, |state, _| state.note_geometry(geometry, maximized));
        });

        Self {
            workbench,
            flyout: None,
            _bounds_subscription: bounds_subscription,
        }
    }
}

impl Render for WorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let workbench = self.workbench.clone();
        let view = cx.entity();

        let (
            palette,
            base_font,
            collapsed_setting,
            tab_bar_mode,
            settings_open,
            phase,
            status_line,
            hotkey_label,
            tab_count,
        ) = {
            let state = workbench.read(cx);
            (
                theme::resolve(state.settings().theme, window),
                state.settings().editor.font_size as f32,
                state.settings().layout.sidebar_collapsed,
                state.settings().layout.tab_bar_mode,
                state.is_settings_open(),
                state.phase(),
                state.status_line().to_owned(),
                state.platform().hotkey_status().label(),
                state.tabs().len(),
            )
        };

        let collapsed = collapsed_setting || window.viewport_size().width.as_f32() <= 820.0;

        let rail = render_rail(&workbench, &view, palette, collapsed, cx);
        let tab_bar = render_tab_bar(&workbench, palette, tab_bar_mode, cx);
        let content = render_content(&workbench, palette, cx);

        let mut main = div().flex_1().flex().overflow_hidden().child(rail).child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .overflow_hidden()
                .child(tab_bar)
                .child(content),
        );

        if settings_open {
            main = main.child(settings::panel(&workbench, palette, cx));
        }

        let status = div()
            .h(px(26.0))
            .px_3()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .bg(rgb(palette.panel))
            .border_t_1()
            .border_color(rgb(palette.border))
            .text_xs()
            .text_color(rgb(palette.muted))
            .child(format!("{} · {} 个标签", phase.label(), tab_count))
            .child(div().flex_1().truncate().child(status_line))
            .child(format!("热键：{hotkey_label}"));

        let flyout = if collapsed {
            self.flyout
                .map(|category| render_flyout(&workbench, &view, palette, category))
        } else {
            None
        };

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(palette.bg))
            .text_color(rgb(palette.text))
            .text_size(px(base_font.max(12.0)))
            .child(main)
            .child(status)
            .children(flyout)
    }
}

fn render_rail(
    workbench: &Entity<Workbench>,
    view: &Entity<WorkbenchView>,
    palette: Palette,
    collapsed: bool,
    cx: &App,
) -> AnyElement {
    let (favorites_len, home_view) = {
        let state = workbench.read(cx);
        (
            state.settings().navigation.favorites.len(),
            state.home_view(),
        )
    };

    let mut rail = div()
        .w(px(if collapsed { 46.0 } else { 196.0 }))
        .h_full()
        .flex()
        .flex_col()
        .gap_1()
        .py_2()
        .px_1()
        .bg(rgb(palette.rail))
        .border_r_1()
        .border_color(rgb(palette.border));

    {
        let wb = workbench.clone();
        rail = rail.child(nav_item(
            palette,
            "rail-home",
            "首",
            "首页",
            collapsed,
            matches!(home_view, HomeView::Overview),
            move |cx| {
                wb.update(cx, |state, cx| state.show_home(cx));
            },
        ));
    }
    {
        let wb = workbench.clone();
        rail = rail.child(nav_item(
            palette,
            "rail-favorites",
            "★",
            &format!("收藏 ({favorites_len})"),
            collapsed,
            false,
            move |cx| {
                wb.update(cx, |state, cx| {
                    state.show_home(cx);
                    state.set_status("收藏工具显示在首页顶部", cx);
                });
            },
        ));
    }

    rail = rail.child(div().h(px(1.0)).mx_1().my_1().bg(rgb(palette.border)));

    for (index, category) in Category::ALL.into_iter().enumerate() {
        let wb = workbench.clone();
        let view = view.clone();
        let active = matches!(home_view, HomeView::Category(current) if current == category);
        rail = rail.child(nav_item(
            palette,
            &format!("rail-category-{index}"),
            category_glyph(category),
            category_label(category),
            collapsed,
            active,
            move |cx| {
                wb.update(cx, |state, cx| state.open_category(category, cx));
                view.update(cx, |this, cx| {
                    this.flyout = if collapsed { Some(category) } else { None };
                    cx.notify();
                });
            },
        ));
    }

    rail = rail.child(div().flex_1());

    {
        let wb = workbench.clone();
        rail = rail.child(nav_item(
            palette,
            "rail-settings",
            "⚙",
            "设置",
            collapsed,
            false,
            move |cx| {
                wb.update(cx, |state, cx| state.open_settings(cx));
            },
        ));
    }
    {
        let wb = workbench.clone();
        rail = rail.child(nav_item(
            palette,
            "rail-collapse",
            if collapsed { "»" } else { "«" },
            if collapsed {
                "展开侧栏"
            } else {
                "收起侧栏"
            },
            collapsed,
            false,
            move |cx| {
                wb.update(cx, |state, cx| state.toggle_sidebar(cx));
            },
        ));
    }

    rail.into_any_element()
}

fn nav_item(
    palette: Palette,
    id: &str,
    glyph: &'static str,
    label: &str,
    collapsed: bool,
    active: bool,
    handler: impl Fn(&mut App) + 'static,
) -> AnyElement {
    let mut item = div()
        .id(id.to_owned())
        .flex()
        .items_center()
        .gap_2()
        .h(px(30.0))
        .px_2()
        .rounded_md()
        .cursor_pointer()
        .text_xs()
        .bg(if active {
            rgb(palette.accent_soft)
        } else {
            rgb(palette.rail)
        })
        .text_color(if active {
            rgb(palette.accent)
        } else {
            rgb(palette.text)
        })
        .on_click(move |_, _, cx| handler(cx))
        .child(div().w(px(18.0)).text_center().text_sm().child(glyph));

    if !collapsed {
        item = item.child(div().truncate().child(label.to_owned()));
    }

    item.into_any_element()
}

fn render_tab_bar(
    workbench: &Entity<Workbench>,
    palette: Palette,
    mode: TabBarMode,
    cx: &App,
) -> AnyElement {
    let tabs: Vec<(usize, String, bool)> = workbench
        .read(cx)
        .tabs()
        .iter()
        .enumerate()
        .map(|(index, tab)| {
            let title = match tab.kind {
                TabKind::Home => "首页".to_owned(),
                TabKind::Tool {
                    tool_id, instance, ..
                } => format!("{} #{instance}", tool_name(tool_id)),
            };
            (index, title, !tab.is_home())
        })
        .collect();
    let active = workbench.read(cx).active_index();

    let mut tabs_row = div().flex().flex_wrap().gap_1().flex_1();
    for (index, title, closable) in tabs {
        let is_active = index == active;
        let wb = workbench.clone();
        let mut tab = div()
            .id(format!("tab-{index}"))
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .rounded_md()
            .cursor_pointer()
            .text_xs()
            .bg(if is_active {
                rgb(palette.accent_soft)
            } else {
                rgb(palette.panel)
            })
            .border_1()
            .border_color(if is_active {
                rgb(palette.accent)
            } else {
                rgb(palette.border)
            })
            .text_color(if is_active {
                rgb(palette.accent)
            } else {
                rgb(palette.text)
            })
            .child(title)
            .on_click(move |_, _, cx| {
                wb.update(cx, |state, cx| state.select_tab(index, cx));
            });

        if closable {
            let wb_close = workbench.clone();
            tab = tab.child(
                div()
                    .id(format!("tab-close-{index}"))
                    .px_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(rgb(palette.muted))
                    .child("×")
                    .on_click(move |_, _, cx| {
                        wb_close.update(cx, |state, cx| state.close_tab(index, cx));
                        cx.stop_propagation();
                    }),
            );
        }

        tabs_row = tabs_row.child(tab);
    }

    let wb = workbench.clone();
    let mode_label = match mode {
        TabBarMode::MultiRowGrid => "网格",
        TabBarMode::SingleRowScroll => "单行",
    };

    div()
        .flex()
        .items_center()
        .gap_2()
        .px_2()
        .py_1()
        .min_h(px(40.0))
        .bg(rgb(palette.bg))
        .border_b_1()
        .border_color(rgb(palette.border))
        .child(tabs_row)
        .child(
            div()
                .id("tab-mode-toggle")
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .border_1()
                .border_color(rgb(palette.border))
                .bg(rgb(palette.panel))
                .text_color(rgb(palette.muted))
                .child(mode_label)
                .on_click(move |_, _, cx| {
                    wb.update(cx, |state, cx| state.toggle_tab_bar_mode(cx));
                }),
        )
        .into_any_element()
}

fn render_content(
    workbench: &Entity<Workbench>,
    palette: Palette,
    cx: &mut Context<WorkbenchView>,
) -> AnyElement {
    let active_kind = {
        let state = workbench.read(cx);
        match &state.active_tab().kind {
            TabKind::Home => None,
            TabKind::Tool { view, .. } => Some(view.clone()),
        }
    };

    let Some(tool_view) = active_kind else {
        return render_home(workbench, palette, cx);
    };

    div()
        .flex_1()
        .overflow_hidden()
        .child(tool_view)
        .into_any_element()
}

fn render_home(workbench: &Entity<Workbench>, palette: Palette, cx: &App) -> AnyElement {
    let state = workbench.read(cx);
    let home_view = state.home_view();
    let favorites = state.settings().navigation.favorites.clone();
    let show_recents = state.settings().navigation.show_recents;
    let recents: Vec<ToolId> = state.recents().iter().collect();
    let notice = state.settings_notice().map(str::to_owned);

    let mut page = div()
        .flex_1()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .overflow_y_scrollbar();

    if let Some(notice) = notice {
        page = page.child(
            div()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(rgb(palette.danger))
                .text_xs()
                .text_color(rgb(palette.danger))
                .child(notice),
        );
    }

    let heading = match home_view {
        HomeView::Overview => "工作台首页".to_owned(),
        HomeView::Category(category) => category_label(category).to_owned(),
    };
    page = page.child(
        div()
            .flex()
            .items_end()
            .justify_between()
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(heading),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(palette.muted))
                            .child("28 个离线工具 · 点击工具打开独立标签实例 · 命令面板 Ctrl/Cmd+K 将在 M3 接入"),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(palette.muted))
                    .child(format!("收藏 {} · 最近 {}", favorites.len(), recents.len())),
            ),
    );

    if !favorites.is_empty() {
        page = page.child(chip_section(workbench, palette, "收藏", favorites));
    }

    if show_recents && !recents.is_empty() {
        page = page.child(chip_section(
            workbench,
            palette,
            "最近使用（仅内存）",
            recents,
        ));
    }

    if let HomeView::Category(category) = home_view {
        page = page.child(tool_grid(workbench, palette, category));
    } else {
        for category in Category::ALL {
            page = page.child(tool_grid(workbench, palette, category));
        }
    }

    page.into_any_element()
}

fn chip_section(
    workbench: &Entity<Workbench>,
    palette: Palette,
    title: &str,
    tools: Vec<ToolId>,
) -> AnyElement {
    let mut row = div().flex().flex_wrap().gap_2();
    for tool in tools {
        let wb = workbench.clone();
        row = row.child(
            div()
                .id(format!("chip-{title}-{}", tool.as_str()))
                .px_2()
                .py_1()
                .rounded_md()
                .cursor_pointer()
                .border_1()
                .border_color(rgb(palette.border))
                .bg(rgb(palette.panel))
                .text_xs()
                .child(tool_name(tool))
                .on_click(move |_, _, cx| {
                    wb.update(cx, |state, cx| state.open_tool(tool, cx));
                }),
        );
    }

    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(palette.accent))
                .child(title.to_owned()),
        )
        .child(row)
        .into_any_element()
}

fn tool_grid(workbench: &Entity<Workbench>, palette: Palette, category: Category) -> AnyElement {
    let mut list = div().flex().flex_col().gap_1();
    for tool in tools_in(category) {
        let wb = workbench.clone();
        list = list.child(
            div()
                .id(format!("tool-{}", tool.as_str()))
                .flex()
                .items_center()
                .justify_between()
                .px_3()
                .py_2()
                .rounded_md()
                .cursor_pointer()
                .border_1()
                .border_color(rgb(palette.border))
                .bg(rgb(palette.panel))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .child(div().text_xs().child(tool_name(tool)))
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(palette.muted))
                                .child(tool_hint(tool)),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(palette.accent))
                        .child("打开 ›"),
                )
                .on_click(move |_, _, cx| {
                    wb.update(cx, |state, cx| state.open_tool(tool, cx));
                }),
        );
    }

    div()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(rgb(palette.border))
        .bg(rgb(palette.panel_alt))
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(category_label(category)),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(palette.muted))
                        .child(format!("{} 个工具", tools_in(category).count())),
                ),
        )
        .child(list)
        .into_any_element()
}

fn render_flyout(
    workbench: &Entity<Workbench>,
    view: &Entity<WorkbenchView>,
    palette: Palette,
    category: Category,
) -> AnyElement {
    let mut list = div().flex().flex_col().gap_1();
    for tool in tools_in(category) {
        let wb = workbench.clone();
        let view = view.clone();
        list = list.child(
            div()
                .id(format!("flyout-{}", tool.as_str()))
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .text_xs()
                .hover(|style| style.bg(rgb(palette.panel_alt)))
                .child(tool_name(tool))
                .on_click(move |_, _, cx| {
                    wb.update(cx, |state, cx| state.open_tool(tool, cx));
                    view.update(cx, |this, cx| {
                        this.flyout = None;
                        cx.notify();
                    });
                }),
        );
    }

    div()
        .absolute()
        .left(px(46.0))
        .top(px(0.0))
        .bottom(px(26.0))
        .w(px(230.0))
        .flex()
        .flex_col()
        .gap_2()
        .p_2()
        .bg(rgb(palette.panel))
        .border_r_1()
        .border_color(rgb(palette.border))
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(palette.accent))
                .child(category_label(category)),
        )
        .child(list)
        .into_any_element()
}

/// Expose the phase label for tests/status usage elsewhere.
pub fn phase_label(phase: Phase) -> &'static str {
    phase.label()
}
