//! GPUI 工作台壳层与工具视图（ADR-0001 / ADR-0004）。
//!
//! 本 crate 是 7-crate workspace 中**唯一**引入 `gpui-kit` 的层，承载 A+B 融合
//! 工作台、共享积木（`ToolHeaderBar` / `CodePane` / `SplitWorkspace` 等）、标准模板
//! 渲染器与 28 工具视图。M1 脚手架：仅建立依赖边界，工作台与视图在 M2/M4 落地。

pub use devtools_core;
