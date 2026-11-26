---
status: accepted
---

# 采用锁版 `longbridge/gpui-kit`（`gpui-pre` 后端）作为 V1 UI 基础层并强制与计算内核解耦

V1 选择 Apache-2.0 三层框架 **`longbridge/gpui-kit`**（默认 `gpui-pre` 后端，开启 `component` 与 `assets`，精确锁定版本并按里程碑批量升级）作为桌面 UI 基础层，个别不满足需求的控件可局部下沉至同源 `gpui-base` 或底层 `gpui` 原语定制；同时强制将全部工具计算内核、智能识别与命令模糊索引隔离在零 GPUI 依赖的纯 Rust crate 中。选用该方案是为了直接获得 75+ 生产级桌面控件（含 Rope 编辑器、`Diff`、`TabBar`、`Resizable`、`Command`、`DataTable`、`TitleBar`、`Theme`），规避 crates.io 官方 `gpui 0.2.2` 已失效且纯 `gpui` 零业务控件的高额自建成本，并以清晰架构边界对冲 `0.x` 高频 Breaking Change 风险。

## Considered Options

- **官方 crates.io `gpui 0.2.2`**：停滞超过一年，仍使用已淘汰的 `blade-graphics` / `cocoa 0.26`，在 Linux 存在编译故障，且缺少常驻后台所需的 `QuitMode::Explicit`、`WindowingRequest::Headless` 与 `ActivationPolicy` 原语，不可用。
- **直接 Git 依赖 Zed `main` 的纯 `gpui` 从零自建控件**：纯 `gpui` 仅提供 12 个底层排版/绘制原语，自研 Rope 编辑器、语法高亮、虚拟表格、弹窗浮层与主题系统的工程量过大，投入产出比严重失衡。
- **仅使用 `gpui-base` 自建全部视觉组件**：仍受限于 `gpui-pre` 版本锁定，却需手写全部 60+ 控件样式，显著拖慢 V1 交付；采用 `gpui-kit` 完整组件层后仍可在局部平滑下沉至 `gpui-base`。
- **开启实验性 `gpui-fast` 后端**：目前仍处实验期，且在 `gpui-kit v0.11.0` 中会触发双 GPUI 核心同时编译（`gpui-kit#4119`）及 macOS 显存隐患，暂不启用，留待成熟后零迁移成本评估。
- **`gpui-ce` / `adabraka-ui`**：前者主动偏离上游且缺成体系业务组件库，后者已停更且绑定私有分叉，均排除。

## Consequences

- **严禁混用旧版第三方 GPUI crate**：`gpui-kit` 对 `gpui-pre-*` 精确等号锁死，无法与依赖官方 `gpui 0.2.2` 或其他 `gpui-pre` 版本的第三方 UI crate 混用。系统托盘、全局快捷键、剪贴板、单实例 IPC 与异步任务一律使用无 GPUI 依赖的纯 Rust crate 经通道桥接。
- **版本与工具链管理**：根 `Cargo.toml` 精确锁定 `gpui-kit` 版本并提交 `Cargo.lock` 与 `rust-toolchain.toml`（MSRV **Rust 1.92+**，`edition = "2024"`），采用按里程碑批量升级而非追每周小更。
- **Feature 裁剪与开发配置基线**：禁用 `gpui-fast`、`gpui-webview`、`speech`、`gpui-shell`；语法高亮仅按需启用 V1 高频 `tree-sitter-*` feature（不启用全量 `tree-sitter-languages`）；命令面板通过 `.filterable(false)` 外接纯 Rust 模糊打分引擎（如 `nucleo-matcher`）；根 `Cargo.toml` 需配置 `[profile.dev.package]` 对框架/布局/文本核心依赖开启 `opt-level = 3`，Release 开启 `strip = "symbols"`。
- **三端构建约束**：Windows 依赖 Direct3D 11 与 Windows SDK `fxc.exe`（不可从 Linux 交叉编译），CI 须使用三端原生 Runner；Linux 运行时强依赖可用 Vulkan 驱动。
