use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use devtools_core::{CancelFlag, RawPayload, ToolError, ToolErrorKind, ToolId};
use gpui_kit::component::color_picker::{ColorPicker, ColorPickerEvent, ColorPickerState};
use gpui_kit::component::input::{Editor, EditorState, Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement;
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;

use super::Model;
use crate::icons::{IconName, icon_el, tool_icon};
use crate::labels::{tool_hint, tool_name};
use crate::syntax::{SyntaxSpec, resolve_syntax};
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

/// Tools that produce meaningful default output immediately on open even when
/// no input text has been entered.
pub fn is_auto_seed_tool(tool: ToolId) -> bool {
    matches!(
        tool,
        ToolId::UuidUlidGenerator | ToolId::PasswordTokenGenerator | ToolId::ChmodCalculator
    )
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

    pub fn reset_idle(&mut self) {
        self.cancel.cancel();
        self.generation = self
            .generation
            .checked_add(1)
            .expect("tool generation exhausted");
        self.cancel = CancelFlag::new();
        self.state = ExecutionState::Idle;
        self.output = None;
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

    pub fn has_non_text_payload(&self) -> bool {
        match &self.payload {
            Some(RawPayload::Files(paths)) => !paths.is_empty(),
            Some(RawPayload::Image(image)) => !image.bytes.is_empty(),
            _ => false,
        }
    }

    pub fn is_effectively_empty(&self, tool: ToolId, model: &Model) -> bool {
        if self.has_non_text_payload() {
            return false;
        }
        match tool {
            ToolId::TextDiff | ToolId::ListCompare => {
                self.text.is_empty() && self.second.is_empty()
            }
            ToolId::JsonDiff => self.text.trim().is_empty() && self.second.trim().is_empty(),
            ToolId::RegexTester => self.text.is_empty() && self.query.is_empty(),
            ToolId::Base64Codec => self.text.is_empty() && self.query.trim().is_empty(),
            ToolId::QrCodeGenerator => {
                if matches!(
                    model,
                    Model::Converter(super::converters_gen::ConverterOptions::Qr { mode: 3, .. })
                ) {
                    self.query.trim().is_empty()
                } else {
                    self.text.is_empty()
                }
            }
            ToolId::HtmlEntityUnicode
            | ToolId::StringEscape
            | ToolId::GzipCompress
            | ToolId::TextAnalysis
            | ToolId::HashHmac => self.text.is_empty(),
            _ => self.text.trim().is_empty(),
        }
    }
}

#[derive(Clone, Default)]
pub(super) struct WcagSummary {
    pub label: &'static str,
    pub bg_hex: u32,
    pub ratio: f64,
    pub aa: bool,
    pub aaa: bool,
}

#[derive(Clone, Default)]
pub(super) struct ToolResult {
    pub output: String,
    pub rows: Vec<(String, String)>,
    pub table: Option<(Vec<String>, Vec<Vec<String>>)>,
    /// `(line_text, Some(true)=added, Some(false)=removed, None=context)`
    pub diff_lines: Vec<(String, Option<bool>)>,
    pub swatch: Option<u32>,
    pub wcag: Vec<WcagSummary>,
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
    text_lang: &'static str,
    second_lang: &'static str,
    output_lang: &'static str,
    query: Entity<InputState>,
    replacement: Entity<InputState>,
    secret: Entity<InputState>,
    expected: Entity<InputState>,
    color: Entity<ColorPickerState>,
    _subscriptions: Vec<Subscription>,
}

fn is_pure_code_result(result: &ToolResult) -> bool {
    result.rows.is_empty()
        && result.table.is_none()
        && result.diff_lines.is_empty()
        && result.swatch.is_none()
        && result.strength.is_none()
        && result.image.is_none()
        && result.highlights.is_empty()
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
            if !text.is_empty() {
                match &mut self.model {
                    Model::Standard(options) => options.inject(text, launch.as_ref()),
                    Model::Converter(options) => options.inject(text, launch.as_ref()),
                    Model::Debug(options) => options.inject(text),
                }
            }
        }
    }

    pub fn load_sample(&mut self, cx: &mut Context<Self>) {
        let sample = crate::samples::sample_for(self.context.tool_id);
        self.inputs.text = sample.text;
        self.inputs.second = sample.second;
        self.inputs.query = sample.query;
        self.inputs.replacement = sample.replacement;
        self.inputs.secret = sample.secret;
        self.inputs.expected = sample.expected;
        self.inputs.payload = Some(RawPayload::Text(self.inputs.text.clone().into()));

        // Ensure QR sample generates a QR code rather than attempting file decode.
        if let Model::Converter(super::converters_gen::ConverterOptions::Qr { mode, .. }) =
            &mut self.model
            && *mode == 3
        {
            *mode = 0;
        }
        // Ensure JWT sample stays in Decode or Verify mode rather than Sign (which expects JSON).
        if let Model::Debug(super::text_debug_sec::DebugOptions::Jwt { mode, .. }) = &mut self.model
            && *mode == 2
        {
            *mode = 1;
        }

        self.publish_input(cx);
        self.schedule(true, true, cx);
    }

    pub fn clear_inputs(&mut self, cx: &mut Context<Self>) {
        self.inputs.text.clear();
        self.inputs.second.clear();
        self.inputs.query.clear();
        self.inputs.replacement.clear();
        self.inputs.secret.clear();
        self.inputs.expected.clear();
        self.inputs.payload = None;
        self.publish_input(cx);
        self.schedule(true, false, cx);
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
            _ if text.is_empty() => None,
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

    pub(super) fn trigger_option_change(&mut self, immediate: bool, cx: &mut Context<Self>) {
        self.publish_input(cx);
        self.schedule(immediate, immediate, cx);
    }

    fn schedule(&mut self, immediate: bool, manual: bool, cx: &mut Context<Self>) {
        self.needs_run = false;

        // Quiet Idle guardrail: non-generator tools with empty inputs stay in calm Idle
        // instead of throwing red EOF / syntax errors on startup or after clearing.
        if !is_auto_seed_tool(self.context.tool_id)
            && self
                .inputs
                .is_effectively_empty(self.context.tool_id, &self.model)
        {
            self.runtime.reset_idle();
            self.result = None;
            self.task = None;
            self.started = None;
            self.inputs.progress = Arc::new(Mutex::new(None));
            self.context.session.update(cx, |session, cx| {
                session.output = None;
                cx.notify();
            });
            cx.notify();
            return;
        }

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

    fn syntax_specs(&self) -> (SyntaxSpec, SyntaxSpec, SyntaxSpec) {
        let langs = self.model.syntax_languages();
        let output = self.runtime.output.as_deref().unwrap_or_default();
        (
            resolve_syntax(langs.text, &self.inputs.text),
            resolve_syntax(langs.second, &self.inputs.second),
            resolve_syntax(langs.output, output),
        )
    }

    fn ensure_editors(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (text_spec, second_spec, output_spec) = self.syntax_specs();
        if self.editors.is_none() {
            let is_sql = self.context.tool_id == ToolId::SqlFormatter;
            let text = cx.new(|cx| {
                let mut state = EditorState::new(window, cx).language(text_spec.highlighter);
                if is_sql {
                    state.set_highlighter_factory(crate::syntax::sql_highlighter_factory(), cx);
                }
                state
            });
            let second =
                cx.new(|cx| EditorState::new(window, cx).language(second_spec.highlighter));
            let output = cx.new(|cx| {
                let mut state = EditorState::new(window, cx).language(output_spec.highlighter);
                if is_sql {
                    state.set_highlighter_factory(crate::syntax::sql_highlighter_factory(), cx);
                }
                state
            });
            let query = cx.new(|cx| InputState::new(window, cx));
            let replacement = cx.new(|cx| InputState::new(window, cx));
            let secret = cx.new(|cx| InputState::new(window, cx).masked(true));
            let expected = cx.new(|cx| InputState::new(window, cx));
            let color = cx.new(|cx| ColorPickerState::new(window, cx).default_value(rgb(0x3b82f6)));
            let mut subscriptions = vec![cx.subscribe(&color, |this, _, event, cx| {
                if let ColorPickerEvent::Change(Some(color)) = event {
                    let color = color.to_rgb();
                    let channel = |value: f32| (value * 255.).round() as u8;
                    let a = channel(color.a);
                    this.inputs.text = if a == 255 {
                        format!(
                            "#{:02x}{:02x}{:02x}",
                            channel(color.r),
                            channel(color.g),
                            channel(color.b)
                        )
                    } else {
                        format!(
                            "#{:02x}{:02x}{:02x}{:02x}",
                            channel(color.r),
                            channel(color.g),
                            channel(color.b),
                            a
                        )
                    };
                    this.inputs.payload = Some(RawPayload::Text(this.inputs.text.clone().into()));
                    this.changed(cx);
                }
            })];
            for (slot, state) in [(Slot::Text, &text), (Slot::Second, &second)] {
                subscriptions.push(cx.subscribe(state, move |this, state, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = state.read(cx).value().to_string();
                        if matches!(slot, Slot::Text) {
                            this.inputs.payload = if value.is_empty() {
                                None
                            } else {
                                Some(RawPayload::Text(value.clone().into()))
                            };
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
                text_lang: text_spec.highlighter,
                second_lang: second_spec.highlighter,
                output_lang: output_spec.highlighter,
                query,
                replacement,
                secret,
                expected,
                color,
                _subscriptions: subscriptions,
            });
        }
        let editors = self.editors.as_mut().expect("initialized editors");
        if editors.text_lang != text_spec.highlighter {
            editors.text_lang = text_spec.highlighter;
            editors.text.update(cx, |state, cx| {
                state.set_highlighter(text_spec.highlighter, cx);
            });
        }
        if editors.second_lang != second_spec.highlighter {
            editors.second_lang = second_spec.highlighter;
            editors.second.update(cx, |state, cx| {
                state.set_highlighter(second_spec.highlighter, cx);
            });
        }
        if editors.output_lang != output_spec.highlighter {
            editors.output_lang = output_spec.highlighter;
            editors.output.update(cx, |state, cx| {
                state.set_highlighter(output_spec.highlighter, cx);
            });
        }
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
            .label("拾色器")
            .into_any_element()
    }

    /// Full-height code editor slot meant to stretch inside a `flex_1 min_h_0` container.
    pub fn full_editor(&self, slot: Slot) -> AnyElement {
        let editors = self.editors.as_ref().expect("editors mounted");
        let state = match slot {
            Slot::Text => &editors.text,
            Slot::Second => &editors.second,
            _ => unreachable!("full_editor only supports multiline editor slots"),
        };
        div()
            .flex_1()
            .min_h(px(140.0))
            .min_w_0()
            .size_full()
            .child(
                Editor::new(state)
                    .appearance(false)
                    .bordered(false)
                    .size_full(),
            )
            .into_any_element()
    }

    /// Compact single-line input bar used above/below the main code editor.
    pub fn inline_field(&self, slot: Slot, label: &str, palette: Palette) -> AnyElement {
        let editors = self.editors.as_ref().expect("editors mounted");
        let input = match slot {
            Slot::Query => Input::new(&editors.query),
            Slot::Replacement => Input::new(&editors.replacement),
            Slot::Secret => Input::new(&editors.secret).mask_toggle(),
            Slot::Expected => Input::new(&editors.expected),
            _ => unreachable!("inline_field only supports single-line Input slots"),
        };
        div()
            .flex_shrink_0()
            .flex()
            .flex_col()
            .gap_1()
            .px_3()
            .py_2()
            .bg(rgb(palette.panel_alt))
            .border_b_1()
            .border_color(rgb(palette.border))
            .child(
                div()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(palette.muted))
                    .child(label.to_owned()),
            )
            .child(input)
            .into_any_element()
    }

    /// Unified segmented option switcher (eliminates awkward "X (关闭为 Y)" toggles).
    pub fn segmented(
        &self,
        group_id: &str,
        labels: &[&str],
        selected_index: usize,
        palette: Palette,
        cx: &Context<Self>,
        on_select: impl Fn(&mut Self, usize) + 'static,
    ) -> AnyElement {
        let on_select = Arc::new(on_select);
        let mut track = div()
            .flex()
            .items_center()
            .gap_0p5()
            .p(px(2.0))
            .rounded_md()
            .bg(rgb(palette.chrome))
            .border_1()
            .border_color(rgb(palette.border));

        for (index, &label) in labels.iter().enumerate() {
            let active = index == selected_index;
            let on_select = on_select.clone();
            track = track.child(
                div()
                    .id(format!("seg-{group_id}-{index}"))
                    .px_2()
                    .py_0p5()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_size(px(11.5))
                    .font_weight(if active {
                        FontWeight::MEDIUM
                    } else {
                        FontWeight::NORMAL
                    })
                    .bg(rgb(if active {
                        palette.panel
                    } else {
                        palette.chrome
                    }))
                    .when(active, |el| {
                        el.border_1().border_color(rgb(palette.border_strong))
                    })
                    .hover(move |s| s.bg(rgb(if active { palette.panel } else { palette.hover })))
                    .text_color(rgb(if active {
                        palette.accent
                    } else {
                        palette.muted
                    }))
                    .child(label.to_owned())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        on_select(this, index);
                        this.trigger_option_change(false, cx);
                    })),
            );
        }
        track.into_any_element()
    }

    /// Check-pill for genuine independent boolean options.
    pub fn toggle_pill(
        &self,
        id: &str,
        label: impl Into<SharedString>,
        active: bool,
        palette: Palette,
        cx: &Context<Self>,
        change: impl Fn(&mut Self) + 'static,
    ) -> AnyElement {
        div()
            .id(id.to_owned())
            .h(px(25.0))
            .px_2p5()
            .rounded_md()
            .cursor_pointer()
            .flex()
            .items_center()
            .gap_1()
            .border_1()
            .border_color(rgb(if active {
                palette.accent
            } else {
                palette.border
            }))
            .bg(rgb(if active {
                palette.accent_soft
            } else {
                palette.panel
            }))
            .hover(move |s| {
                s.bg(rgb(if active {
                    palette.accent_soft
                } else {
                    palette.hover
                }))
            })
            .text_size(px(11.5))
            .font_weight(if active {
                FontWeight::MEDIUM
            } else {
                FontWeight::NORMAL
            })
            .text_color(rgb(if active {
                palette.accent
            } else {
                palette.muted
            }))
            .when(active, |el| {
                el.child(icon_el(IconName::Check, 11.0, palette.accent))
            })
            .child(label.into())
            .on_click(cx.listener(move |this, _, _, cx| {
                change(this);
                this.trigger_option_change(false, cx);
            }))
            .into_any_element()
    }

    /// Immediate action pill (e.g. Regenerate, Now, Presets).
    pub fn action_pill(
        &self,
        id: &str,
        icon: Option<IconName>,
        label: impl Into<SharedString>,
        palette: Palette,
        cx: &Context<Self>,
        action: impl Fn(&mut Self) + 'static,
    ) -> AnyElement {
        div()
            .id(id.to_owned())
            .h(px(25.0))
            .px_2p5()
            .rounded_md()
            .cursor_pointer()
            .flex()
            .items_center()
            .gap_1()
            .border_1()
            .border_color(rgb(palette.border))
            .bg(rgb(palette.panel))
            .hover(move |s| {
                s.bg(rgb(palette.hover))
                    .border_color(rgb(palette.border_strong))
            })
            .text_size(px(11.5))
            .text_color(rgb(palette.text))
            .when_some(icon, |el, ic| el.child(icon_el(ic, 11.5, palette.accent)))
            .child(label.into())
            .on_click(cx.listener(move |this, _, _, cx| {
                action(this);
                this.trigger_option_change(true, cx);
            }))
            .into_any_element()
    }

    pub fn copy_row(
        &self,
        id: usize,
        key: &str,
        value: &str,
        mono_size: f32,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let value_owned = value.to_owned();
        let copy = value_owned.clone();
        let enabled = self.runtime.can_copy();
        div()
            .id(format!("result-row-{id}"))
            .flex()
            .items_start()
            .gap_3()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(palette.panel_alt))
            .border_1()
            .border_color(rgb(palette.border))
            .hover(move |s| {
                s.bg(rgb(palette.hover))
                    .border_color(rgb(palette.border_strong))
            })
            .cursor_pointer()
            .child(
                div()
                    .w(px(136.0))
                    .flex_shrink_0()
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(palette.muted))
                    .child(key.to_owned()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(mono_size))
                    .text_color(rgb(palette.text))
                    .child(value_owned),
            )
            .child(div().flex_shrink_0().pt_0p5().child(icon_el(
                IconName::Copy,
                12.0,
                palette.subtle,
            )))
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

        let mut split_track = div()
            .flex()
            .items_center()
            .gap_0p5()
            .p(px(2.0))
            .rounded_md()
            .bg(rgb(palette.chrome))
            .border_1()
            .border_color(rgb(palette.border));

        for (index, (mode, icon, label)) in [
            (SplitMode::SideBySide, IconName::Columns2, "左右"),
            (SplitMode::Stacked, IconName::Rows2, "上下"),
            (SplitMode::Single, IconName::Square, "单栏"),
        ]
        .into_iter()
        .enumerate()
        {
            let active = self.split == mode;
            split_track = split_track.child(
                div()
                    .id(format!("split-{index}"))
                    .h(px(24.0))
                    .px_2()
                    .rounded_sm()
                    .flex()
                    .items_center()
                    .gap_1()
                    .text_size(px(11.5))
                    .cursor_pointer()
                    .bg(rgb(if active {
                        palette.panel
                    } else {
                        palette.chrome
                    }))
                    .when(active, |el| {
                        el.border_1()
                            .border_color(rgb(palette.border_strong))
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .hover(move |s| s.bg(rgb(if active { palette.panel } else { palette.hover })))
                    .text_color(rgb(if active {
                        palette.accent
                    } else {
                        palette.muted
                    }))
                    .child(icon_el(
                        icon,
                        12.0,
                        if active {
                            palette.accent
                        } else {
                            palette.muted
                        },
                    ))
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.split = mode;
                        cx.notify();
                    })),
            );
        }

        let mut actions = div()
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap_1p5()
            .child(split_track);

        if self.runtime.state == ExecutionState::Running {
            actions = actions.child(
                div()
                    .id("cancel")
                    .h(px(28.0))
                    .px_2p5()
                    .rounded_md()
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap_1()
                    .bg(rgb(palette.danger_soft))
                    .border_1()
                    .border_color(rgb(palette.danger))
                    .text_xs()
                    .text_color(rgb(palette.danger))
                    .child(icon_el(IconName::CircleX, 12.5, palette.danger))
                    .child("取消")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.runtime.cancel();
                        this.task = None;
                        cx.notify();
                    })),
            );
        } else if self.runtime.state == ExecutionState::Manual {
            actions = actions.child(
                div()
                    .id("run")
                    .h(px(28.0))
                    .px_2p5()
                    .rounded_md()
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap_1()
                    .bg(rgb(palette.accent_soft))
                    .border_1()
                    .border_color(rgb(palette.accent))
                    .text_xs()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(palette.accent))
                    .child(icon_el(IconName::Play, 12.0, palette.accent))
                    .child("运行")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.schedule(true, true, cx);
                    })),
            );
        }

        // Favorite button
        actions = actions.child(
            div()
                .id("favorite")
                .h(px(28.0))
                .w(px(28.0))
                .rounded_md()
                .cursor_pointer()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(palette.panel))
                .border_1()
                .border_color(rgb(palette.border))
                .hover(move |s| s.bg(rgb(palette.hover)))
                .child(icon_el(
                    if favorite {
                        IconName::StarFill
                    } else {
                        IconName::Star
                    },
                    13.5,
                    if favorite {
                        palette.warning
                    } else {
                        palette.muted
                    },
                ))
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(wb) = this.workbench.upgrade() {
                        wb.update(cx, |wb, cx| wb.toggle_favorite(this.context.tool_id, cx));
                    }
                })),
        );

        // Multi-open button
        actions = actions.child(
            div()
                .id("new")
                .h(px(28.0))
                .px_2p5()
                .rounded_md()
                .cursor_pointer()
                .flex()
                .items_center()
                .gap_1()
                .bg(rgb(palette.panel))
                .border_1()
                .border_color(rgb(palette.border))
                .hover(move |s| s.bg(rgb(palette.hover)))
                .text_xs()
                .text_color(rgb(palette.text))
                .child(icon_el(IconName::Plus, 12.5, palette.muted))
                .child("多开")
                .on_click(cx.listener(|this, _, _, cx| {
                    if let Some(wb) = this.workbench.upgrade() {
                        wb.update(cx, |wb, cx| wb.open_tool(this.context.tool_id, cx));
                    }
                })),
        );

        let can_copy = self.runtime.can_copy();

        // Relay ("发送到 ▸")
        actions = actions.child(
            div()
                .id("relay")
                .h(px(28.0))
                .px_2p5()
                .rounded_md()
                .flex()
                .items_center()
                .gap_1()
                .bg(rgb(palette.panel))
                .border_1()
                .border_color(rgb(palette.border))
                .when(can_copy, |el| {
                    el.cursor_pointer()
                        .hover(move |s| s.bg(rgb(palette.hover)))
                        .text_color(rgb(palette.text))
                })
                .when(!can_copy, |el| {
                    el.opacity(0.45).text_color(rgb(palette.muted))
                })
                .text_xs()
                .child(icon_el(
                    IconName::Send,
                    12.0,
                    if can_copy {
                        palette.accent
                    } else {
                        palette.muted
                    },
                ))
                .child("发送到 ▸")
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !can_copy {
                        return;
                    }
                    if let Some(wb) = this.workbench.upgrade() {
                        wb.update(cx, |wb, cx| wb.open_relay(this.context.tab_id, cx));
                    }
                })),
        );

        // Single primary high-emphasis CTA: "复制并收起"
        actions = actions.child(
            div()
                .id("dismiss")
                .h(px(28.0))
                .px_3()
                .rounded_md()
                .flex()
                .items_center()
                .gap_1p5()
                .text_xs()
                .font_weight(FontWeight::MEDIUM)
                .when(can_copy, |el| {
                    el.cursor_pointer()
                        .bg(rgb(palette.accent))
                        .hover(move |s| s.bg(rgb(palette.accent_hover)))
                        .text_color(rgb(0xffffff))
                })
                .when(!can_copy, |el| {
                    el.bg(rgb(palette.panel_alt))
                        .border_1()
                        .border_color(rgb(palette.border))
                        .text_color(rgb(palette.subtle))
                })
                .child(icon_el(
                    IconName::ClipboardCheck,
                    13.0,
                    if can_copy { 0xffffff } else { palette.subtle },
                ))
                .child("复制并收起")
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !can_copy {
                        return;
                    }
                    if let Some(wb) = this.workbench.upgrade() {
                        wb.update(cx, |wb, cx| wb.copy_output(this.context.tab_id, true, cx));
                    }
                })),
        );

        div()
            .h(px(44.0))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .gap_3()
            .px_3p5()
            .bg(rgb(palette.panel))
            .border_b_1()
            .border_color(rgb(palette.border))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .w(px(26.0))
                            .h(px(26.0))
                            .flex_shrink_0()
                            .rounded_md()
                            .bg(rgb(palette.accent_soft))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon_el(
                                tool_icon(self.context.tool_id),
                                14.0,
                                palette.accent,
                            )),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(13.5))
                            .truncate()
                            .child(tool_name(self.context.tool_id)),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .px_1p5()
                            .py_0p5()
                            .rounded_sm()
                            .bg(rgb(palette.accent_soft))
                            .text_size(px(10.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(palette.accent))
                            .child(format!("#{}", self.context.instance)),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.5))
                            .text_color(rgb(palette.muted))
                            .child(tool_hint(self.context.tool_id)),
                    ),
            )
            .child(actions)
            .into_any_element()
    }

    fn render_input_card(&self, palette: Palette, cx: &Context<Self>) -> AnyElement {
        let has_any_input = self.inputs.bytes() > 0 || self.inputs.has_non_text_payload();
        let (text_spec, second_spec, _) = self.syntax_specs();
        let input_degraded = text_spec.degraded || second_spec.degraded;
        let stats_label = if self.inputs.text.is_empty() && self.inputs.second.is_empty() {
            "空".to_owned()
        } else {
            let chars = self.inputs.text.chars().count() + self.inputs.second.chars().count();
            let lines = self.inputs.text.lines().count().max(1);
            format!("{chars} 字符 · {lines} 行")
        };

        let header = div()
            .h(px(32.0))
            .flex_shrink_0()
            .px_3()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .bg(rgb(palette.chrome))
            .border_b_1()
            .border_color(rgb(palette.border))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .text_xs()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(palette.text))
                            .child(input_pane_title(self.context.tool_id)),
                    )
                    .when_some(text_spec.badge, |el, badge| {
                        el.child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(rgb(palette.accent_soft))
                                .text_size(px(10.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(palette.accent))
                                .child(badge),
                        )
                    })
                    .when(input_degraded, |el| {
                        el.child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(rgb(palette.warning_soft))
                                .text_size(px(10.0))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(rgb(palette.warning))
                                .child("大文本 · 纯文本模式"),
                        )
                    })
                    .child(
                        div()
                            .text_size(px(11.0))
                            .text_color(rgb(palette.subtle))
                            .child(stats_label),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .id("input-load-sample")
                            .h(px(22.0))
                            .px_2()
                            .rounded_sm()
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap_1()
                            .hover(move |s| s.bg(rgb(palette.hover)))
                            .text_size(px(11.0))
                            .text_color(rgb(palette.accent))
                            .child(icon_el(IconName::FlaskConical, 11.5, palette.accent))
                            .child("示例")
                            .on_click(cx.listener(|this, _, _, cx| this.load_sample(cx))),
                    )
                    .when(has_any_input, |el| {
                        el.child(
                            div()
                                .id("input-clear")
                                .h(px(22.0))
                                .px_2()
                                .rounded_sm()
                                .cursor_pointer()
                                .flex()
                                .items_center()
                                .gap_1()
                                .hover(move |s| s.bg(rgb(palette.hover)))
                                .text_size(px(11.0))
                                .text_color(rgb(palette.muted))
                                .child(icon_el(IconName::Eraser, 11.5, palette.muted))
                                .child("清空")
                                .on_click(cx.listener(|this, _, _, cx| this.clear_inputs(cx))),
                        )
                    }),
            );

        let mut content = div()
            .flex_1()
            .min_h_0()
            .min_w_0()
            .flex()
            .flex_col()
            .overflow_hidden();

        match &self.inputs.payload {
            Some(RawPayload::Files(paths)) if !paths.is_empty() => {
                content = content.child(
                    div()
                        .flex_shrink_0()
                        .px_3()
                        .py_1p5()
                        .bg(rgb(palette.accent_soft))
                        .border_b_1()
                        .border_color(rgb(palette.border))
                        .text_xs()
                        .text_color(rgb(palette.accent))
                        .truncate()
                        .child(format!(
                            "已载入文件：{}",
                            paths
                                .iter()
                                .map(|p| p.display().to_string())
                                .collect::<Vec<_>>()
                                .join(" · ")
                        )),
                );
            }
            Some(RawPayload::Image(image)) if !image.bytes.is_empty() => {
                content = content.child(
                    div()
                        .flex_shrink_0()
                        .px_3()
                        .py_1p5()
                        .bg(rgb(palette.accent_soft))
                        .border_b_1()
                        .border_color(rgb(palette.border))
                        .text_xs()
                        .text_color(rgb(palette.accent))
                        .child(format!(
                            "已载入图片：{} × {} · {} 字节",
                            image.width,
                            image.height,
                            image.bytes.len()
                        )),
                );
            }
            _ => (),
        }

        let fields = match &self.model {
            Model::Standard(_) => self.full_editor(Slot::Text),
            Model::Converter(options) => options.inputs(self, palette, cx),
            Model::Debug(options) => options.inputs(self, palette, cx),
        };

        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .size_full()
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(rgb(palette.border))
            .bg(rgb(palette.panel))
            .overflow_hidden()
            .child(header)
            .child(content.child(fields))
            .into_any_element()
    }

    fn render_output_card(
        &self,
        mono_size: f32,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let sensitive = self.context.tool_id.default_capabilities().sensitive_output;
        let is_stale = matches!(self.runtime.state, ExecutionState::Stale(_));
        let can_copy = self.runtime.can_copy();
        let (_, _, output_spec) = self.syntax_specs();
        let show_output_syntax = self.result.as_ref().is_none_or(is_pure_code_result);

        let output_stats = self.result.as_ref().map(|r| {
            if !r.rows.is_empty() {
                format!("{} 项字段", r.rows.len())
            } else if let Some((_, rows)) = &r.table {
                format!("{} 行表格", rows.len())
            } else {
                let bytes = r.output.len();
                let lines = r
                    .output
                    .lines()
                    .count()
                    .max(usize::from(!r.output.is_empty()));
                format!("{lines} 行 · {bytes} B")
            }
        });

        let mut left_badges = div()
            .flex()
            .items_center()
            .gap_1p5()
            .min_w_0()
            .child(
                div()
                    .text_xs()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(palette.text))
                    .child("输出"),
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
                    .child("只读"),
            );

        if show_output_syntax && let Some(badge) = output_spec.badge {
            left_badges = left_badges.child(
                div()
                    .px_1p5()
                    .py_0p5()
                    .rounded_sm()
                    .bg(rgb(palette.accent_soft))
                    .text_size(px(10.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(palette.accent))
                    .child(badge),
            );
            if output_spec.degraded {
                left_badges = left_badges.child(
                    div()
                        .px_1p5()
                        .py_0p5()
                        .rounded_sm()
                        .bg(rgb(palette.warning_soft))
                        .text_size(px(10.0))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(palette.warning))
                        .child("大文本 · 纯文本模式"),
                );
            }
        }

        if sensitive {
            left_badges = left_badges.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .px_1p5()
                    .py_0p5()
                    .rounded_sm()
                    .bg(rgb(palette.warning_soft))
                    .text_size(px(10.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(palette.warning))
                    .child(icon_el(IconName::Lock, 10.0, palette.warning))
                    .child("敏感输出 · 排除历史"),
            );
        }

        if is_stale {
            left_badges = left_badges.child(
                div()
                    .px_1p5()
                    .py_0p5()
                    .rounded_sm()
                    .bg(rgb(palette.warning_soft))
                    .text_size(px(10.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(palette.warning))
                    .child("Stale · 上次有效结果"),
            );
        }

        let header = div()
            .h(px(32.0))
            .flex_shrink_0()
            .px_3()
            .flex()
            .items_center()
            .justify_between()
            .gap_2()
            .bg(rgb(palette.chrome))
            .border_b_1()
            .border_color(rgb(palette.border))
            .child(left_badges)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .when_some(output_stats, |el, stats| {
                        el.child(
                            div()
                                .text_size(px(11.0))
                                .text_color(rgb(palette.subtle))
                                .child(stats),
                        )
                    })
                    .child(
                        div()
                            .id("output-copy")
                            .h(px(22.0))
                            .px_2()
                            .rounded_sm()
                            .flex()
                            .items_center()
                            .gap_1()
                            .text_size(px(11.0))
                            .when(can_copy, |el| {
                                el.cursor_pointer()
                                    .hover(move |s| s.bg(rgb(palette.hover)))
                                    .text_color(rgb(palette.accent))
                            })
                            .when(!can_copy, |el| {
                                el.opacity(0.45).text_color(rgb(palette.subtle))
                            })
                            .child(icon_el(
                                IconName::Copy,
                                11.5,
                                if can_copy {
                                    palette.accent
                                } else {
                                    palette.subtle
                                },
                            ))
                            .child("复制")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !can_copy {
                                    return;
                                }
                                if let Some(wb) = this.workbench.upgrade() {
                                    wb.update(cx, |wb, cx| {
                                        wb.copy_output(this.context.tab_id, false, cx)
                                    });
                                }
                            })),
                    ),
            );

        let body = self.render_output_body(mono_size, palette, cx);

        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .size_full()
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(rgb(palette.border))
            .bg(rgb(palette.panel))
            .overflow_hidden()
            .child(header)
            .child(body)
            .into_any_element()
    }

    fn render_output_body(
        &self,
        mono_size: f32,
        palette: Palette,
        cx: &Context<Self>,
    ) -> AnyElement {
        let Some(result) = &self.result else {
            if let ExecutionState::Failed(err) = &self.runtime.state {
                let loc = err
                    .span
                    .map(|s| format!("第 {} 行，第 {} 列", s.line, s.column));
                return div()
                    .flex_1()
                    .min_h_0()
                    .size_full()
                    .p_6()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(
                        div()
                            .w(px(38.0))
                            .h(px(38.0))
                            .rounded_full()
                            .bg(rgb(palette.danger_soft))
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(icon_el(IconName::CircleAlert, 18.0, palette.danger)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(palette.danger))
                            .child("输入内容需要修正"),
                    )
                    .when_some(loc, |el, l| {
                        el.child(
                            div()
                                .px_2()
                                .py_0p5()
                                .rounded_sm()
                                .bg(rgb(palette.panel_alt))
                                .border_1()
                                .border_color(rgb(palette.border))
                                .text_xs()
                                .text_color(rgb(palette.muted))
                                .child(l),
                        )
                    })
                    .child(
                        div()
                            .max_w(px(440.0))
                            .text_center()
                            .text_xs()
                            .text_color(rgb(palette.muted))
                            .child(err.message.clone()),
                    )
                    .when_some(err.hint.clone(), |el, hint| {
                        el.child(
                            div()
                                .max_w(px(440.0))
                                .text_center()
                                .text_xs()
                                .text_color(rgb(palette.subtle))
                                .child(hint),
                        )
                    })
                    .child(
                        div()
                            .id("error-load-sample")
                            .mt_1()
                            .h(px(28.0))
                            .px_3()
                            .rounded_md()
                            .cursor_pointer()
                            .flex()
                            .items_center()
                            .gap_1p5()
                            .bg(rgb(palette.accent_soft))
                            .border_1()
                            .border_color(rgb(palette.accent))
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(palette.accent))
                            .child(icon_el(IconName::FlaskConical, 12.5, palette.accent))
                            .child("载入有效示例")
                            .on_click(cx.listener(|this, _, _, cx| this.load_sample(cx))),
                    )
                    .into_any_element();
            }

            // Friendly Idle / Empty State
            let paste_hint = if cfg!(target_os = "macos") {
                "⌘⇧V 智能粘贴"
            } else {
                "Ctrl+Shift+V 智能粘贴"
            };
            return div()
                .flex_1()
                .min_h_0()
                .size_full()
                .p_6()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap_2p5()
                .child(
                    div()
                        .w(px(42.0))
                        .h(px(42.0))
                        .rounded_xl()
                        .bg(rgb(palette.accent_soft))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(icon_el(
                            tool_icon(self.context.tool_id),
                            20.0,
                            palette.accent,
                        )),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(palette.text))
                        .child(tool_name(self.context.tool_id)),
                )
                .child(
                    div()
                        .max_w(px(380.0))
                        .text_center()
                        .text_xs()
                        .text_color(rgb(palette.muted))
                        .child(format!(
                            "在左侧输入数据、按 {paste_hint}，或一键载入真实示例体验"
                        )),
                )
                .child(
                    div()
                        .id("empty-load-sample")
                        .mt_1()
                        .h(px(30.0))
                        .px_3p5()
                        .rounded_md()
                        .cursor_pointer()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .bg(rgb(palette.accent_soft))
                        .border_1()
                        .border_color(rgb(palette.accent))
                        .hover(move |s| s.bg(rgb(palette.hover)))
                        .text_xs()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(palette.accent))
                        .child(icon_el(IconName::FlaskConical, 13.0, palette.accent))
                        .child("载入示例数据")
                        .on_click(cx.listener(|this, _, _, cx| this.load_sample(cx))),
                )
                .into_any_element();
        };

        if is_pure_code_result(result) {
            return div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .size_full()
                .child(
                    Editor::new(&self.editors.as_ref().expect("editors mounted").output)
                        .readonly(true)
                        .appearance(false)
                        .bordered(false)
                        .size_full(),
                )
                .into_any_element();
        }

        let mut out = div()
            .id("tool-output")
            .flex_1()
            .min_h_0()
            .min_w_0()
            .size_full()
            .flex()
            .flex_col()
            .gap_2p5()
            .p_3()
            .overflow_y_scrollbar()
            .text_size(px(mono_size));

        // Color swatch + WCAG contrast cards
        if let Some(color) = result.swatch {
            let mut contrast_cards = div().flex().gap_2p5().w_full();
            for item in &result.wcag {
                let fg_sample = if item.bg_hex == 0xffffff {
                    0x111827
                } else {
                    0xf9fafb
                };
                contrast_cards = contrast_cards.child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .p_2p5()
                        .rounded_md()
                        .bg(rgb(palette.panel_alt))
                        .border_1()
                        .border_color(rgb(palette.border))
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    div()
                                        .w(px(28.0))
                                        .h(px(28.0))
                                        .rounded_md()
                                        .bg(rgb(item.bg_hex))
                                        .border_1()
                                        .border_color(rgb(palette.border_strong))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .text_xs()
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(color))
                                        .child("Aa"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .child(
                                            div()
                                                .text_xs()
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(rgb(palette.text))
                                                .child(format!("{:.2} : 1", item.ratio)),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(10.5))
                                                .text_color(rgb(palette.muted))
                                                .child(format!("背景 {}", item.label)),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_1()
                                .child(wcag_badge("AA", item.aa, palette))
                                .child(wcag_badge("AAA", item.aaa, palette))
                                .map(|el| {
                                    let _ = fg_sample;
                                    el
                                }),
                        ),
                );
            }
            out = out
                .child(
                    div()
                        .h(px(68.0))
                        .w_full()
                        .rounded_lg()
                        .border_1()
                        .border_color(rgb(palette.border_strong))
                        .bg(rgb(color)),
                )
                .when(!result.wcag.is_empty(), |el| el.child(contrast_cards));
        }

        // Password strength bar
        if let Some(score) = result.strength {
            let (label, bar_color) = match score {
                0 | 1 => ("弱 · 建议增加长度与字符种类", palette.danger),
                2 => ("中等 · 可抵御基础字典猜测", palette.warning),
                3 => ("强 · 适合常规生产凭据", palette.accent),
                _ => ("极强 · 高抗暴力破解熵值", palette.success),
            };
            let mut segments = div().flex().gap_1p5().w_full();
            for step in 0..4u8 {
                let lit = score > step || (score == 0 && step == 0);
                segments =
                    segments.child(div().flex_1().h(px(6.0)).rounded_full().bg(rgb(if lit {
                        bar_color
                    } else {
                        palette.border
                    })));
            }
            out = out.child(
                div()
                    .p_2p5()
                    .rounded_md()
                    .bg(rgb(palette.panel_alt))
                    .border_1()
                    .border_color(rgb(palette.border))
                    .flex()
                    .flex_col()
                    .gap_1p5()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_xs()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(bar_color))
                                    .child(label),
                            )
                            .child(
                                div()
                                    .text_color(rgb(palette.muted))
                                    .child(format!("zxcvbn {score} / 4")),
                            ),
                    )
                    .child(segments),
            );
        }

        // Regex match highlights
        if !result.highlights.is_empty() {
            out = out.child(
                div()
                    .p_3()
                    .rounded_md()
                    .bg(rgb(palette.panel_alt))
                    .border_1()
                    .border_color(rgb(palette.border))
                    .child(
                        StyledText::new(result.highlighted_text.clone()).with_highlights(
                            result.highlights.iter().map(|(range, alternate)| {
                                (
                                    range.clone(),
                                    HighlightStyle {
                                        background_color: Some(
                                            rgb(if *alternate {
                                                palette.accent_soft
                                            } else {
                                                palette.warning_soft
                                            })
                                            .into(),
                                        ),
                                        color: Some(
                                            rgb(if *alternate {
                                                palette.accent
                                            } else {
                                                palette.warning
                                            })
                                            .into(),
                                        ),
                                        font_weight: Some(FontWeight::SEMIBOLD),
                                        ..HighlightStyle::default()
                                    },
                                )
                            }),
                        ),
                    ),
            );
        }

        // Image / QR preview
        if let Some(image) = &result.image {
            out = out.child(
                div()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(0xffffff))
                    .border_1()
                    .border_color(rgb(palette.border))
                    .self_center()
                    .child(
                        img(Arc::new(Image::from_bytes(
                            result.image_format.unwrap_or(ImageFormat::Png),
                            image.to_vec(),
                        )))
                        .w(px(220.0))
                        .h(px(220.0)),
                    ),
            );
        }

        // Virtualized DataTable
        if let Some((columns, rows)) = &result.table {
            let mut table = div()
                .flex_1()
                .min_h(px(240.0))
                .flex()
                .flex_col()
                .rounded_md()
                .border_1()
                .border_color(rgb(palette.border))
                .overflow_hidden()
                .text_xs();
            table = table.child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .bg(rgb(palette.chrome))
                    .border_b_1()
                    .border_color(rgb(palette.border))
                    .children(columns.iter().enumerate().map(|(index, name)| {
                        div()
                            .id(format!("sort-{index}"))
                            .flex_1()
                            .min_w(px(100.0))
                            .px_2p5()
                            .py_2()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(palette.text))
                            .cursor_pointer()
                            .hover(move |s| s.bg(rgb(palette.hover)))
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
                    })),
            );
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
                                    .h(px(30.0))
                                    .flex()
                                    .items_center()
                                    .bg(rgb(if index % 2 == 0 {
                                        palette.panel
                                    } else {
                                        palette.panel_alt
                                    }))
                                    .border_b_1()
                                    .border_color(rgb(palette.border))
                                    .children(rows[index].iter().map(|cell| {
                                        div()
                                            .flex_1()
                                            .min_w(px(100.0))
                                            .px_2p5()
                                            .truncate()
                                            .child(cell.clone())
                                    }))
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1()
                .min_h(px(220.0)),
            );
            out = out.child(table);
        }

        // Key-value structured rows
        if !result.rows.is_empty() {
            let mut list = div().flex().flex_col().gap_1p5();
            for (index, (key, value)) in result.rows.iter().enumerate() {
                list = list.child(self.copy_row(index, key, value, mono_size, palette, cx));
            }
            out = out.child(list);
        }

        // Colored inline diff lines
        if !result.diff_lines.is_empty() {
            let mut diff_box = div()
                .flex()
                .flex_col()
                .rounded_md()
                .border_1()
                .border_color(rgb(palette.border))
                .overflow_hidden();
            for (line, kind) in &result.diff_lines {
                let (row_bg, row_fg) = match kind {
                    Some(true) => (palette.success_soft, palette.success),
                    Some(false) => (palette.danger_soft, palette.danger),
                    None => (palette.panel, palette.muted),
                };
                diff_box = diff_box.child(
                    div()
                        .px_3()
                        .py_0p5()
                        .bg(rgb(row_bg))
                        .text_size(px(mono_size))
                        .text_color(rgb(row_fg))
                        .child(line.clone()),
                );
            }
            out = out.child(diff_box);
        } else if result.rows.is_empty() && result.table.is_none() {
            // Secondary text output (e.g. Password / Token / QR SVG)
            out = out.child(
                div()
                    .flex_1()
                    .min_h(px(180.0))
                    .rounded_md()
                    .border_1()
                    .border_color(rgb(palette.border))
                    .overflow_hidden()
                    .child(
                        Editor::new(&self.editors.as_ref().expect("editors mounted").output)
                            .readonly(true)
                            .appearance(false)
                            .bordered(false)
                            .size_full(),
                    ),
            );
        }

        out.into_any_element()
    }
}

fn wcag_badge(level: &'static str, passed: bool, palette: Palette) -> AnyElement {
    div()
        .px_1p5()
        .py_0p5()
        .rounded_sm()
        .bg(rgb(if passed {
            palette.success_soft
        } else {
            palette.danger_soft
        }))
        .text_size(px(10.0))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(if passed {
            palette.success
        } else {
            palette.danger
        }))
        .child(format!("{level} {}", if passed { "✓" } else { "×" }))
        .into_any_element()
}

fn input_pane_title(tool: ToolId) -> &'static str {
    match tool {
        ToolId::TextDiff | ToolId::JsonDiff | ToolId::ListCompare => "对比输入 A / B",
        ToolId::UuidUlidGenerator => "生成器与版本反解",
        ToolId::PasswordTokenGenerator => "生成器参数",
        ToolId::ChmodCalculator => "权限矩阵与路径",
        ToolId::ColorConverter => "颜色表达式",
        _ => "输入",
    }
}

pub(super) fn options_bar(palette: Palette, groups: Vec<AnyElement>) -> AnyElement {
    div()
        .flex_shrink_0()
        .flex()
        .flex_wrap()
        .items_center()
        .gap_x_3()
        .gap_y_1p5()
        .px_3p5()
        .py_2()
        .bg(rgb(palette.panel_alt))
        .border_b_1()
        .border_color(rgb(palette.border))
        .children(groups)
        .into_any_element()
}

pub(super) fn option_group(
    label: &'static str,
    palette: Palette,
    control: impl IntoElement,
) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap_1p5()
        .when(!label.is_empty(), |el| {
            el.child(
                div()
                    .text_size(px(11.0))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(palette.muted))
                    .child(label),
            )
        })
        .child(control)
        .into_any_element()
}

impl Render for ToolView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.ensure_editors(window, cx);
        if self.needs_run {
            self.schedule(true, false, cx);
        }
        let (palette, mono_size) = self
            .workbench
            .upgrade()
            .map(|wb| {
                let state = wb.read(cx);
                (
                    crate::theme::resolve(state.settings().theme, window),
                    f32::from(state.settings().editor.font_size),
                )
            })
            .unwrap_or((crate::theme::DARK, 14.0));

        let controls = match &self.model {
            Model::Standard(options) => options.controls(self, palette, cx),
            Model::Converter(options) => options.controls(self, palette, cx),
            Model::Debug(options) => options.controls(self, palette, cx),
        };

        let input_card = self.render_input_card(palette, cx);
        let output_card = self.render_output_card(mono_size, palette, cx);

        let body: AnyElement = match self.split {
            SplitMode::SideBySide => div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .flex()
                .gap_3()
                .p_3()
                .child(input_card)
                .child(output_card)
                .into_any_element(),
            SplitMode::Stacked | SplitMode::Single => div()
                .flex_1()
                .min_h_0()
                .min_w_0()
                .flex()
                .flex_col()
                .gap_3()
                .p_3()
                .child(input_card)
                .child(output_card)
                .into_any_element(),
        };

        let (dot_color, status_text, inline_error) = match &self.runtime.state {
            ExecutionState::Idle => (
                palette.subtle,
                "空闲就绪 · 输入内容或点击「示例」开始".to_owned(),
                None,
            ),
            ExecutionState::Pending => (palette.accent, "正在等待输入稳定…".into(), None),
            ExecutionState::Running
                if self
                    .started
                    .is_some_and(|start| start.elapsed() >= Duration::from_millis(150)) =>
            {
                let progress = *self.inputs.progress.lock().expect("progress lock");
                (
                    palette.accent,
                    progress
                        .map(|p| {
                            format!(
                                "计算中… {:.1}% · 已处理 {} 字节 · 可随时取消",
                                p.fraction() * 100.,
                                p.processed
                            )
                        })
                        .unwrap_or_else(|| "后台计算中… 可点击右上角取消".into()),
                    None,
                )
            }
            ExecutionState::Running => (palette.accent, "计算中…".into(), None),
            ExecutionState::Valid => (
                palette.success,
                self.result
                    .as_ref()
                    .map(|r| r.footer.clone())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "计算完成".into()),
                None,
            ),
            ExecutionState::Manual => (
                palette.warning,
                "大负载保护已启用（> 1 MB）：请点击右上角「运行」执行".into(),
                None,
            ),
            ExecutionState::Cancelled => (palette.muted, "已取消本次计算".into(), None),
            ExecutionState::Stale(error) => (
                palette.warning,
                "输入暂未闭合 · 输出区保留上一份有效结果（复制已锁定）".into(),
                Some((true, error.clone())),
            ),
            ExecutionState::Failed(error) => (
                palette.danger,
                format!("解析提示：{}", error.message),
                Some((false, error.clone())),
            ),
        };

        let mut root = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(palette.bg))
            .text_color(rgb(palette.text))
            .child(self.header(palette, cx))
            .child(controls);

        // Show compact banner above workspace when Stale (while output pane keeps prior valid text).
        if let Some((true, error)) = inline_error {
            let location = error
                .span
                .map(|s| format!(" · 第 {} 行，第 {} 列", s.line, s.column))
                .unwrap_or_default();
            root = root.child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_3p5()
                    .py_1p5()
                    .bg(rgb(palette.warning_soft))
                    .border_b_1()
                    .border_color(rgb(palette.warning))
                    .text_xs()
                    .text_color(rgb(palette.warning))
                    .child(icon_el(IconName::CircleAlert, 13.0, palette.warning))
                    .child(div().flex_1().min_w_0().truncate().child(format!(
                        "编辑中语法未闭合：{}{} {}",
                        error.message,
                        location,
                        error.hint.as_deref().unwrap_or("")
                    ))),
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
                .h(px(24.0))
                .flex_shrink_0()
                .px_3p5()
                .flex()
                .items_center()
                .justify_between()
                .gap_3()
                .bg(rgb(palette.panel_alt))
                .border_t_1()
                .border_color(rgb(palette.border))
                .text_size(px(11.0))
                .text_color(rgb(palette.muted))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .child(
                            div()
                                .w(px(6.0))
                                .h(px(6.0))
                                .flex_shrink_0()
                                .rounded_full()
                                .bg(rgb(dot_color)),
                        )
                        .child(div().min_w_0().truncate().child(status_text)),
                )
                .child(
                    div()
                        .flex_shrink_0()
                        .text_color(rgb(palette.subtle))
                        .child("实例独立内存态 · 拖放文件直达"),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Debounce, ExecutionState, Inputs, ToolRuntime, debounce, is_auto_seed_tool};
    use crate::views::Model;
    use devtools_core::{ToolError, ToolErrorKind, ToolId};
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
    fn empty_inputs_stay_idle_for_non_generators_and_seed_generators() {
        let empty = Inputs::default();
        for tool in ToolId::ALL {
            let model = Model::new(tool);
            if is_auto_seed_tool(tool) {
                assert!(
                    model
                        .execute(&empty, &devtools_core::CancelFlag::new(), false)
                        .is_ok(),
                    "{tool:?} auto-seed failed"
                );
            } else {
                assert!(
                    empty.is_effectively_empty(tool, &model),
                    "{tool:?} should stay quiet Idle on empty input"
                );
            }
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
