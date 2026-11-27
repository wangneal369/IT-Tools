//! DevTools 桌面入口（薄，ADR-0004）。
//!
//! M1 脚手架：仅占位输出。CLI 参数（`--toggle` / `--show` / `--silent`）与单实例
//! 抢占转发、两阶段 `Headless` 生命周期在 M2 落地；此处不初始化 GPUI。`devtools-ui`
//! 已在依赖图中链接以证明 gpui-kit 三端构建通路，但尚未被驱动。

fn main() {
    println!(
        "devtools {} — {} built-in tools",
        env!("CARGO_PKG_VERSION"),
        devtools_core::tool::ToolId::ALL.len(),
    );
}
