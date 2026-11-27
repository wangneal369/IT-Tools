//! GPUI workbench shell and tool views (ADR-0001 / ADR-0004).
//!
//! 本 crate 是 7-crate workspace 中**唯一**引入 `gpui-kit` 的层，承载 A+B 融合
//! 工作台、设置抽屉、共享工具栏、工具视图注册表与 28 个离线工具视图。

pub mod boot;
pub mod icons;
pub mod labels;
pub mod samples;
pub mod settings;
pub mod shell;
pub mod smart;
pub mod syntax;
pub mod theme;
pub mod tool_view;
pub mod views;
pub mod window;
pub mod workspace;

pub use boot::{AppBoot, StartMode, run};
pub use shell::WorkbenchView;
pub use tool_view::{ToolViewContext, ToolViewFactory, ToolViewRegistry};
pub use workspace::{
    HomeView, Phase, Recents, TabEntry, TabInstanceId, TabKind, Workbench, WorkbenchInit,
};

pub use devtools_core;
pub use devtools_platform;
