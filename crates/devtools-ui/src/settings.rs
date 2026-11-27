//! Settings drawer (spec §5): edits `AppSettings`, persists atomically, shows
//! OS-queried autostart and any hotkey degradation banner.

use devtools_core::{CloseBehavior, Locale, MacosDockPolicy, Theme};
use devtools_platform::HotkeyStatus;
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::*;

use crate::theme::Palette;
use crate::workspace::Workbench;

pub fn panel(workbench: &Entity<Workbench>, palette: Palette, cx: &App) -> AnyElement {
    let state = workbench.read(cx);
    let settings = state.settings().clone();
    let hotkey_status = state.platform().hotkey_status();
    let tray_error = state.platform().tray_error().map(str::to_owned);
    let settings_notice = state.settings_notice().map(str::to_owned);
    let autostart = state.autostart_state().cloned();

    let body = div().flex().flex_col().gap_3().p_3().child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .child(
                div()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("设置"),
            )
            .child({
                let wb = workbench.clone();
                div()
                    .id("settings-close")
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_xs()
                    .text_color(rgb(palette.muted))
                    .child("关闭")
                    .on_click(move |_, _, cx| {
                        wb.update(cx, |state, cx| state.close_settings(cx));
                    })
            }),
    );

    let mut body = body.overflow_y_scrollbar();

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
    }

    // ---- 外观 -------------------------------------------------------
    body = body.child(section_title(palette, "外观"));
    body = body.child(segmented_row(
        palette,
        "主题",
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
    ));
    body = body.child(setting_row(
        palette,
        "代码字号",
        "8 – 48 px，立即生效",
        {
            let wb_minus = workbench.clone();
            let wb_plus = workbench.clone();
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    small_button(palette, "font-minus", "−").on_click(move |_, _, cx| {
                        wb_minus.update(cx, |state, cx| state.adjust_font_size(-1, cx));
                    }),
                )
                .child(
                    div()
                        .w(px(36.0))
                        .text_center()
                        .text_xs()
                        .child(settings.editor.font_size.to_string()),
                )
                .child(
                    small_button(palette, "font-plus", "+").on_click(move |_, _, cx| {
                        wb_plus.update(cx, |state, cx| state.adjust_font_size(1, cx));
                    }),
                )
        },
    ));
    body = body.child(toggle_row(
        palette,
        "等宽连字",
        "JetBrains Mono 连字（M4 编辑器接入后生效）",
        settings.editor.ligatures,
        {
            let wb = workbench.clone();
            move |value, cx| {
                wb.update(cx, |state, cx| state.set_ligatures(value, cx));
            }
        },
    ));

    // ---- 语言 -------------------------------------------------------
    body = body.child(section_title(palette, "语言"));
    body = body.child(segmented_row(
        palette,
        "界面语言",
        &[("简体中文", Locale::ZhCn), ("English (V2)", Locale::EnUs)],
        settings.locale,
        {
            let wb = workbench.clone();
            move |locale, cx| {
                wb.update(cx, |state, cx| state.set_locale(locale, cx));
            }
        },
    ));

    // ---- 行为 -------------------------------------------------------
    body = body.child(section_title(palette, "行为"));
    body = body.child(segmented_row(
        palette,
        "关窗行为",
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
    ));
    body = body.child(toggle_row(
        palette,
        "获焦自动检测",
        "工作台获焦时执行一次智能识别（无后台轮询）",
        settings.smart_detection.enabled,
        {
            let wb = workbench.clone();
            move |value, cx| {
                wb.update(cx, |state, cx| state.set_smart_detection(value, cx));
            }
        },
    ));
    body = body.child(toggle_row(
        palette,
        "显示最近使用",
        "仅驻进程内存，彻底退出即清空",
        settings.navigation.show_recents,
        {
            let wb = workbench.clone();
            move |value, cx| {
                wb.update(cx, |state, cx| state.set_show_recents(value, cx));
            }
        },
    ));

    // ---- 全局热键 ---------------------------------------------------
    body = body.child(section_title(palette, "全局热键"));
    body = body.child(
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                div()
                    .text_xs()
                    .text_color(rgb(palette.muted))
                    .child(format!(
                        "当前：{}",
                        settings
                            .hotkeys
                            .global_toggle
                            .clone()
                            .unwrap_or_else(|| "已关闭".to_owned())
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_2()
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
                    .text_xs()
                    .text_color(rgb(palette.muted))
                    .child(hotkey_status.label()),
            ),
    );

    // ---- 开机自启 ---------------------------------------------------
    body = body.child(section_title(palette, "开机自启"));
    body = body.child(
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(toggle_row(
                palette,
                "登录时启动",
                "直接进入静默深休眠；以操作系统为唯一事实源",
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
                    .text_xs()
                    .text_color(rgb(palette.muted))
                    .child(match &autostart {
                        Some(Ok(true)) => "操作系统状态：已启用".to_owned(),
                        Some(Ok(false)) => "操作系统状态：未启用".to_owned(),
                        Some(Err(err)) => format!("无法读取操作系统状态：{err}"),
                        None => "打开设置后将查询操作系统状态".to_owned(),
                    }),
            ),
    );

    // ---- 平台 -------------------------------------------------------
    body = body.child(section_title(palette, "平台"));
    body = body.child(segmented_row(
        palette,
        "macOS Dock",
        &[
            ("仅开窗显示", MacosDockPolicy::ActiveWindowOnly),
            ("仅菜单栏", MacosDockPolicy::MenuBarOnly),
            ("始终显示", MacosDockPolicy::Always),
        ],
        settings.platform.macos_dock_policy,
        {
            let wb = workbench.clone();
            move |policy, cx| {
                wb.update(cx, |state, cx| {
                    state.settings.platform.macos_dock_policy = policy;
                    state.persist_settings();
                    cx.notify();
                });
            }
        },
    ));

    div()
        .w(px(380.0))
        .h_full()
        .flex()
        .flex_col()
        .bg(rgb(palette.panel))
        .border_l_1()
        .border_color(rgb(palette.border))
        .child(body)
        .into_any_element()
}

const HOTKEY_PRESETS: &[(&str, Option<&str>)] = &[
    (
        "默认",
        Some(devtools_platform::hotkey::DEFAULT_GLOBAL_SHORTCUT),
    ),
    (
        "macOS 默认",
        Some(devtools_platform::hotkey::DEFAULT_GLOBAL_SHORTCUT_MACOS),
    ),
    ("备用", Some("Ctrl+Shift+Space")),
    ("关闭", None),
];

fn section_title(palette: Palette, title: &'static str) -> AnyElement {
    div()
        .mt_2()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(palette.accent))
        .child(title)
        .into_any_element()
}

fn banner(palette: Palette, message: String) -> AnyElement {
    div()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(rgb(palette.danger))
        .text_xs()
        .text_color(rgb(palette.danger))
        .child(message)
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
        .py_1()
        .child(
            div()
                .flex()
                .flex_col()
                .child(div().text_xs().child(title))
                .child(div().text_xs().text_color(rgb(palette.muted)).child(hint)),
        )
        .child(control)
        .into_any_element()
}

fn toggle_row(
    palette: Palette,
    title: &'static str,
    hint: &'static str,
    value: bool,
    handler: impl Fn(bool, &mut App) + 'static,
) -> AnyElement {
    let pill = div()
        .id(format!("toggle-{title}"))
        .px_3()
        .py_1()
        .rounded_full()
        .cursor_pointer()
        .text_xs()
        .bg(if value {
            rgb(palette.accent_soft)
        } else {
            rgb(palette.panel_alt)
        })
        .text_color(if value {
            rgb(palette.accent)
        } else {
            rgb(palette.muted)
        })
        .child(if value { "已启用" } else { "已关闭" })
        .on_click(move |_, _, cx| handler(!value, cx));

    setting_row(palette, title, hint, pill)
}

fn small_button(palette: Palette, id: &'static str, label: &'static str) -> Stateful<Div> {
    div()
        .id(id)
        .px_2()
        .py_1()
        .rounded_sm()
        .cursor_pointer()
        .border_1()
        .border_color(rgb(palette.border))
        .bg(rgb(palette.panel_alt))
        .text_xs()
        .text_color(rgb(palette.text))
        .child(label)
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
        .px_2()
        .py_1()
        .rounded_sm()
        .cursor_pointer()
        .border_1()
        .border_color(if selected {
            rgb(palette.accent)
        } else {
            rgb(palette.border)
        })
        .bg(if selected {
            rgb(palette.accent_soft)
        } else {
            rgb(palette.panel_alt)
        })
        .text_xs()
        .text_color(if selected {
            rgb(palette.accent)
        } else {
            rgb(palette.muted)
        })
        .child(label.to_owned())
        .on_click(move |_, _, cx| handler(cx))
        .into_any_element()
}

fn segmented_row<T: PartialEq + Copy + 'static>(
    palette: Palette,
    title: &'static str,
    options: &[(&'static str, T)],
    current: T,
    handler: impl Fn(T, &mut App) + 'static,
) -> AnyElement {
    let mut row = div().flex().flex_wrap().gap_2();
    let handler = std::sync::Arc::new(handler);
    for (index, (label, value)) in options.iter().enumerate() {
        let value = *value;
        let selected = value == current;
        let handler = handler.clone();
        row = row.child(
            div()
                .id(format!("seg-{title}-{index}"))
                .px_2()
                .py_1()
                .rounded_sm()
                .cursor_pointer()
                .border_1()
                .border_color(if selected {
                    rgb(palette.accent)
                } else {
                    rgb(palette.border)
                })
                .bg(if selected {
                    rgb(palette.accent_soft)
                } else {
                    rgb(palette.panel_alt)
                })
                .text_xs()
                .text_color(if selected {
                    rgb(palette.accent)
                } else {
                    rgb(palette.muted)
                })
                .child(*label)
                .on_click(move |_, _, cx| handler(value, cx)),
        );
    }
    setting_row(palette, title, "", row)
}
