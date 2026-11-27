//! 28 工具纯计算内核（ADR-0004）。
//!
//! M1 脚手架：本 crate 为零 GPUI / 零 `tokio` 依赖的占位，28 个工具内核按 7 分类
//! 分模块、以 Cargo feature 裁剪，在 M4a/M4b/M4c 落地。所有工具为纯同步/流式函数，
//! 长耗时操作配合 [`CancelFlag`] 与进度回调，运行于 GPUI `BackgroundExecutor`。

pub use devtools_core;
