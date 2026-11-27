//! 快速命令面板 / 侧边栏模糊检索（spec §1 / ADR-0001）。
//!
//! M1 脚手架：本 crate 为零 GPUI 依赖的占位，`nucleo-matcher` 中英 / 缩写 / 拼音
//! 加权模糊检索在 M3 落地（通过 `.filterable(false)` 外接纯 Rust 打分引擎）。

pub use devtools_core;
