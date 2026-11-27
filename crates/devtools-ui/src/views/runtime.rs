use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use devtools_core::{CancelFlag, RawPayload, ToolError, ToolErrorKind, ToolId};
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState};
use gpui_kit::*;

use super::Model;
use crate::theme::Palette;
use crate::tool_view::{SplitMode, ToolViewContext};
use crate::workspace::Workbench;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Debounce {
    After(Duration),
    Manual,
}

pub fn debounce(bytes: usize, immediate: bool) -> Debounce {
    if immediate {
        Debounce::After(Duration::ZERO)
    } else if bytes <= 64 * 1024 {
        Debounce::After(Duration::from_millis(100))
    } else if bytes <= 1024 * 1024 {
        Debounce::After(Duration::from_millis(250))
    } else {
        Debounce::Manual
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum ExecutionState {
    #[default]
    Idle,
    Pending,
    Running,
    Valid,
    Stale(ToolError),
    Failed(ToolError),
    Cancelled,
    Manual,
}

#[derive(Default)]
pub struct ToolRuntime {
    pub generation: u64,
    pub state: ExecutionState,
    pub output: Option<Arc<str>>,
    cancel: CancelFlag,
}

impl ToolRuntime {
    pub fn invalidate(&mut self) -> (u64, CancelFlag) {
        self.cancel.cancel();
        self.generation = self
            .generation
            .checked_add(1)
            .expect("tool generation exhausted");
        self.cancel = CancelFlag::new();
        self.state = ExecutionState::Pending;
        (self.generation, self.cancel.clone())
    }

    pub fn complete(&mut self, generation: u64, result: Result<Arc<str>, ToolError>) -> bool {
        if generation != self.generation || self.cancel.is_cancelled() {
            return false;
        }
        match result {
            Ok(output) => {
                self.output = Some(output);
                self.state = ExecutionState::Valid;
            }
            Err(error) if error.kind == ToolErrorKind::Cancelled => {
                self.state = ExecutionState::Cancelled
            }
            Err(error) if error.kind == ToolErrorKind::InvalidSyntax && self.output.is_some() => {
                self.state = ExecutionState::Stale(error);
            }
            Err(error) => {
                self.output = None;
                self.state = ExecutionState::Failed(error);
            }
        }
        true
    }

    pub fn cancel(&mut self) {
        self.cancel.cancel();
        self.state = ExecutionState::Cancelled;
    }

    pub fn can_copy(&self) -> bool {
        self.state == ExecutionState::Valid && self.output.is_some()
    }
}

impl Drop for ToolRuntime {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[derive(Clone, Default)]
pub(super) struct Inputs {
    pub text: String,
    pub second: String,
    pub query: String,
    pub replacement: String,
    pub secret: String,
    pub expected: String,
    pub payload: Option<RawPayload>,
    pub progress: Arc<Mutex<Option<devtools_tools::Progress>>>,
}

impl Inputs {
    fn bytes(&self) -> usize {
        self.text.len()
            + self.second.len()
            + self.query.len()
            + self.replacement.len()
            + self.secret.len()
            + self.expected.len()
    }
}

#[derive(Clone, Default)]
pub(super) struct ToolResult {
    pub output: String,
    pub rows: Vec<(String, String)>,
    pub table: Option<(Vec<String>, Vec<Vec<String>>)>,
    pub colored: Vec<(String, bool)>,
    pub swatch: Option<u32>,
    pub image: Option<Arc<[u8]>>,
    pub footer: String,
    pub strength: Option<u8>,
    pub highlights: Vec<(std::ops::Range<usize>, bool)>,
    pub highlighted_text: String,
    pub image_format: Option<ImageFormat>,
}

impl ToolResult {
    pub fn text(output: impl Into<String>) -> Self {
        Self {
            output: output.into(),
            ..Self::default()
        }
    }

    pub fn rows(rows: Vec<(String, String)>) -> Self {
        Self {
            output: rows
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join("\n"),
            rows,
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Slot {
    Text,
    Second,
    Query,
    Replacement,
    Secret,
    Expected,
}

impl Slot {
    fn get(self, inputs: &Inputs) -> &str {
        match self {
            Self::Text => &inputs.text,
            Self::Second => &inputs.second,
            Self::Query => &inputs.query,
            Self::Replacement => &inputs.replacement,
            Self::Secret => &inputs.secret,
            Self::Expected => &inputs.expected,
        }
    }
    fn set(self, inputs: &mut Inputs, value: String) {
        match self {
            Self::Text => inputs.text = value,
            Self::Second => inputs.second = value,
            Self::Query => inputs.query = value,
            Self::Replacement => inputs.replacement = value,
            Self::Secret => inputs.secret = value,
            Self::Expected => inputs.expected = value,
        }
    }
}

struct Editors {
    text: Entity<EditorState>,
    second: Entity<EditorState>,
    output: Entity<EditorState>,
    query: Entity<InputState>,
    replacement: Entity<InputState>,
    secret: Entity<InputState>,
    expected: Entity<InputState>,
    color: Entity<ColorPickerState>,
    _subscriptions: Vec<Subscription>,
}

pub(super) struct ToolView {
    pub context: ToolViewContext,
    workbench: WeakEntity<Workbench>,
    pub model: Model,
    pub inputs: Inputs,
    runtime: ToolRuntime,
    result: Option<ToolResult>,
    editors: Option<Editors>,
    split: SplitMode,
    session_input: Option<RawPayload>,
    session_launch: Option<devtools_core::ToolLaunchInput>,
    session_revision: u64,
    needs_run: bool,
    task: Option<Task<()>>,
    started: Option<Instant>,
    _session_subscription: Subscription,
}

impl ToolView {
    pub fn new(
        context: ToolViewContext,
        workbench: WeakEntity<Workbench>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscription = cx.observe(&context.session, |this, session, cx| {
            let session = session.read(cx);
            if session.input_revision != this.session_revision
                || session.input != this.session_input
                || session.launch_input != this.session_launch
            {
                this.session_revision = session.input_revision;
                let payload = session.input.clone();
                let launch = session.launch_input.clone();
                this.accept_payload(payload, launch);
                this.schedule(true, false, cx);
            }
            cx.notify();
        });
        let initial = context.session.read(cx);
        let payload = initial.input.clone();
        let launch = initial.launch_input.clone();
        let revision = initial.input_revision;
        let mut this = Self {
            model: Model::new(context.tool_id),
            context,
            workbench,
            inputs: Inputs::default(),
            runtime: ToolRuntime::default(),
            result: None,
            editors: None,
            split: SplitMode::SideBySide,
            session_input: None,
            session_launch: None,
            session_revision: revision,
            needs_run: true,
            task: None,
            started: None,
            _session_subscription: subscription,
        };
        this.accept_payload(payload, launch);
        this
    }

    fn accept_payload(
        &mut self,
        payload: Option<RawPayload>,
        launch: Option<devtools_core::ToolLaunchInput>,
    ) {
        self.session_input = payload.clone();
        self.session_launch = launch.clone();
        self.inputs.payload = payload.clone();
        if let Some(RawPayload::Text(text)) = &payload {
            let is_digest = launch
                .as_ref()
                .and_then(|l| l.hint.as_ref())
                .is_some_and(|h| h.data_type == devtools_core::DataType::HashDigest);
            if self.context.tool_id == ToolId::HashHmac && is_digest {
                self.inputs.expected = text.to_string();
                self.inputs.text.clear();
            } else {
                self.inputs.text = text.to_string();
                if matches!(
                    self.context.tool_id,
                    ToolId::TextDiff | ToolId::JsonDiff | ToolId::ListCompare
                ) && let Some((left, right)) =
                    text.split_once(devtools_tools::QUICK_INPUT_SEPARATOR)
                {
                    self.inputs.text = left.to_owned();
                    self.inputs.second = right.to_owned();
                }
            }
            match &mut self.model {
                Model::Standard(options) => options.inject(text, launch.as_ref()),
                Model::Converter(options) => options.inject(text, launch.as_ref()),
                Model::Debug(options) => options.inject(text),
            }
        }
    }

    fn publish_input(&mut self, cx: &mut Context<Self>) {
        // Include both editable sides in the authority used by anti-overwrite.
        let text = if matches!(
            self.context.tool_id,
            ToolId::TextDiff | ToolId::JsonDiff | ToolId::ListCompare
        ) && !self.inputs.second.is_empty()
        {
            format!(
                "{}{}{}",
                self.inputs.text,
                devtools_tools::QUICK_INPUT_SEPARATOR,
                self.inputs.second
            )
        } else {
            self.inputs.text.clone()
        };
        let payload = match &self.inputs.payload {
            Some(payload @ (RawPayload::Files(_) | RawPayload::Image(_))) => Some(payload.clone()),
            _ => Some(RawPayload::Text(text.into())),
        };
        self.session_input = payload.clone();
        self.inputs.payload = payload.clone();
        self.context.session.update(cx, |session, cx| {
            session.input = payload;
            session.output = None;
            cx.notify();
        });
    }

    pub fn changed(&mut self, cx: &mut Context<Self>) {
        self.publish_input(cx);
        self.schedule(false, false, cx);
    }

    fn schedule(&mut self, immediate: bool, manual: bool, cx: &mut Context<Self>) {
        self.needs_run = false;
        let (generation, cancel) = self.runtime.invalidate();
        self.task = None;
        self.started = None;
        self.inputs.progress = Arc::new(Mutex::new(None));
        self.context.session.update(cx, |session, cx| {
            session.output = None;
            cx.notify();
        });
        let mut tier = debounce(self.inputs.bytes(), immediate);
        if !immediate
            && self.context.tool_id == ToolId::TextDiff
            && (self.inputs.text.lines().count() > 20_000
                || self.inputs.second.lines().count() > 20_000)
        {
            tier = Debounce::Manual;
        }
        let Debounce::After(delay) = tier else {
            self.runtime.state = ExecutionState::Manual;
            cx.notify();
            return;
        };
        let model = self.model.clone();
        let inputs = self.inputs.clone();
        self.task = Some(cx.spawn(async move |weak, cx| {
            cx.background_executor().timer(delay).await;
            if cancel.is_cancelled() {
                return;
            }
            cx.update(|cx| {
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |view, cx| {
                        view.runtime.state = ExecutionState::Running;
                        view.started = Some(Instant::now());
                        cx.notify();
                    });
                }
            });
            let worker_cancel = cancel.clone();
            let worker = cx.background_executor().spawn(async move {
                if worker_cancel.is_cancelled() {
                    return Err(ToolError::new(ToolErrorKind::Cancelled, "已取消"));
                }
                model.execute(&inputs, &worker_cancel, manual)
            });
            // A delayed notification makes progress visible only after 150 ms.
            let progress_view = weak.clone();
            let progress_cancel = cancel.clone();
            cx.spawn(async move |cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                loop {
                    if progress_cancel.is_cancelled() {
                        break;
                    }
                    let running = cx.update(|cx| {
                        progress_view.upgrade().is_some_and(|view| {
                            view.update(cx, |view, cx| {
                                if view.runtime.generation != generation
                                    || view.runtime.state != ExecutionState::Running
                                {
                                    return false;
                                }
                                cx.notify();
                                true
                            })
                        })
                    });
                    if !running {
                        break;
                    }
                    cx.background_executor()
                        .timer(Duration::from_millis(50))
                        .await;
                }
            })
            .detach();
            let result = worker.await;
            cx.update(|cx| {
                if let Some(view) = weak.upgrade() {
                    view.update(cx, |this, cx| {
                        let output = result
                            .as_ref()
                            .map(|r| Arc::<str>::from(r.output.as_str()))
                            .map_err(Clone::clone);
                        if !this.runtime.complete(generation, output) {
                            return;
                        }
                        if let Ok(result) = result {
                            this.result = Some(result);
                        }
                        if matches!(this.runtime.state, ExecutionState::Failed(_)) {
                            this.result = None;
                        }
                        this.context.session.update(cx, |session, cx| {
                            session.output = this.runtime.can_copy().then(|| {
                                RawPayload::Text(this.runtime.output.clone().expect("valid output"))
                            });
                            session.sensitive_output =
                                this.context.tool_id.default_capabilities().sensitive_output;
                            cx.notify();
                        });
                        cx.notify();
                    });
                }
            });
        }));
        cx.notify();
    }

    pub(crate) fn release_window(&mut self) {
        if matches!(
            self.runtime.state,
            ExecutionState::Pending | ExecutionState::Running
        ) {
            self.runtime.cancel();
            self.needs_run = true;
        }
        self.task = None;
        self.editors = None;
    }

    fn ensure_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editors.is_none() {
            let text = cx.new(|cx| EditorState::new(window, cx));
            let second = cx.new(|cx| EditorState::new(window, cx));
            let output = cx.new(|cx| EditorState::new(window, cx));
            let query = cx.new(|cx| InputState::new(window, cx));
            let replacement = cx.new(|cx| InputState::new(window, cx));
            let secret = cx.new(|cx| InputState::new(window, cx).masked(true));
            let expected = cx.new(|cx| InputState::new(window, cx));
            let color = cx.new(|cx| ColorPickerState::new(window, cx).default_value(rgb(0x5e9eff)));
            let mut subscriptions = vec![cx.subscribe(&color, |this, _, event, cx| {
                if let ColorPickerEvent::Change(Some(color)) = event {
                    let color = color.to_rgb();
                    let channel = |value: f32| (value * 255.).round() as u8;
                    this.inputs.text = format!(
                        "#{:02x}{:02x}{:02x}{:02x}",
                        channel(color.r),
                        channel(color.g),
                        channel(color.b),
                        channel(color.a)
                    );
                    this.changed(cx);
                }
            })];
            for (slot, state) in [(Slot::Text, &text), (Slot::Second, &second)] {
                subscriptions.push(cx.subscribe(state, move |this, state, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = state.read(cx).value().to_string();
                        if matches!(slot, Slot::Text) {
                            this.inputs.payload = Some(RawPayload::Text(value.clone().into()));
                        }
                        slot.set(&mut this.inputs, value);
                        this.changed(cx);
                    }
                }));
            }
            for (slot, state) in [
                (Slot::Query, &query),
                (Slot::Replacement, &replacement),
                (Slot::Secret, &secret),
                (Slot::Expected, &expected),
            ] {
                subscriptions.push(cx.subscribe(state, move |this, state, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        slot.set(&mut this.inputs, state.read(cx).value().to_string());
                        this.changed(cx);
                    }
                }));
            }
            self.editors = Some(Editors {
                text,
                second,
                output,
                query,
                replacement,
                secret,
                expected,
                color,
                _subscriptions: subscriptions,
            });
        }
        let editors = self.editors.as_ref().expect("initialized editors");
        let output = self.runtime.output.as_deref().unwrap_or_default();
        if editors.output.read(cx).value().as_ref() != output {
            editors.output.update(cx, |state, cx| {
                state.set_value(output.to_owned(), window, cx)
            });
        }
        for (slot, state) in [(Slot::Text, &editors.text), (Slot::Second, &editors.second)] {
            if state.read(cx).value().as_ref() != slot.get(&self.inputs) {
                state.update(cx, |state, cx| {
                    state.set_value(slot.get(&self.inputs).to_owned(), window, cx)
                });
            }
        }
        for (slot, state) in [
            (Slot::Query, &editors.query),
            (Slot::Replacement, &editors.replacement),
            (Slot::Secret, &editors.secret),
            (Slot::Expected, &editors.expected),
        ] {
            if state.read(cx).value().as_ref() != slot.get(&self.inputs) {
                state.update(cx, |state, cx| {
                    state.set_value(slot.get(&self.inputs).to_owned(), window, cx)
                });
            }
        }
    }

    pub fn color_picker(&self) -> AnyElement {
        ColorPicker::new(&self.editors.as_ref().expect("editors mounted").color)
            .label("拾取颜色 / Alpha")
            .into_any_element()
    }

    pub fn field(&self, slot: Slot, label: &str) -> AnyElement {
        let editors = self.editors.as_ref().expect("editors mounted");
        let field = match slot {
            Slot::Text => Editor::new(&editors.text).h(px(230.)).into_any_element(),
            Slot::Second => Editor::new(&editors.second).h(px(230.)).into_any_element(),
            Slot::Query => Input::new(&editors.query).into_any_element(),
            Slot::Replacement => Input::new(&editors.replacement).into_any_element(),
            Slot::Secret => Input::new(&editors.secret).into_any_element(),
            Slot::Expected => Input::new(&editors.expected).into_any_element(),
        };
        div()
            .flex()
            .flex_col()
            .gap_1()
            .w_full()
            .min_w_0()
            .child(div().text_xs().child(label.to_owned()))
            .child(field)
            .into_any_element()
    }

    pub fn chip(
        &self,
        id: &str,
        label: impl Into<SharedString>,
        active: bool,
        palette: Palette,
        cx: &Context<Self>,
        change: impl Fn(&mut Self) + 'static,
    ) -> AnyElement {
        let immediate = matches!(id, "regenerate" | "now");
        div()
            .id(id.to_owned())
            .px_2()
            .py_1()
            .rounded_sm()
            .text_xs()
            .cursor_pointer()
            .bg(rgb(if active {
                palette.accent_soft
            } else {
                palette.panel_alt
            }))
            .text_color(rgb(if active {
                palette.accent
            } else {
                palette.muted
            }))
            .child(label.into())
            .on_click(cx.listener(move |this, _, _, cx| {
                change(this);
                this.publish_input(cx);
                this.schedule(immediate, immediate, cx);
            }))
            .into_any_element()
    }

    pub fn copy_row(
        &self,
        id: usize,
        key: &str,
        value: &str,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let value = value.to_owned();
        let copy = value.clone();
        let enabled = self.runtime.can_copy();
        div()
            .id(format!("result-row-{id}"))
            .flex()
            .gap_3()
            .p_2()
            .border_b_1()
            .border_color(rgb(palette.border))
            .cursor_pointer()
            .child(
                div()
                    .w(px(130.))
                    .flex_shrink_0()
                    .text_color(rgb(palette.muted))
                    .child(key.to_owned()),
            )
            .child(div().flex_1().min_w_0().child(value))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !enabled {
                    return;
                }
                let text: Arc<str> = copy.clone().into();
                this.context.session.update(cx, |session, _| {
                    session.output = Some(RawPayload::Text(text))
                });
                if let Some(wb) = this.workbench.upgrade() {
                    wb.update(cx, |wb, cx| wb.copy_output(this.context.tab_id, false, cx));
                }
                this.context.session.update(cx, |session, _| {
                    session.output = this.runtime.can_copy().then(|| {
                        RawPayload::Text(this.runtime.output.clone().expect("valid output"))
                    })
                });
            }))
            .into_any_element()
    }

    fn header(&self, palette: Palette, cx: &Context<Self>) -> AnyElement {
        let favorite = self
            .workbench
            .upgrade()
            .is_some_and(|wb| wb.read(cx).is_favorite(self.context.tool_id));
        let mut actions = div().flex().items_center().gap_1();
        for (index, (mode, label)) in [
            (SplitMode::SideBySide, "左右"),
            (SplitMode::Stacked, "上下"),
            (SplitMode::Single, "单栏"),
        ]
        .into_iter()
        .enumerate()
        {
            actions = actions.child(
                div()
                    .id(format!("split-{index}"))
                    .px_2()
                    .text_xs()
                    .cursor_pointer()
                    .text_color(rgb(if self.split == mode {
                        palette.accent
                    } else {
                        palette.muted
                    }))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.split = mode;
                        cx.notify();
                    })),
            );
        }
        for (id, label) in [
            ("favorite", if favorite { "★" } else { "☆" }),
            ("new", "+ 多开"),
            ("relay", "发送到 ▸"),
            ("run", "运行"),
            ("cancel", "取消"),
            ("copy", "复制"),
            ("dismiss", "复制并收起"),
        ] {
            let enabled = !matches!(id, "relay" | "copy" | "dismiss") || self.runtime.can_copy();
            actions = actions.child(
                div()
                    .id(id)
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .text_xs()
                    .cursor_pointer()
                    .bg(rgb(if id == "dismiss" && enabled {
                        palette.accent
                    } else {
                        palette.panel_alt
                    }))
                    .text_color(rgb(if !enabled {
                        palette.muted
                    } else if id == "dismiss" {
                        0xffffff
                    } else {
                        palette.text
                    }))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !enabled {
                            return;
                        }
                        match id {
                            "run" => this.schedule(true, true, cx),
                            "cancel" => {
                                this.runtime.cancel();
                                this.task = None;
                                cx.notify();
                            }
                            _ => {
                                if let Some(wb) = this.workbench.upgrade() {
                                    wb.update(cx, |wb, cx| match id {
                                        "favorite" => wb.toggle_favorite(this.context.tool_id, cx),
                                        "new" => wb.open_tool(this.context.tool_id, cx),
                                        "relay" => wb.open_relay(this.context.tab_id, cx),
                                        "copy" | "dismiss" => {
                                            wb.copy_output(this.context.tab_id, id == "dismiss", cx)
                                        }
                                        _ => (),
                                    });
                                }
                            }
                        }
                    })),
            );
        }
        div()
            .h(px(44.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .px_3()
            .border_b_1()
            .border_color(rgb(palette.border))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(crate::labels::tool_name(self.context.tool_id))
                    .child(
                        div()
                            .text_xs()
                            .text_color(rgb(palette.accent))
                            .child(format!("#{}", self.context.instance)),
                    ),
            )
            .child(actions)
            .into_any_element()
    }

    fn output(&self, palette: Palette, cx: &Context<Self>) -> AnyElement {
        let mut out = div()
            .id("tool-output")
            .flex()
            .flex_col()
            .gap_2()
            .min_w_0()
            .size_full()
            .overflow_y_scroll()
            .text_sm();
        if self.context.tool_id.default_capabilities().sensitive_output {
            out = out.child(
                div()
                    .text_xs()
                    .text_color(rgb(palette.accent))
                    .child("敏感输出 · 复制时排除剪贴板历史"),
            );
        }
        let Some(result) = &self.result else {
            return out.child("等待输入 / 运行").into_any_element();
        };
        if !result.highlights.is_empty() {
            out = out.child(
                StyledText::new(result.highlighted_text.clone()).with_highlights(
                    result.highlights.iter().map(|(range, alternate)| {
                        (
                            range.clone(),
                            HighlightStyle {
                                background_color: Some(
                                    rgb(if *alternate {
                                        palette.accent_soft
                                    } else {
                                        palette.border
                                    })
                                    .into(),
                                ),
                                font_weight: Some(FontWeight::BOLD),
                                ..HighlightStyle::default()
                            },
                        )
                    }),
                ),
            );
        }
        if let Some(color) = result.swatch {
            out = out.child(div().h(px(80.)).w_full().rounded_md().bg(rgb(color)));
        }
        if let Some(score) = result.strength {
            out = out
                .child(div().text_xs().child(format!("zxcvbn 强度 {score} / 4")))
                .child(
                    div()
                        .w_full()
                        .h(px(8.))
                        .bg(rgb(palette.accent_soft))
                        .rounded_sm()
                        .child(
                            div()
                                .h_full()
                                .w(relative(f32::from(score) / 4.))
                                .bg(rgb(palette.accent))
                                .rounded_sm(),
                        ),
                );
        }
        if let Some(image) = &result.image {
            out = out.child(
                img(Arc::new(Image::from_bytes(
                    result.image_format.unwrap_or(ImageFormat::Png),
                    image.to_vec(),
                )))
                .w(px(256.))
                .h(px(256.)),
            );
        }
        if let Some((columns, rows)) = &result.table {
            let mut table = div().flex().flex_col().text_xs();
            table = table.child(div().flex().children(columns.iter().enumerate().map(
                |(index, name)| {
                    div()
                        .id(format!("sort-{index}"))
                        .flex_1()
                        .min_w(px(100.))
                        .p_2()
                        .font_weight(FontWeight::BOLD)
                        .cursor_pointer()
                        .child(format!("{name} ↕"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some((_, rows)) =
                                this.result.as_mut().and_then(|r| r.table.as_mut())
                            {
                                if rows
                                    .windows(2)
                                    .all(|pair| pair[0].get(index) <= pair[1].get(index))
                                {
                                    rows.reverse();
                                } else {
                                    rows.sort_by(|a, b| a.get(index).cmp(&b.get(index)));
                                }
                                cx.notify();
                            }
                        }))
                },
            )));
            let count = rows.len();
            table = table.child(
                uniform_list(
                    "result-table",
                    count,
                    cx.processor(move |this, range: std::ops::Range<usize>, _, _| {
                        let rows = &this
                            .result
                            .as_ref()
                            .expect("table result")
                            .table
                            .as_ref()
                            .expect("table")
                            .1;
                        range
                            .map(|index| {
                                div()
                                    .id(index)
                                    .h(px(30.))
                                    .flex()
                                    .border_b_1()
                                    .border_color(rgb(palette.border))
                                    .children(rows[index].iter().map(|cell| {
                                        div()
                                            .flex_1()
                                            .min_w(px(100.))
                                            .px_2()
                                            .overflow_hidden()
                                            .child(cell.clone())
                                    }))
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .h(px(340.)),
            );
            out = out.child(table);
        }
        for (index, (key, value)) in result.rows.iter().enumerate() {
            out = out.child(self.copy_row(index, key, value, palette, cx));
        }
        if !result.colored.is_empty() {
            for (text, added) in &result.colored {
                out = out.child(
                    div()
                        .text_color(rgb(if *added {
                            palette.accent
                        } else {
                            palette.danger
                        }))
                        .child(text.clone()),
                );
            }
        } else if result.rows.is_empty() && result.table.is_none() {
            out = out.child(
                Editor::new(&self.editors.as_ref().expect("editors mounted").output)
                    .readonly(true)
                    .h(px(420.)),
            );
        }
        out.into_any_element()
    }
}

impl Render for ToolView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_editors(window, cx);
        if self.needs_run {
            self.schedule(true, false, cx);
        }
        let palette = self
            .workbench
            .upgrade()
            .map(|wb| crate::theme::resolve(wb.read(cx).settings().theme, window))
            .unwrap_or(crate::theme::DARK);
        let controls = match &self.model {
            Model::Standard(options) => options.controls(self, palette, cx),
            Model::Converter(options) => options.controls(self, palette, cx),
            Model::Debug(options) => options.controls(self, palette, cx),
        };
        let fields = inputs_for_single(self);
        let mut input_pane = div().flex().flex_col().gap_2();
        match &self.inputs.payload {
            Some(RawPayload::Files(paths)) => {
                input_pane = input_pane.child(div().text_xs().child(format!(
                        "文件：{}",
                        paths
                            .iter()
                            .map(|p| p.display().to_string())
                            .collect::<Vec<_>>()
                            .join(" · ")
                    )))
            }
            Some(RawPayload::Image(image)) => {
                input_pane = input_pane.child(div().text_xs().child(format!(
                    "图片：{} × {} · {} 字节",
                    image.width,
                    image.height,
                    image.bytes.len()
                )))
            }
            _ => (),
        }
        let inputs = input_pane.child(fields);
        let mut body = div().flex_1().min_h_0().min_w_0().flex().gap_3().p_3();
        if self.split == SplitMode::Stacked {
            body = body.flex_col();
        }
        body = body
            .child(
                div()
                    .id("input-scroll")
                    .overflow_y_scroll()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(inputs),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .min_h_0()
                    .child(self.output(palette, cx)),
            );
        let body = if self.split == SplitMode::Single {
            div()
                .id("single-scroll")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .p_3()
                .child(inputs_for_single(self))
                .child(div().mt_3().child(self.output(palette, cx)))
                .into_any_element()
        } else {
            body.into_any_element()
        };
        let (status, error) = match &self.runtime.state {
            ExecutionState::Idle => ("就绪".to_owned(), None),
            ExecutionState::Pending => ("等待编辑完成".into(), None),
            ExecutionState::Running
                if self
                    .started
                    .is_some_and(|start| start.elapsed() >= Duration::from_millis(150)) =>
            {
                let progress = *self.inputs.progress.lock().expect("progress lock");
                (
                    progress
                        .map(|p| {
                            format!(
                                "运行中… {:.1}% · {} 字节 · 可取消",
                                p.fraction() * 100.,
                                p.processed
                            )
                        })
                        .unwrap_or_else(|| "运行中… 可取消".into()),
                    None,
                )
            }
            ExecutionState::Running => ("就绪".into(), None),
            ExecutionState::Valid => (
                self.result
                    .as_ref()
                    .map(|r| r.footer.clone())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "完成".into()),
                None,
            ),
            ExecutionState::Manual => ("大文本：请点击运行".into(), None),
            ExecutionState::Cancelled => ("已取消".into(), None),
            ExecutionState::Stale(error) => ("Stale · 保留上次有效输出".into(), Some(error)),
            ExecutionState::Failed(error) => ("执行失败".into(), Some(error)),
        };
        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(palette.bg))
            .text_color(rgb(palette.text))
            .child(self.header(palette, cx))
            .child(controls);
        if let Some(error) = error {
            let location = error
                .span
                .map(|s| format!(" · 第 {} 行，第 {} 列", s.line, s.column))
                .unwrap_or_default();
            root = root.child(
                div()
                    .px_3()
                    .py_2()
                    .text_xs()
                    .text_color(rgb(palette.danger))
                    .child(format!(
                        "{}: {}{} {}",
                        error.kind,
                        error.message,
                        location,
                        error.hint.as_deref().unwrap_or("")
                    )),
            );
        }
        root.on_drop(cx.listener(|this, paths: &ExternalPaths, _, cx| {
            let payload = RawPayload::Files(paths.paths().to_vec());
            this.inputs.payload = Some(payload.clone());
            this.session_input = Some(payload.clone());
            this.context.session.update(cx, |session, cx| {
                session.input = Some(payload);
                cx.notify();
            });
            this.schedule(true, true, cx);
        }))
        .child(body)
        .child(
            div()
                .h(px(26.))
                .px_3()
                .text_xs()
                .border_t_1()
                .border_color(rgb(palette.border))
                .text_color(rgb(palette.muted))
                .child(status),
        )
    }
}

fn inputs_for_single(view: &ToolView) -> AnyElement {
    match &view.model {
        Model::Standard(_) => view.field(Slot::Text, "输入"),
        Model::Converter(options) => options.inputs(view),
        Model::Debug(options) => options.inputs(view),
    }
}

#[cfg(test)]
mod tests {
    use super::{Debounce, ExecutionState, ToolRuntime, debounce};
    use devtools_core::{ToolError, ToolErrorKind};
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn debounce_obeys_boundaries_and_explicit_override() {
        for size in [0, 1, 65_536] {
            assert_eq!(
                debounce(size, false),
                Debounce::After(Duration::from_millis(100))
            );
        }
        for size in [65_537, 1_048_576] {
            assert_eq!(
                debounce(size, false),
                Debounce::After(Duration::from_millis(250))
            );
        }
        assert_eq!(debounce(1_048_577, false), Debounce::Manual);
        for size in [0, 65_537, usize::MAX] {
            assert_eq!(debounce(size, true), Debounce::After(Duration::ZERO));
        }
    }

    #[test]
    fn syntax_error_retains_last_valid_result_but_disables_copy() {
        let mut state = ToolRuntime::default();
        let (first, _) = state.invalidate();
        assert!(state.complete(first, Ok(Arc::from("valid"))));
        assert!(state.can_copy());
        let (next, _) = state.invalidate();
        let error = ToolError::new(ToolErrorKind::InvalidSyntax, "unfinished");
        assert!(state.complete(next, Err(error.clone())));
        assert_eq!(state.state, ExecutionState::Stale(error));
        assert_eq!(state.output.as_deref(), Some("valid"));
        assert!(!state.can_copy());
        let (next, _) = state.invalidate();
        state.complete(next, Ok(Arc::from("recovered")));
        assert!(state.can_copy());
    }

    #[test]
    fn generations_cancel_and_discard_out_of_order_results() {
        let mut state = ToolRuntime::default();
        let (old, old_cancel) = state.invalidate();
        let (new, _) = state.invalidate();
        assert!(old_cancel.is_cancelled());
        assert!(!state.complete(old, Ok(Arc::from("old"))));
        assert!(state.complete(new, Ok(Arc::from("new"))));
        state.cancel();
        assert!(!state.complete(new, Ok(Arc::from("after cancel"))));
    }

    #[test]
    fn non_syntax_errors_clear_old_results_and_initial_errors_are_not_stale() {
        let mut state = ToolRuntime::default();
        let (generation, _) = state.invalidate();
        state.complete(
            generation,
            Err(ToolError::new(ToolErrorKind::InvalidSyntax, "bad")),
        );
        assert!(matches!(state.state, ExecutionState::Failed(_)));
        let (generation, _) = state.invalidate();
        state.complete(generation, Ok(Arc::from("valid")));
        let (generation, _) = state.invalidate();
        state.complete(
            generation,
            Err(ToolError::new(ToolErrorKind::SizeLimitExceeded, "limit")),
        );
        assert!(state.output.is_none());
    }
}
