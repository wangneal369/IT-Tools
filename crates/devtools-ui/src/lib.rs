//! GPUI workbench shell and tool views (ADR-0001 / ADR-0004).
//!
//! 本 crate 是 7-crate workspace 中**唯一**引入 `gpui-kit` 的层，承载 A+B 融合
//! 工作台、设置抽屉、共享工具栏、工具视图注册表与 28 工具视图。
//!
//! M2 交付：可折叠双态侧边栏（含浮层抽屉）、多行网格 ↔ 单行融合页签栏、多实例
//! 工具标签、工作台首页 / 分类视图、设置面板、窗口关闭拦截与两阶段 Headless
//! 生命周期。M3/M4 通过 [`tool_view::ToolViewRegistry`] 插入真实工具视图。

pub mod boot;
pub mod labels;
pub mod settings;
pub mod shell;
pub mod smart;
pub mod theme;
pub mod tool_view;
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
