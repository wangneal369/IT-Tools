//! Settings drawer (spec §5): edits `AppSettings`, persists atomically, shows
//! OS-queried autostart and any hotkey degradation banner.

use devtools_core::{AutoJumpMode, CloseBehavior, Locale, MacosDockPolicy, Theme, WakePlacement};
use devtools_platform::HotkeyStatus;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use crate::icons::{IconName, icon_el};
use crate::theme::Palette;
use crate::workspace::Workbench;

pub const DRAWER_WIDTH: f32 = 356.0;

pub fn panel(workbench: &Entity<Workbench>, palette: Palette, cx: &App) -> AnyElement {
    let state = workbench.read(cx);
    let settings = state.settings().clone();
    let hotkey_status = state.platform().hotkey_status();
    let hotkey_last_error = state.platform().hotkey_last_error().map(str::to_owned);
    let tray_error = state.platform().tray_error().map(str::to_owned);
    let settings_notice = state.settings_notice().map(str::to_owned);
    let autostart = state.autostart_state().cloned();

    let header = div()
        .h(px(40.0))
        .flex_shrink_0()
        .px_3p5()
        .flex()
        .items_center()
        .justify_between()
        .bg(rgb(palette.chrome))
        .border_b_1()
        .border_color(rgb(palette.border))
        .child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(icon_el(IconName::SlidersHorizontal, 14.0, palette.accent))
                .child(
                    div()
                        .text_size(px(13.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("偏好设置"),
                ),
        )
        .child({
            let wb = workbench.clone();
            div()
                .id("settings-close")
                .w(px(24.0))
                .h(px(24.0))
                .rounded_md()
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .hover(move |s| s.bg(rgb(palette.hover)))
                .child(icon_el(IconName::X, 13.5, palette.muted))
                .on_click(move |_, _, cx| {
                    wb.update(cx, |state, cx| state.close_settings(cx));
                })
        });

    let mut body = div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .gap_3()
        .p_3p5()
        .overflow_y_scrollbar();

    if let Some(reason) = tray_error {
        body = body.child(banner(
            palette,
            format!("托盘不可用：{reason}（关窗将按不可达处理）"),
        ));
    }
    if let Some(notice) = settings_notice {
        body = body.child(banner(palette, notice));
    }
    if let HotkeyStatus::Degraded { reason, .. } = &hotkey_status {
        body = body.child(banner(palette, format!("全局热键降级：{reason}")));
    } else if let Some(err) = hotkey_last_error {
        body = body.child(banner(
            palette,
            format!("最近一次热键修改失败（已保持旧绑定）：{err}"),
        ));
    }

    // ---- 外观与编辑器 ------------------------------------------------
    body = body.child(section_card(
        palette,
        "外观与编辑器",
        vec![
            segmented_row(
                palette,
                "界面主题",
                "切换工作台与代码区配色，立即生效",
                &[
                    ("跟随系统", Theme::System),
                    ("暗色", Theme::Dark),
                    ("亮色", Theme::Light),
                ],
                settings.theme,
                {
                    let wb = workbench.clone();
                    move |theme, cx| {
                        wb.update(cx, |state, cx| state.set_theme(theme, cx));
                    }
                },
            ),
            divider(palette),
            setting_row(
                palette,
                "代码字号",
                "范围 8–48 px，编辑器与输出区同步生效",
                {
                    let wb_minus = workbench.clone();
                    let wb_plus = workbench.clone();
                    div()
                        .flex()
                        .items_center()
                        .gap_1()
                        .p(px(2.0))
                        .rounded_md()
                        .bg(rgb(palette.chrome))
                        .border_1()
                        .border_color(rgb(palette.border))
                        .child(
                            stepper_button(palette, "font-minus", IconName::Minus).on_click(
                                move |_, _, cx| {
                                    wb_minus.update(cx, |state, cx| state.adjust_font_size(-1, cx));
                                },
                            ),
                        )
                        .child(
                            div()
                                .w(px(44.0))
                                .text_center()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .child(format!("{} px", settings.editor.font_size)),
                        )
                        .child(
                            stepper_button(palette, "font-plus", IconName::Plus).on_click(
                                move |_, _, cx| {
                                    wb_plus.update(cx, |state, cx| state.adjust_font_size(1, cx));
                                },
                            ),
                        )
                },
            ),
            divider(palette),
            toggle_row(
                palette,
                "等宽字体连字",
                "启用代码区编程符号连字渲染偏好",
                settings.editor.ligatures,
                {
                    let wb = workbench.clone();
                    move |value, cx| {
                        wb.update(cx, |state, cx| state.set_ligatures(value, cx));
                    }
                },
            ),
        ],
    ));

    // ---- 常驻与导航 --------------------------------------------------
    body = body.child(section_card(
        palette,
        "常驻与窗口行为",
        vec![
            segmented_row(
                palette,
                "关闭按钮行为",
                "关闭主窗口时保持后台驻留或直接退出",
                &[
                    ("收起到托盘", CloseBehavior::DismissToTray),
                    ("退出应用", CloseBehavior::Quit),
                ],
                settings.residency.close,
                {
                    let wb = workbench.clone();
                    move |behavior, cx| {
                        wb.update(cx, |state, cx| state.set_close_behavior(behavior, cx));
                    }
                },
            ),
            divider(palette),
            toggle_row(
                palette,
                "Esc 收起到托盘",
                "无顶层浮层时按 Esc 快速隐藏主窗口",
                settings.residency.esc_to_tray,
                {
                    let wb = workbench.clone();
                    move |value, cx| {
                        wb.update(cx, |state, cx| state.set_esc_to_tray(value, cx));
                    }
                },
            ),
            divider(palette),
            segmented_row(
                palette,
                "窗口唤起位置",
                "从后台唤起主工作台时的屏幕定位策略",
                &[
                    ("跟随屏幕居中", WakePlacement::FollowCursorScreen),
                    ("记忆上次位置", WakePlacement::RememberLastPosition),
                ],
                settings.residency.wake_placement,
                {
                    let wb = workbench.clone();
                    move |placement, cx| {
                        wb.update(cx, |state, cx| state.set_wake_placement(placement, cx));
                    }
                },
            ),
            divider(palette),
            toggle_row(
                palette,
                "标签骨架恢复",
                "重启后仅恢复空标签实例，绝不落盘输入/输出",
                settings.navigation.tab_skeleton_restore,
                {
                    let wb = workbench.clone();
                    move |value, cx| {
                        wb.update(cx, |state, cx| state.set_tab_skeleton_restore(value, cx));
                    }
                },
            ),
            divider(palette),
            toggle_row(
                palette,
                "显示最近使用",
                "仅驻当前进程内存，彻底退出即清空",
                settings.navigation.show_recents,
                {
                    let wb = workbench.clone();
                    move |value, cx| {
                        wb.update(cx, |state, cx| state.set_show_recents(value, cx));
                    }
                },
            ),
        ],
    ));

    // ---- 智能识别与隐私 ----------------------------------------------
    body = body.child(section_card(
        palette,
        "智能识别与隐私护栏",
        vec![
            toggle_row(
                palette,
                "获焦自动检测",
                "工作台获焦时异步识别一次（零后台轮询）",
                settings.smart_detection.enabled,
                {
                    let wb = workbench.clone();
                    move |value, cx| {
                        wb.update(cx, |state, cx| state.set_smart_detection(value, cx));
                    }
                },
            ),
            divider(palette),
            segmented_row(
                palette,
                "唯一高置信命中跳转",
                "默认仅轻提示条推荐，不打断当前编辑",
                &[
                    ("关闭", AutoJumpMode::Off),
                    ("仅首页", AutoJumpMode::HomeOnly),
                    ("总是", AutoJumpMode::Always),
                ],
                settings.smart_detection.auto_jump,
                {
                    let wb = workbench.clone();
                    move |mode, cx| {
                        wb.update(cx, |state, cx| state.set_auto_jump_mode(mode, cx));
                    }
                },
            ),
            divider(palette),
            toggle_row(
                palette,
                "敏感结果排除剪贴板历史",
                "密码 / JWT 复制时写入系统剪贴板隐私排除标记",
                settings
                    .smart_detection
                    .exclude_sensitive_from_clipboard_history,
                {
                    let wb = workbench.clone();
                    move |value, cx| {
                        wb.update(cx, |state, cx| {
                            state.set_sensitive_copy_exclude_history(value, cx)
                        });
                    }
                },
            ),
        ],
    ));

    // ---- 全局热键与自启 ----------------------------------------------
    body = body.child(section_card(
        palette,
        "全局热键与开机自启",
        vec![
            div()
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .font_weight(FontWeight::MEDIUM)
                                .child("全局智能切换热键"),
                        )
                        .child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .bg(rgb(palette.chrome))
                                .border_1()
                                .border_color(rgb(palette.border))
                                .text_size(px(11.0))
                                .text_color(rgb(palette.accent))
                                .child(
                                    settings
                                        .hotkeys
                                        .global_toggle
                                        .clone()
                                        .unwrap_or_else(|| "已禁用".to_owned()),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_wrap()
                        .gap_1p5()
                        .children(HOTKEY_PRESETS.iter().map(|(label, value)| {
                            let wb = workbench.clone();
                            let value = *value;
                            let selected = settings.hotkeys.global_toggle.as_deref() == value;
                            chip(
                                palette,
                                format!("hotkey-{label}"),
                                label,
                                selected,
                                move |cx| {
                                    wb.update(cx, |state, cx| {
                                        state.apply_hotkey(value.map(str::to_owned), cx)
                                    });
                                },
                            )
                        })),
                )
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(palette.muted))
                        .child(hotkey_status.label()),
                )
                .into_any_element(),
            divider(palette),
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(toggle_row(
                    palette,
                    "登录时静默自启",
                    "以 --silent 直接进入深休眠；状态实时对齐系统",
                    matches!(autostart, Some(Ok(true))),
                    {
                        let wb = workbench.clone();
                        move |_value, cx| {
                            wb.update(cx, |state, cx| state.toggle_autostart(cx));
                        }
                    },
                ))
                .child(
                    div()
                        .text_size(px(11.0))
                        .text_color(rgb(palette.subtle))
                        .child(match &autostart {
                            Some(Ok(true)) => "操作系统登录项状态：已启用".to_owned(),
                            Some(Ok(false)) => "操作系统登录项状态：未启用".to_owned(),
                            Some(Err(err)) => format!("读取系统登录项失败：{err}"),
                            None => "正在同步操作系统登录项状态…".to_owned(),
                        }),
                )
                .into_any_element(),
        ],
    ));

    // ---- 平台与语言 --------------------------------------------------
    body = body.child(section_card(
        palette,
        "平台与语言",
        vec![
            segmented_row(
                palette,
                "界面语言",
                "V1 以简体中文为主，多语言将在 V2 开放",
                &[("简体中文", Locale::ZhCn), ("English (V2)", Locale::EnUs)],
                settings.locale,
                {
                    let wb = workbench.clone();
                    move |locale, cx| {
                        wb.update(cx, |state, cx| state.set_locale(locale, cx));
                    }
                },
            ),
            divider(palette),
            segmented_row(
                palette,
                "macOS Dock 图标",
                "控制收起到菜单栏后 Dock 图标显隐策略",
                &[
                    ("仅开窗", MacosDockPolicy::ActiveWindowOnly),
                    ("仅菜单栏", MacosDockPolicy::MenuBarOnly),
                    ("常驻", MacosDockPolicy::Always),
                ],
                settings.platform.macos_dock_policy,
                {
                    let wb = workbench.clone();
                    move |policy, cx| {
                        wb.update(cx, |state, cx| state.set_macos_dock_policy(policy, cx));
                    }
                },
            ),
            divider(palette),
            toggle_row(
                palette,
                "Linux 无托盘强制常驻",
                "无 SNI 托盘宿主时关窗不退出（需 --toggle 唤起）",
                settings.platform.linux_force_tray_without_sni,
                {
                    let wb = workbench.clone();
                    move |value, cx| {
                        wb.update(cx, |state, cx| state.set_linux_force_tray(value, cx));
                    }
                },
            ),
        ],
    ));

    div()
        .w(px(DRAWER_WIDTH))
        .flex_shrink_0()
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(palette.panel))
        .border_l_1()
        .border_color(rgb(palette.border))
        .child(header)
        .child(body)
        .into_any_element()
}

const HOTKEY_PRESETS: &[(&str, Option<&str>)] = &[
    (
        "Win/Linux 默认",
        Some(devtools_platform::hotkey::DEFAULT_GLOBAL_SHORTCUT),
    ),
    (
        "macOS 默认",
        Some(devtools_platform::hotkey::DEFAULT_GLOBAL_SHORTCUT_MACOS),
    ),
    ("备用 Ctrl+Shift+Space", Some("Ctrl+Shift+Space")),
    ("禁用", None),
];

fn section_card(palette: Palette, title: &'static str, rows: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap_2p5()
        .p_3()
        .rounded_lg()
        .bg(rgb(palette.panel_alt))
        .border_1()
        .border_color(rgb(palette.border))
        .child(
            div()
                .text_size(px(11.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(palette.accent))
                .child(title),
        )
        .children(rows)
        .into_any_element()
}

fn divider(palette: Palette) -> AnyElement {
    div()
        .h(px(1.0))
        .w_full()
        .bg(rgb(palette.border))
        .into_any_element()
}

fn banner(palette: Palette, message: String) -> AnyElement {
    div()
        .flex()
        .items_start()
        .gap_2()
        .p_2p5()
        .rounded_md()
        .bg(rgb(palette.danger_soft))
        .border_1()
        .border_color(rgb(palette.danger))
        .text_xs()
        .text_color(rgb(palette.danger))
        .child(icon_el(IconName::CircleAlert, 13.5, palette.danger))
        .child(div().flex_1().min_w_0().child(message))
        .into_any_element()
}

fn setting_row(
    palette: Palette,
    title: &'static str,
    hint: &'static str,
    control: impl IntoElement,
) -> AnyElement {
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
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(palette.text))
                        .child(title),
                )
                .when(!hint.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(palette.muted))
                            .child(hint),
                    )
                }),
        )
        .child(div().flex_shrink_0().child(control))
        .into_any_element()
}

fn toggle_row(
    palette: Palette,
    title: &'static str,
    hint: &'static str,
    value: bool,
    handler: impl Fn(bool, &mut App) + 'static,
) -> AnyElement {
    let switch = div()
        .id(format!("toggle-{title}"))
        .w(px(36.0))
        .h(px(20.0))
        .p(px(2.0))
        .rounded_full()
        .cursor_pointer()
        .flex()
        .items_center()
        .when(value, |el| el.justify_end())
        .when(!value, |el| el.justify_start())
        .bg(rgb(if value {
            palette.accent
        } else {
            palette.border_strong
        }))
        .child(
            div()
                .w(px(16.0))
                .h(px(16.0))
                .rounded_full()
                .bg(rgb(0xffffff)),
        )
        .on_click(move |_, _, cx| handler(!value, cx));

    setting_row(palette, title, hint, switch)
}

fn stepper_button(palette: Palette, id: &'static str, icon: IconName) -> Stateful<Div> {
    div()
        .id(id)
        .w(px(22.0))
        .h(px(22.0))
        .rounded_sm()
        .cursor_pointer()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgb(palette.panel))
        .hover(move |s| s.bg(rgb(palette.hover)))
        .child(icon_el(icon, 12.0, palette.text))
}

fn chip(
    palette: Palette,
    id: String,
    label: &str,
    selected: bool,
    handler: impl Fn(&mut App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .px_2p5()
        .py_1()
        .rounded_md()
        .cursor_pointer()
        .border_1()
        .border_color(rgb(if selected {
            palette.accent
        } else {
            palette.border
        }))
        .bg(rgb(if selected {
            palette.accent_soft
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
        .text_size(px(11.5))
        .font_weight(if selected {
            FontWeight::MEDIUM
        } else {
            FontWeight::NORMAL
        })
        .text_color(rgb(if selected {
            palette.accent
        } else {
            palette.muted
        }))
        .child(label.to_owned())
        .on_click(move |_, _, cx| handler(cx))
        .into_any_element()
}

fn segmented_row<T: PartialEq + Copy + 'static>(
    palette: Palette,
    title: &'static str,
    hint: &'static str,
    options: &[(&'static str, T)],
    current: T,
    handler: impl Fn(T, &mut App) + 'static,
) -> AnyElement {
    let mut track = div()
        .w_full()
        .flex()
        .gap_0p5()
        .p(px(2.0))
        .rounded_md()
        .bg(rgb(palette.chrome))
        .border_1()
        .border_color(rgb(palette.border));

    let handler = std::sync::Arc::new(handler);
    for (index, (label, value)) in options.iter().enumerate() {
        let value = *value;
        let selected = value == current;
        let handler = handler.clone();
        track = track.child(
            div()
                .id(format!("seg-{title}-{index}"))
                .flex_1()
                .min_w_0()
                .py_1()
                .px_2()
                .rounded_sm()
                .cursor_pointer()
                .text_center()
                .text_size(px(11.5))
                .font_weight(if selected {
                    FontWeight::MEDIUM
                } else {
                    FontWeight::NORMAL
                })
                .bg(rgb(if selected {
                    palette.panel
                } else {
                    palette.chrome
                }))
                .when(selected, |el| {
                    el.border_1().border_color(rgb(palette.border_strong))
                })
                .hover(move |s| {
                    s.bg(rgb(if selected {
                        palette.panel
                    } else {
                        palette.hover
                    }))
                })
                .text_color(rgb(if selected {
                    palette.accent
                } else {
                    palette.muted
                }))
                .truncate()
                .child(*label)
                .on_click(move |_, _, cx| handler(value, cx)),
        );
    }

    div()
        .flex()
        .flex_col()
        .gap_1p5()
        .child(
            div()
                .flex()
                .flex_col()
                .gap_0p5()
                .child(
                    div()
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(palette.text))
                        .child(title),
                )
                .when(!hint.is_empty(), |el| {
                    el.child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(palette.muted))
                            .child(hint),
                    )
                }),
        )
        .child(track)
        .into_any_element()
}
