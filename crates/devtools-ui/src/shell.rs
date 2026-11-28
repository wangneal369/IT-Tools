//! A + B fused workbench shell (spec §3.2).
//!
//! Layout: collapsible category rail (with flyout when collapsed), tab bar in
//! multi-row-grid or single-row-scroll mode, the active tool/home canvas, the
//! 26 px status bar and the settings drawer. The shell is a thin projection of
//! the window-independent [`Workbench`] entity.

use devtools_core::{Category, TabBarMode, Theme, ToolId};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::icons::{IconName, category_icon, icon_el, tool_icon};
use crate::labels::{category_label, tool_hint, tool_name, tools_in};
use crate::settings;
use crate::theme::{self, Palette};
use crate::workspace::{HomeView, Phase, TabKind, Workbench, geometry_from_bounds};

const RAIL_EXPANDED_WIDTH: f32 = 200.0;
const RAIL_COLLAPSED_WIDTH: f32 = 48.0;

pub struct WorkbenchView {
    workbench: Entity<Workbench>,
    _bounds_subscription: Subscription,
    _activation_subscription: Subscription,
    _appearance_subscription: Subscription,
    _state_subscription: Subscription,
    _input_subscription: Subscription,
    palette_input: Entity<InputState>,
    palette_epoch: u64,
    previous_focus: Option<FocusHandle>,
}

impl WorkbenchView {
    pub fn new(workbench: Entity<Workbench>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let observed = workbench.clone();
        let bounds_subscription = cx.observe_window_bounds(window, move |_view, window, cx| {
            let geometry = geometry_from_bounds(window.window_bounds());
            let maximized = window.is_maximized();
            observed.update(cx, |state, _| state.note_geometry(geometry, maximized));
        });

        if window.is_window_active() {
            let initial = workbench.clone();
            cx.defer(move |cx| initial.update(cx, |state, cx| state.smart_paste(false, cx)));
        }
        let focus_state = workbench.clone();
        let activation = cx.observe_window_activation(window, move |_, window, cx| {
            if window.is_window_active() {
                focus_state.update(cx, |state, cx| state.smart_paste(false, cx));
            }
        });

        let appearance_state = workbench.clone();
        let appearance_subscription = cx.observe_window_appearance(window, move |_, window, cx| {
            let settings = appearance_state.read(cx).settings().clone();
            if settings.theme == Theme::System {
                theme::sync_gpui_theme(&settings, Some(window.appearance()), cx);
                cx.notify();
            }
        });

        let state_subscription = cx.observe(&workbench, |_, _, cx| cx.notify());
        let palette_input = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("搜索 28 个工具 — 支持中文 / English / 缩写 / 拼音")
        });
        let state_for_input = workbench.clone();
        let input_subscription = cx.subscribe(&palette_input, move |_, input, event, cx| {
            if matches!(event, InputEvent::Change) {
                let value = input.read(cx).value().to_string();
                state_for_input.update(cx, |state, cx| {
                    state.palette_query = value;
                    state.palette_selected = 0;
                    cx.notify();
                });
            }
        });
        Self {
            workbench,
            _bounds_subscription: bounds_subscription,
            _activation_subscription: activation,
            _appearance_subscription: appearance_subscription,
            _state_subscription: state_subscription,
            _input_subscription: input_subscription,
            palette_input,
            palette_epoch: 0,
            previous_focus: None,
        }
    }
}

impl Render for WorkbenchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let workbench = self.workbench.clone();
        let (open, epoch) = {
            let state = workbench.read(cx);
            (state.palette_open, state.palette_epoch)
        };
        if open && epoch != self.palette_epoch {
            self.previous_focus = window.focused(cx);
            self.palette_epoch = epoch;
            self.palette_input.update(cx, |input, cx| {
                input.set_value("", window, cx);
                input.focus(window, cx);
            });
        } else if !open && let Some(focus) = self.previous_focus.take() {
            window.focus(&focus, cx);
        }

        let (
            palette,
            collapsed_setting,
            tab_bar_mode,
            settings_open,
            flyout_category,
            phase,
            status_line,
            hotkey_label,
            tab_count,
        ) = {
            let state = workbench.read(cx);
            (
                theme::resolve(state.settings().theme, window),
                state.settings().layout.sidebar_collapsed,
                state.settings().layout.tab_bar_mode,
                state.is_settings_open(),
                state.flyout_category(),
                state.phase(),
                state.status_line().to_owned(),
                state.platform().hotkey_status().label(),
                state.tabs().len(),
            )
        };

        let viewport_width = window.viewport_size().width.as_f32();
        let collapsed = collapsed_setting || viewport_width <= 820.0;
        let rail_width = if collapsed {
            RAIL_COLLAPSED_WIDTH
        } else {
            RAIL_EXPANDED_WIDTH
        };
        let content_width = (viewport_width
            - rail_width
            - if settings_open {
                settings::DRAWER_WIDTH
            } else {
                0.0
            })
        .max(320.0);

        let rail = render_rail(&workbench, palette, collapsed, cx);
        let tab_bar = render_tab_bar(&workbench, palette, tab_bar_mode, cx);
        let content = render_content(&workbench, palette, content_width, cx);

        let mut main = div()
            .flex_1()
            .min_h_0()
            .flex()
            .overflow_hidden()
            .child(rail)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .flex()
                    .flex_col()
                    .overflow_hidden()
                    .child(tab_bar)
                    .children(render_detection_banner(&workbench, palette, cx))
                    .child(content),
            );

        if settings_open {
            main = main.child(settings::panel(&workbench, palette, cx));
        }

        let status = div()
            .h(px(26.0))
            .flex_shrink_0()
            .px_3()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .bg(rgb(palette.chrome))
            .border_t_1()
            .border_color(rgb(palette.border))
            .text_xs()
            .text_color(rgb(palette.muted))
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(6.0))
                            .h(px(6.0))
                            .rounded_full()
                            .bg(rgb(palette.success)),
                    )
                    .child(format!("{} · {} 个标签", phase.label(), tab_count))
                    .child(div().text_color(rgb(palette.subtle)).child("· 100% 离线")),
            )
            .child(div().flex_1().min_w_0().truncate().child(status_line))
            .child(
                div()
                    .flex_shrink_0()
                    .text_color(rgb(palette.subtle))
                    .child(format!("热键：{hotkey_label}")),
            );

        let flyout = if collapsed {
            flyout_category.map(|category| render_flyout(&workbench, palette, category, cx))
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
            .text_size(px(13.0))
            .child(main)
            .child(status)
            .children(flyout)
            .children(render_relay(&workbench, palette, cx))
            .children(if open {
                Some(render_palette(&workbench, &self.palette_input, palette, cx))
            } else {
                None
            })
    }
}

fn render_rail(
    workbench: &Entity<Workbench>,
    palette: Palette,
    collapsed: bool,
    cx: &App,
) -> AnyElement {
    let (favorites_len, home_view, settings_open, active_tool_category) = {
        let state = workbench.read(cx);
        let active_cat = match &state.active_tab().kind {
            TabKind::Home => None,
            TabKind::Tool { tool_id, .. } => Some(tool_id.category()),
        };
        (
            state.settings().navigation.favorites.len(),
            state.home_view(),
            state.is_settings_open(),
            active_cat,
        )
    };
    let on_home_tab = active_tool_category.is_none();

    let mut rail = div()
        .w(px(if collapsed {
            RAIL_COLLAPSED_WIDTH
        } else {
            RAIL_EXPANDED_WIDTH
        }))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .gap_0p5()
        .py_2()
        .px_2()
        .bg(rgb(palette.rail))
        .border_r_1()
        .border_color(rgb(palette.border));

    // Brand mark
    {
        let wb = workbench.clone();
        let mut brand = div()
            .id("rail-brand")
            .h(px(34.0))
            .mb_1()
            .px_1p5()
            .rounded_md()
            .flex()
            .items_center()
            .when(collapsed, |el| el.justify_center())
            .gap_2()
            .cursor_pointer()
            .on_click(move |_, _, cx| {
                wb.update(cx, |state, cx| {
                    state.show_home(cx);
                    state.close_flyout(cx);
                });
            })
            .child(
                img(crate::icons::brand_icon_image())
                    .w(px(24.0))
                    .h(px(24.0))
                    .flex_shrink_0()
                    .rounded_md(),
            );

        if !collapsed {
            brand = brand
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_sm()
                        .truncate()
                        .child("DevTools"),
                )
                .child(
                    div()
                        .px_1p5()
                        .py_0p5()
                        .rounded_sm()
                        .bg(rgb(palette.panel))
                        .border_1()
                        .border_color(rgb(palette.border))
                        .text_size(px(10.0))
                        .text_color(rgb(palette.muted))
                        .child("V1"),
                );
        }
        rail = rail.child(brand);
    }

    {
        let wb = workbench.clone();
        rail = rail.child(nav_item(
            palette,
            "rail-home",
            IconName::House,
            "工作台首页",
            None,
            false,
            collapsed,
            on_home_tab && matches!(home_view, HomeView::Overview),
            move |cx| {
                wb.update(cx, |state, cx| {
                    state.show_home(cx);
                    state.close_flyout(cx);
                });
            },
        ));
    }
    {
        let wb = workbench.clone();
        rail = rail.child(nav_item(
            palette,
            "rail-favorites",
            IconName::Star,
            "收藏工具",
            Some(favorites_len.to_string()),
            false,
            collapsed,
            false,
            move |cx| {
                wb.update(cx, |state, cx| {
                    state.show_home(cx);
                    state.close_flyout(cx);
                    state.set_status("收藏工具展示在工作台首页顶部", cx);
                });
            },
        ));
    }

    rail = rail.child(div().h(px(1.0)).mx_1().my_1p5().bg(rgb(palette.border)));

    if !collapsed {
        rail = rail.child(
            div()
                .px_2()
                .pb_1()
                .text_size(px(11.0))
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(palette.subtle))
                .child("工具分类"),
        );
    }

    for (index, category) in Category::ALL.into_iter().enumerate() {
        let wb = workbench.clone();
        let recommended =
            tools_in(category).any(|tool| workbench.read(cx).recommendation_for(tool));
        let active = (on_home_tab
            && matches!(home_view, HomeView::Category(current) if current == category))
            || active_tool_category == Some(category)
            || (collapsed && workbench.read(cx).flyout_category() == Some(category));
        let count = tools_in(category).count();
        rail = rail.child(nav_item(
            palette,
            &format!("rail-category-{index}"),
            category_icon(category),
            category_label(category),
            Some(count.to_string()),
            recommended,
            collapsed,
            active,
            move |cx| {
                wb.update(cx, |state, cx| {
                    if collapsed {
                        let next = if state.flyout_category() == Some(category) {
                            None
                        } else {
                            Some(category)
                        };
                        state.set_flyout_category(next, cx);
                    } else {
                        state.open_category(category, cx);
                        state.close_flyout(cx);
                    }
                });
            },
        ));
    }

    rail = rail.child(div().flex_1());
    rail = rail.child(div().h(px(1.0)).mx_1().my_1().bg(rgb(palette.border)));

    {
        let wb = workbench.clone();
        rail = rail.child(nav_item(
            palette,
            "rail-settings",
            IconName::Settings,
            "偏好设置",
            None,
            false,
            collapsed,
            settings_open,
            move |cx| {
                wb.update(cx, |state, cx| {
                    if state.is_settings_open() {
                        state.close_settings(cx);
                    } else {
                        state.open_settings(cx);
                    }
                });
            },
        ));
    }
    {
        let wb = workbench.clone();
        rail = rail.child(nav_item(
            palette,
            "rail-collapse",
            if collapsed {
                IconName::PanelLeftOpen
            } else {
                IconName::PanelLeftClose
            },
            if collapsed {
                "展开侧栏"
            } else {
                "收起侧栏"
            },
            None,
            false,
            collapsed,
            false,
            move |cx| {
                wb.update(cx, |state, cx| {
                    state.close_flyout(cx);
                    state.toggle_sidebar(cx);
                });
            },
        ));
    }

    rail.into_any_element()
}

#[allow(clippy::too_many_arguments)]
fn nav_item(
    palette: Palette,
    id: &str,
    icon: IconName,
    label: &str,
    badge: Option<String>,
    recommended: bool,
    collapsed: bool,
    active: bool,
    handler: impl Fn(&mut App) + 'static,
) -> AnyElement {
    let icon_color = if active || recommended {
        palette.accent
    } else {
        palette.muted
    };
    let text_color = if active { palette.accent } else { palette.text };

    let mut item = div()
        .id(id.to_owned())
        .relative()
        .flex()
        .items_center()
        .when(collapsed, |el| el.justify_center())
        .gap_2()
        .h(px(30.0))
        .px_2()
        .rounded_md()
        .cursor_pointer()
        .text_xs()
        .bg(rgb(if active {
            palette.accent_soft
        } else {
            palette.rail
        }))
        .text_color(rgb(text_color))
        .hover(move |style| {
            style.bg(rgb(if active {
                palette.accent_soft
            } else {
                palette.hover
            }))
        })
        .on_click(move |_, _, cx| handler(cx))
        .child(icon_el(icon, 14.5, icon_color));

    if !collapsed {
        item = item.child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .when(active, |el| el.font_weight(FontWeight::MEDIUM))
                .child(label.to_owned()),
        );
        if recommended {
            item = item.child(
                div()
                    .w(px(6.0))
                    .h(px(6.0))
                    .rounded_full()
                    .bg(rgb(palette.accent)),
            );
        }
        if let Some(badge) = badge {
            item = item.child(
                div()
                    .px_1p5()
                    .rounded_sm()
                    .text_size(px(10.5))
                    .text_color(rgb(if active {
                        palette.accent
                    } else {
                        palette.subtle
                    }))
                    .child(badge),
            );
        }
    } else if recommended {
        item = item.child(
            div()
                .absolute()
                .top(px(5.0))
                .right(px(5.0))
                .w(px(5.0))
                .h(px(5.0))
                .rounded_full()
                .bg(rgb(palette.accent)),
        );
    }

    item.into_any_element()
}

fn render_tab_bar(
    workbench: &Entity<Workbench>,
    palette: Palette,
    mode: TabBarMode,
    cx: &App,
) -> AnyElement {
    let tabs: Vec<(usize, IconName, String, Option<u32>, bool)> = workbench
        .read(cx)
        .tabs()
        .iter()
        .enumerate()
        .map(|(index, tab)| match tab.kind {
            TabKind::Home => (index, IconName::House, "首页".to_owned(), None, false),
            TabKind::Tool {
                tool_id, instance, ..
            } => (
                index,
                tool_icon(tool_id),
                tool_name(tool_id).to_owned(),
                Some(instance),
                true,
            ),
        })
        .collect();
    let active = workbench.read(cx).active_index();

    let mut tabs_row = match mode {
        TabBarMode::MultiRowGrid => div()
            .flex()
            .flex_wrap()
            .gap_1()
            .flex_1()
            .min_w_0()
            .max_h(px(98.0)),
        TabBarMode::SingleRowScroll => div()
            .flex()
            .flex_nowrap()
            .gap_1()
            .flex_1()
            .min_w_0()
            .overflow_hidden(),
    };

    for (index, icon, title, instance, closable) in tabs {
        let is_active = index == active;
        let wb = workbench.clone();
        let mut tab = div()
            .id(format!("tab-{index}"))
            .h(px(28.0))
            .flex()
            .items_center()
            .gap_1p5()
            .px_2p5()
            .when(mode == TabBarMode::MultiRowGrid && closable, |el| {
                el.min_w(px(124.0)).max_w(px(210.0))
            })
            .when(mode == TabBarMode::SingleRowScroll, |el| {
                el.flex_shrink_0().max_w(px(200.0))
            })
            .rounded_md()
            .cursor_pointer()
            .text_xs()
            .bg(rgb(if is_active {
                palette.panel
            } else {
                palette.chrome
            }))
            .border_1()
            .border_color(rgb(if is_active {
                palette.border_strong
            } else {
                palette.chrome
            }))
            .text_color(rgb(if is_active {
                palette.text
            } else {
                palette.muted
            }))
            .hover(move |style| {
                style
                    .bg(rgb(if is_active {
                        palette.panel
                    } else {
                        palette.hover
                    }))
                    .text_color(rgb(palette.text))
            })
            .child(icon_el(
                icon,
                13.0,
                if is_active {
                    palette.accent
                } else {
                    palette.muted
                },
            ))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .when(is_active, |el| el.font_weight(FontWeight::MEDIUM))
                    .child(title),
            )
            .on_click(move |_, _, cx| {
                wb.update(cx, |state, cx| state.select_tab(index, cx));
            });

        if let Some(instance) = instance {
            tab = tab.child(
                div()
                    .flex_shrink_0()
                    .text_size(px(10.5))
                    .text_color(rgb(if is_active {
                        palette.accent
                    } else {
                        palette.subtle
                    }))
                    .child(format!("#{instance}")),
            );
        }

        if closable {
            let wb_close = workbench.clone();
            tab = tab.child(
                div()
                    .id(format!("tab-close-{index}"))
                    .ml_auto()
                    .w(px(16.0))
                    .h(px(16.0))
                    .flex_shrink_0()
                    .rounded_sm()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(palette.active)))
                    .child(icon_el(IconName::X, 11.0, palette.subtle))
                    .on_click(move |_, _, cx| {
                        wb_close.update(cx, |state, cx| state.close_tab(index, cx));
                        cx.stop_propagation();
                    }),
            );
        }

        tabs_row = tabs_row.child(tab);
    }

    let wb = workbench.clone();
    let (mode_icon, mode_label) = match mode {
        TabBarMode::MultiRowGrid => (IconName::LayoutGrid, "网格"),
        TabBarMode::SingleRowScroll => (IconName::Rows2, "单行"),
    };
    let shortcut_badge = if cfg!(target_os = "macos") {
        "⌘K"
    } else {
        "Ctrl+K"
    };

    let tabs_container: AnyElement = match mode {
        TabBarMode::MultiRowGrid => tabs_row.overflow_y_scrollbar().into_any_element(),
        TabBarMode::SingleRowScroll => tabs_row.overflow_x_scrollbar().into_any_element(),
    };

    div()
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_2()
        .px_2p5()
        .py_1p5()
        .min_h(px(40.0))
        .bg(rgb(palette.chrome))
        .border_b_1()
        .border_color(rgb(palette.border))
        .child(tabs_container)
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_1p5()
                .child(
                    div()
                        .id("command-palette-open")
                        .h(px(28.0))
                        .px_2p5()
                        .rounded_md()
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap_2()
                        .bg(rgb(palette.panel))
                        .border_1()
                        .border_color(rgb(palette.border))
                        .hover(move |style| {
                            style
                                .border_color(rgb(palette.border_strong))
                                .bg(rgb(palette.hover))
                        })
                        .text_xs()
                        .text_color(rgb(palette.muted))
                        .child(icon_el(IconName::Search, 12.5, palette.muted))
                        .child("搜索工具")
                        .child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(rgb(palette.panel_alt))
                                .border_1()
                                .border_color(rgb(palette.border))
                                .text_size(px(10.0))
                                .text_color(rgb(palette.subtle))
                                .child(shortcut_badge),
                        )
                        .on_click({
                            let wb = workbench.clone();
                            move |_, _, cx| {
                                wb.update(cx, |s, cx| s.open_palette(cx));
                            }
                        }),
                )
                .child(
                    div()
                        .id("tab-mode-toggle")
                        .h(px(28.0))
                        .px_2()
                        .rounded_md()
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap_1()
                        .text_xs()
                        .border_1()
                        .border_color(rgb(palette.border))
                        .bg(rgb(palette.panel))
                        .text_color(rgb(palette.muted))
                        .hover(move |style| {
                            style.bg(rgb(palette.hover)).text_color(rgb(palette.text))
                        })
                        .child(icon_el(mode_icon, 12.5, palette.muted))
                        .child(mode_label)
                        .on_click(move |_, _, cx| {
                            wb.update(cx, |state, cx| state.toggle_tab_bar_mode(cx));
                        }),
                ),
        )
        .into_any_element()
}

fn render_content(
    workbench: &Entity<Workbench>,
    palette: Palette,
    content_width: f32,
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
        return render_home(workbench, palette, content_width, cx);
    };

    div()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .overflow_hidden()
        .child(tool_view)
        .into_any_element()
}

fn render_home(
    workbench: &Entity<Workbench>,
    palette: Palette,
    content_width: f32,
    cx: &App,
) -> AnyElement {
    let state = workbench.read(cx);
    let home_view = state.home_view();
    let favorites = state.settings().navigation.favorites.clone();
    let show_recents = state.settings().navigation.show_recents;
    let recents: Vec<ToolId> = state.recents().iter().collect();
    let notice = state.settings_notice().map(str::to_owned);

    let cols = if content_width >= 880.0 {
        3
    } else if content_width >= 540.0 {
        2
    } else {
        1
    };

    let mut page = div()
        .flex_1()
        .min_h_0()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_4()
        .px_5()
        .py_4()
        .overflow_y_scrollbar();

    if let Some(notice) = notice {
        page = page.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .p_2p5()
                .rounded_md()
                .bg(rgb(palette.danger_soft))
                .border_1()
                .border_color(rgb(palette.danger))
                .text_xs()
                .text_color(rgb(palette.danger))
                .child(icon_el(IconName::CircleAlert, 14.0, palette.danger))
                .child(div().flex_1().min_w_0().child(notice)),
        );
    }

    let title_row: AnyElement = match home_view {
        HomeView::Overview => div()
            .text_base()
            .font_weight(FontWeight::SEMIBOLD)
            .truncate()
            .child("工作台首页")
            .into_any_element(),
        HomeView::Category(category) => {
            let wb = workbench.clone();
            div()
                .flex()
                .items_center()
                .gap_1p5()
                .min_w_0()
                .child(
                    div()
                        .id("home-breadcrumb-root")
                        .text_sm()
                        .text_color(rgb(palette.muted))
                        .cursor_pointer()
                        .hover(move |s| s.text_color(rgb(palette.accent)))
                        .child("工作台首页")
                        .on_click(move |_, _, cx| {
                            wb.update(cx, |s, cx| s.show_home(cx));
                        }),
                )
                .child(icon_el(IconName::ChevronRight, 13.0, palette.subtle))
                .child(icon_el(category_icon(category), 15.0, palette.accent))
                .child(
                    div()
                        .text_base()
                        .font_weight(FontWeight::SEMIBOLD)
                        .truncate()
                        .child(category_label(category)),
                )
                .into_any_element()
        }
    };

    let wb_paste = workbench.clone();
    page = page.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap_0p5()
                    .child(title_row)
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(palette.muted))
                            .truncate()
                            .child("28 个纯离线开发工具 · 支持获焦智能识别与单跳接力 · 零数据落盘"),
                    ),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .px_2p5()
                            .py_1()
                            .rounded_full()
                            .bg(rgb(palette.panel))
                            .border_1()
                            .border_color(rgb(palette.border))
                            .text_xs()
                            .text_color(rgb(palette.muted))
                            .child(format!("收藏 {} · 最近 {}", favorites.len(), recents.len())),
                    )
                    .child(
                        div()
                            .id("home-smart-paste")
                            .h(px(28.0))
                            .px_2p5()
                            .rounded_md()
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .bg(rgb(palette.panel))
                            .border_1()
                            .border_color(rgb(palette.border))
                            .hover(move |s| {
                                s.bg(rgb(palette.hover))
                                    .border_color(rgb(palette.border_strong))
                            })
                            .text_xs()
                            .text_color(rgb(palette.text))
                            .child(icon_el(IconName::ClipboardPaste, 13.0, palette.accent))
                            .child("智能粘贴")
                            .on_click(move |_, _, cx| {
                                wb_paste.update(cx, |s, cx| s.smart_paste(true, cx));
                            }),
                    ),
            ),
    );

    if !favorites.is_empty() {
        page = page.child(chip_section(
            workbench,
            palette,
            IconName::StarFill,
            "收藏工具",
            favorites,
        ));
    }

    if show_recents && !recents.is_empty() {
        page = page.child(chip_section(
            workbench,
            palette,
            IconName::Clock,
            "最近使用（仅驻内存）",
            recents,
        ));
    }

    if let HomeView::Category(category) = home_view {
        page = page.child(tool_grid(workbench, palette, category, cols, cx));
    } else {
        for category in Category::ALL {
            page = page.child(tool_grid(workbench, palette, category, cols, cx));
        }
    }

    page.into_any_element()
}

fn chip_section(
    workbench: &Entity<Workbench>,
    palette: Palette,
    icon: IconName,
    title: &str,
    tools: Vec<ToolId>,
) -> AnyElement {
    let mut row = div().flex().flex_wrap().gap_1p5();
    for tool in tools {
        let wb = workbench.clone();
        row = row.child(
            div()
                .id(format!("chip-{title}-{}", tool.as_str()))
                .h(px(28.0))
                .px_2p5()
                .rounded_md()
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_1p5()
                .border_1()
                .border_color(rgb(palette.border))
                .bg(rgb(palette.panel))
                .hover(move |s| {
                    s.bg(rgb(palette.hover))
                        .border_color(rgb(palette.border_strong))
                })
                .text_xs()
                .child(icon_el(tool_icon(tool), 13.0, palette.accent))
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
                .flex()
                .items_center()
                .gap_1p5()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(palette.muted))
                .child(icon_el(icon, 12.5, palette.accent))
                .child(title.to_owned()),
        )
        .child(row)
        .into_any_element()
}

fn tool_grid(
    workbench: &Entity<Workbench>,
    palette: Palette,
    category: Category,
    cols: usize,
    cx: &App,
) -> AnyElement {
    let tools: Vec<ToolId> = tools_in(category).collect();
    let mut grid = div().flex().flex_col().gap_2p5();

    for chunk in tools.chunks(cols.max(1)) {
        let mut row = div().flex().gap_2p5().w_full();
        for &tool in chunk {
            row = row.child(tool_card(workbench, palette, tool, cx));
        }
        for _ in chunk.len()..cols.max(1) {
            row = row.child(div().flex_1().min_w_0());
        }
        grid = grid.child(row);
    }

    div()
        .flex()
        .flex_col()
        .gap_2p5()
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_0p5()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(icon_el(category_icon(category), 14.0, palette.accent))
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(palette.text))
                                .child(category_label(category)),
                        ),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(palette.subtle))
                        .child(format!("{} 个工具", tools.len())),
                ),
        )
        .child(grid)
        .into_any_element()
}

fn tool_card(
    workbench: &Entity<Workbench>,
    palette: Palette,
    tool: ToolId,
    cx: &App,
) -> AnyElement {
    let (matched, favorite) = {
        let state = workbench.read(cx);
        (state.recommendation_for(tool), state.is_favorite(tool))
    };
    let wb = workbench.clone();
    let wb_fav = workbench.clone();

    div()
        .id(format!("tool-{}", tool.as_str()))
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_2()
        .p_3()
        .rounded_lg()
        .cursor_pointer()
        .border_1()
        .border_color(rgb(if matched {
            palette.accent
        } else {
            palette.border
        }))
        .bg(rgb(palette.panel))
        .hover(move |style| {
            style.bg(rgb(palette.hover)).border_color(rgb(if matched {
                palette.accent
            } else {
                palette.border_strong
            }))
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap_2p5()
                .min_w_0()
                .child(
                    div()
                        .w(px(30.0))
                        .h(px(30.0))
                        .flex_shrink_0()
                        .rounded_md()
                        .bg(rgb(if matched {
                            palette.accent_soft
                        } else {
                            palette.panel_alt
                        }))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon_el(tool_icon(tool), 15.0, palette.accent)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(if matched {
                            palette.accent
                        } else {
                            palette.text
                        }))
                        .truncate()
                        .child(tool_name(tool)),
                )
                .when(matched, |el| {
                    el.child(
                        div()
                            .flex_shrink_0()
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .bg(rgb(palette.accent_soft))
                            .text_size(px(10.0))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(palette.accent))
                            .child("识别推荐"),
                    )
                })
                .child(
                    div()
                        .id(format!("card-fav-{}", tool.as_str()))
                        .w(px(22.0))
                        .h(px(22.0))
                        .flex_shrink_0()
                        .rounded_sm()
                        .flex()
                        .items_center()
                        .justify_center()
                        .hover(move |s| s.bg(rgb(palette.active)))
                        .child(icon_el(
                            if favorite {
                                IconName::StarFill
                            } else {
                                IconName::Star
                            },
                            12.5,
                            if favorite {
                                palette.warning
                            } else {
                                palette.subtle
                            },
                        ))
                        .on_click(move |_, _, cx| {
                            wb_fav.update(cx, |s, cx| s.toggle_favorite(tool, cx));
                            cx.stop_propagation();
                        }),
                ),
        )
        .child(
            div()
                .text_size(px(11.5))
                .text_color(rgb(palette.muted))
                .truncate()
                .child(tool_hint(tool)),
        )
        .on_click(move |_, _, cx| {
            wb.update(cx, |state, cx| {
                if matched {
                    state.launch_recommendation(tool, cx);
                } else {
                    state.open_tool(tool, cx);
                }
            });
        })
        .into_any_element()
}

fn render_flyout(
    workbench: &Entity<Workbench>,
    palette: Palette,
    category: Category,
    cx: &App,
) -> AnyElement {
    let mut list = div().flex().flex_col().gap_1().overflow_y_scrollbar();
    for tool in tools_in(category) {
        let matched = workbench.read(cx).recommendation_for(tool);
        let wb = workbench.clone();
        list = list.child(
            div()
                .id(format!("flyout-{}", tool.as_str()))
                .flex()
                .items_center()
                .gap_2()
                .px_2p5()
                .py_1p5()
                .rounded_md()
                .cursor_pointer()
                .text_xs()
                .text_color(rgb(if matched {
                    palette.accent
                } else {
                    palette.text
                }))
                .hover(move |style| style.bg(rgb(palette.hover)))
                .child(icon_el(tool_icon(tool), 13.5, palette.accent))
                .child(div().flex_1().min_w_0().truncate().child(tool_name(tool)))
                .on_click(move |_, _, cx| {
                    wb.update(cx, |state, cx| {
                        state.open_tool(tool, cx);
                        state.close_flyout(cx);
                    });
                }),
        );
    }

    let wb_backdrop = workbench.clone();
    let wb_close = workbench.clone();

    div()
        .id("flyout-backdrop")
        .absolute()
        .left(px(RAIL_COLLAPSED_WIDTH))
        .top(px(0.0))
        .right(px(0.0))
        .bottom(px(26.0))
        .on_click(move |_, _, cx| {
            wb_backdrop.update(cx, |s, cx| s.close_flyout(cx));
        })
        .child(
            div()
                .id("flyout-drawer")
                .w(px(244.0))
                .h_full()
                .flex()
                .flex_col()
                .gap_2()
                .p_2p5()
                .bg(rgb(palette.panel))
                .border_r_1()
                .border_color(rgb(palette.border_strong))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_1()
                        .pb_1()
                        .border_b_1()
                        .border_color(rgb(palette.border))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1p5()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(palette.text))
                                .child(icon_el(category_icon(category), 13.5, palette.accent))
                                .child(category_label(category)),
                        )
                        .child(
                            div()
                                .id("flyout-close")
                                .w(px(20.0))
                                .h(px(20.0))
                                .rounded_sm()
                                .flex()
                                .items_center()
                                .justify_center()
                                .cursor_pointer()
                                .hover(move |s| s.bg(rgb(palette.hover)))
                                .child(icon_el(IconName::X, 12.0, palette.muted))
                                .on_click(move |_, _, cx| {
                                    wb_close.update(cx, |s, cx| s.close_flyout(cx));
                                }),
                        ),
                )
                .child(list),
        )
        .into_any_element()
}

/// Expose the phase label for tests/status usage elsewhere.
pub fn phase_label(phase: Phase) -> &'static str {
    phase.label()
}

fn render_detection_banner(
    workbench: &Entity<Workbench>,
    palette: Palette,
    cx: &App,
) -> Option<AnyElement> {
    let state = workbench.read(cx);
    let recommendations: Vec<_> = state.smart.result.banner().cloned().collect();
    if recommendations.is_empty() {
        return None;
    }
    let mut banner = div()
        .h(px(36.0))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap_2()
        .px_3p5()
        .bg(rgb(palette.accent_soft))
        .border_b_1()
        .border_color(rgb(palette.border))
        .text_xs()
        .child(
            div()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_1p5()
                .font_weight(FontWeight::MEDIUM)
                .text_color(rgb(palette.accent))
                .child(icon_el(IconName::Zap, 13.0, palette.accent))
                .child("剪贴板识别"),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(rgb(palette.muted))
                .child(state.smart.result.preview.clone()),
        );
    for recommendation in recommendations {
        let tool = recommendation.tool_id;
        let wb = workbench.clone();
        banner = banner.child(
            div()
                .id(format!("detect-{}", tool.as_str()))
                .h(px(24.0))
                .px_2p5()
                .rounded_md()
                .cursor_pointer()
                .flex_shrink_0()
                .flex()
                .items_center()
                .gap_1p5()
                .bg(rgb(palette.panel))
                .border_1()
                .border_color(rgb(palette.border_strong))
                .hover(move |s| s.bg(rgb(palette.hover)))
                .text_color(rgb(palette.accent))
                .font_weight(FontWeight::MEDIUM)
                .child(icon_el(tool_icon(tool), 12.0, palette.accent))
                .child(tool_name(tool))
                .on_click(move |_, _, cx| {
                    wb.update(cx, |s, cx| s.launch_recommendation(tool, cx));
                }),
        );
    }
    let wb = workbench.clone();
    Some(
        banner
            .child(
                div()
                    .id("detect-dismiss")
                    .w(px(22.0))
                    .h(px(22.0))
                    .flex_shrink_0()
                    .rounded_sm()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(move |s| s.bg(rgb(palette.hover)))
                    .child(icon_el(IconName::X, 12.5, palette.muted))
                    .on_click(move |_, _, cx| {
                        wb.update(cx, |s, cx| s.clear_detection(cx));
                    }),
            )
            .into_any_element(),
    )
}

fn render_palette(
    workbench: &Entity<Workbench>,
    input: &Entity<InputState>,
    palette: Palette,
    cx: &App,
) -> AnyElement {
    let state = workbench.read(cx);
    let results = state.palette_results();
    let result_count = results.len();
    let mut rows = div()
        .flex()
        .flex_col()
        .gap_0p5()
        .p_2()
        .max_h(px(390.0))
        .overflow_y_scrollbar();
    let mut previous = None;
    for (index, result) in results.iter().enumerate() {
        if previous != Some(result.group) {
            previous = Some(result.group);
            let (group_icon, group_title) = match result.group {
                devtools_search::SearchGroup::Detection => (IconName::Zap, "剪贴板智能推荐"),
                devtools_search::SearchGroup::Favorite => (IconName::Star, "收藏工具"),
                devtools_search::SearchGroup::Catalog => (IconName::Layers, "全部工具"),
            };
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1p5()
                    .px_2p5()
                    .pt_1p5()
                    .pb_1()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(palette.subtle))
                    .child(icon_el(group_icon, 11.5, palette.accent))
                    .child(group_title),
            );
        }
        let tool = result.descriptor.id;
        let selected = index == state.palette_selected;
        let wb = workbench.clone();
        rows = rows.child(
            div()
                .id(format!("palette-{index}"))
                .flex()
                .items_center()
                .gap_2p5()
                .px_2p5()
                .py_2()
                .rounded_md()
                .cursor_pointer()
                .bg(rgb(if selected {
                    palette.accent_soft
                } else {
                    palette.panel
                }))
                .border_1()
                .border_color(rgb(if selected {
                    palette.accent
                } else {
                    palette.panel
                }))
                .hover(move |s| {
                    s.bg(rgb(if selected {
                        palette.accent_soft
                    } else {
                        palette.hover
                    }))
                })
                .child(
                    div()
                        .w(px(28.0))
                        .h(px(28.0))
                        .flex_shrink_0()
                        .rounded_md()
                        .bg(rgb(palette.panel_alt))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon_el(tool_icon(tool), 14.0, palette.accent)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap_0p5()
                        .child(
                            div()
                                .text_size(px(13.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(if selected {
                                    palette.accent
                                } else {
                                    palette.text
                                }))
                                .truncate()
                                .child(result.descriptor.name),
                        )
                        .child(
                            div()
                                .text_size(px(11.0))
                                .text_color(rgb(palette.muted))
                                .truncate()
                                .child(tool_hint(tool)),
                        ),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .px_2()
                        .py_0p5()
                        .rounded_sm()
                        .bg(rgb(palette.panel_alt))
                        .border_1()
                        .border_color(rgb(palette.border))
                        .text_size(px(10.5))
                        .text_color(rgb(palette.muted))
                        .child(category_label(result.descriptor.category)),
                )
                .on_click(move |_, _, cx| {
                    wb.update(cx, |s, cx| s.activate_palette(index, cx));
                }),
        );
    }
    if results.is_empty() {
        rows = rows.child(
            div()
                .py_8()
                .flex()
                .flex_col()
                .items_center()
                .gap_1p5()
                .text_color(rgb(palette.muted))
                .child(icon_el(IconName::Search, 20.0, palette.subtle))
                .child(
                    div()
                        .text_xs()
                        .child("没有匹配的工具，可尝试中文、英文缩写或全拼"),
                ),
        );
    }

    let wb_backdrop = workbench.clone();
    let wb_close = workbench.clone();
    div()
        .id("palette-backdrop")
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .items_center()
        .pt(px(56.0))
        .px_4()
        .bg(rgba(if palette.is_dark {
            0x00000088
        } else {
            0x0f172a44
        }))
        .on_click(move |_, _, cx| {
            wb_backdrop.update(cx, |s, cx| s.close_palette(cx));
        })
        .child(
            div()
                .id("palette-card")
                .w(px(580.0))
                .max_w_full()
                .rounded_xl()
                .overflow_hidden()
                .bg(rgb(palette.panel))
                .border_1()
                .border_color(rgb(palette.border_strong))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(|_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_2()
                        .px_3p5()
                        .py_2()
                        .border_b_1()
                        .border_color(rgb(palette.border))
                        .child(icon_el(IconName::Search, 15.0, palette.accent))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(Input::new(input).appearance(false).bordered(false)),
                        )
                        .child(
                            div()
                                .id("palette-close")
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .cursor_pointer()
                                .bg(rgb(palette.panel_alt))
                                .border_1()
                                .border_color(rgb(palette.border))
                                .hover(move |s| s.bg(rgb(palette.hover)))
                                .text_size(px(11.0))
                                .text_color(rgb(palette.muted))
                                .child("Esc")
                                .on_click(move |_, _, cx| {
                                    wb_close.update(cx, |s, cx| s.close_palette(cx));
                                }),
                        ),
                )
                .child(div().max_h(px(390.0)).overflow_hidden().child(rows))
                .child(
                    div()
                        .px_3p5()
                        .py_2()
                        .bg(rgb(palette.chrome))
                        .border_t_1()
                        .border_color(rgb(palette.border))
                        .flex()
                        .items_center()
                        .justify_between()
                        .text_size(px(11.0))
                        .text_color(rgb(palette.muted))
                        .child("↑ ↓ 选择 · Enter 打开工具 · Esc 关闭")
                        .child(format!("{result_count} 项结果")),
                ),
        )
        .into_any_element()
}

fn render_relay(workbench: &Entity<Workbench>, palette: Palette, cx: &App) -> Option<AnyElement> {
    let state = workbench.read(cx);
    let relay = state.relay.as_ref()?;
    let targets: Vec<_> = relay
        .result
        .recommendations
        .iter()
        .filter(|r| r.confidence.is_banner())
        .take(8)
        .map(|r| r.tool_id)
        .collect();

    let wb_close = workbench.clone();
    let mut menu = div()
        .id("relay-card")
        .absolute()
        .right(px(18.0))
        .top(px(86.0))
        .w(px(296.0))
        .p_2()
        .rounded_lg()
        .bg(rgb(palette.panel))
        .border_1()
        .border_color(rgb(palette.border_strong))
        .flex()
        .flex_col()
        .gap_1()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(|_, _, cx| cx.stop_propagation())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .px_2()
                .py_1()
                .border_b_1()
                .border_color(rgb(palette.border))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(palette.text))
                        .child(icon_el(IconName::Send, 12.5, palette.accent))
                        .child("发送到 · 单跳工具接力"),
                )
                .child(
                    div()
                        .id("relay-close")
                        .w(px(20.0))
                        .h(px(20.0))
                        .rounded_sm()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(move |s| s.bg(rgb(palette.hover)))
                        .child(icon_el(IconName::X, 12.0, palette.muted))
                        .on_click(move |_, _, cx| {
                            wb_close.update(cx, |s, cx| {
                                s.relay = None;
                                cx.notify();
                            });
                        }),
                ),
        );

    if targets.is_empty() {
        menu = menu.child(
            div()
                .px_2p5()
                .py_3()
                .text_xs()
                .text_color(rgb(palette.muted))
                .child("当前输出暂无匹配的下一跳兼容工具"),
        );
    } else {
        for tool in targets {
            let wb = workbench.clone();
            menu = menu.child(
                div()
                    .id(format!("relay-target-{}", tool.as_str()))
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1p5()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |s| s.bg(rgb(palette.hover)))
                    .child(icon_el(tool_icon(tool), 13.5, palette.accent))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(tool_name(tool)),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(rgb(palette.muted))
                                    .truncate()
                                    .child(tool_hint(tool)),
                            ),
                    )
                    .on_click(move |_, _, cx| {
                        wb.update(cx, |s, cx| s.launch_relay(tool, cx));
                    }),
            );
        }
    }

    let wb_backdrop = workbench.clone();
    Some(
        div()
            .id("relay-backdrop")
            .absolute()
            .inset_0()
            .on_click(move |_, _, cx| {
                wb_backdrop.update(cx, |s, cx| {
                    s.relay = None;
                    cx.notify();
                });
            })
            .child(menu)
            .into_any_element(),
    )
}
