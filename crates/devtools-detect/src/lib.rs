//! 智能识别分层 DAG 引擎（spec §4 / ADR-0003）。
//!
//! M1 脚手架：本 crate 为零 GPUI 依赖的占位，20 组探测器、6 条互斥压制与
//! 密码管理器零读取隐私护栏在 M3 落地。与 `devtools-tools` 同级互不依赖。
//!
//! 剪贴板按 `Files > Image > Text` 读取，产出 [`MatchedDataChain`] 与
//! [`ParsedHint`]，4 级置信度 [`Confidence`]。

pub use devtools_core;
