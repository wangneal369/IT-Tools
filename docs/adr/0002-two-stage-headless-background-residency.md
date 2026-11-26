---
status: accepted
---

# 采用“销毁窗口 + 空闲切 Headless”两阶段后台驻留与异步通道事件桥接架构

V1 常驻后台桌面壳层采用 `QuitMode::Explicit` 配合 **三态两阶段休眠模型（`Active` ↔ `Warm Standby` ↔ `Deep Sleep / Headless`）**：用户「收起到托盘」时保存窗口几何信息并销毁 GPUI `Window`（将全部多标签页与内存工作区状态保留在独立于窗口的 `Entity`/`Global` 中），空闲 **60 秒**后调用 `request_windowing(WindowingRequest::Headless)` 彻底释放 GPU 上下文与显示连接；同时将系统托盘、全局快捷键与单实例 IPC 统一通过无锁 `async_channel` 异步回流至 GPUI `ForegroundExecutor`。选用该架构是因为 GPUI `PlatformWindow` 三端均无单窗 `hide()/show()` 原语，强行原生隐藏活窗口会破坏 Wayland `xdg_toplevel` 状态机并命中 Windows 上游 VSync 空转缺陷（Zed `#66221`，60Hz 空耗 ~3.4% 单核 CPU 且不释放显存），而两阶段休眠兼顾了短期热唤起（~15–30ms）、长期零能耗（~0% CPU / ~0 MB 显存）以及 Wayland 重连注入 `XDG_ACTIVATION_TOKEN` 合法抢焦。

## Considered Options

- **拦截关闭并通过 `raw-window-handle` 原生隐藏活窗口（`SW_HIDE` / `orderOut:` / X11 `unmap`）**：虽可获得 ~1–5ms 唤起延迟，但在 Linux Wayland 下 `xdg_toplevel` 无标准隐藏原语会破坏 GPUI 帧状态机，在 Windows 会持续触发 Zed `#66221` VSync 空转且长期占用 SwapChain/Atlas 显存，属于脆弱的非官方 hack，排除。
- **仅销毁窗口停留在 `Warm Standby`，永不切入 `Headless`**：可立刻释放窗口级 SwapChain/Surface，但 Windows 端 `VSyncProvider` 线程与共享 `DirectXDevices` 仍持续常驻，且 Linux Wayland 在 `Windowed` 态缺少向已有连接注入外部 `XDG_ACTIVATION_TOKEN` 的公开 API，无法完成合法后台抢焦，排除。
- **使用 `gpui-tray` 或 Linux 默认 `tray-icon` (`libappindicator` / GTK3) 后端**：`gpui-tray` 强耦合特定 GPUI 版本快照，违反 ADR-0001 生态隔离边界；`libappindicator` 强依赖 GTK3 主循环，与 GPUI Linux `calloop` 冲突并引入重量级 C 依赖，排除。
- **在托盘菜单/热键原生回调中同步操作 GPUI `App`**：Windows `TrackPopupMenu` 与 macOS `NSMenu` 均运行嵌套模态消息循环，同步回调极易触发 `AppCell` (`RefCell`) 双重可变借用 panic，排除。

## Consequences

- **强制状态与窗口解耦**：主工作台的多标签页集合、活跃工具状态、输入/输出内存缓冲与滚动位置必须完全托管于 window-independent `Entity`/`Global`；`Window` 及其视图树仅作为可随时销毁并幂等重建的投影外壳（彻底退出应用时随进程清空，契合零敏感数据落盘红线）。
- **平台差异化休眠与安全收尾护栏**：
  - **Windows**：收起时在 `on_window_should_close` 返回 `false` 后显式调用 `window.remove_window()`（规避 Zed `#66369`）；60 秒后切 `Headless` 触发 `detach_gpu()` 停 VSync 线程；显式退出或收到 `WM_ENDSESSION` 时按序取消后台任务、销毁窗口/托盘/热键句柄再调用 `cx.quit()`（防范 Zed `#66382`）。
  - **macOS**：默认混合模式下，收起到托盘**瞬间**即切回 `ActivationPolicy::Accessory` 立即隐藏 Dock 图标（不等待 60 秒深休眠延迟）；开窗时切回 `Regular` 支持 `Cmd+Tab` 与原生顶栏菜单。
  - **Linux**：60 秒宽限期附加**剪贴板所有权护栏**——若本进程仍持有 X11/Wayland 剪贴板选区且无剪贴板管理器接管，则暂缓断连切 `Headless` 以防刚复制内容丢失；`tray-icon 0.27` 强制禁用默认 GTK feature，仅启用纯 Rust `ksni` + `muda 0.22 (snapshot)`。
- **全平台异步事件总线与降级边界**：所有 OS 集成事件（托盘点击/菜单、`global-hotkey 0.10` / Linux Wayland `ashpd 0.14`、三端单实例 IPC）一律经 `async_channel::Sender<SystemEvent>` 非阻塞投递给 GPUI `cx.spawn` 消费循环；Linux Wayland 严禁回退至假成功的 XWayland `XGrabKey`，统一提供 `--toggle` / `--show` / `--silent` 壳层控制参数走单实例 IPC 兜底，且检测到无 SNI 托盘宿主时默认将关闭按钮降级为直接退出。
- **唤起延迟与原型验证要求**：从 `Deep Sleep` 唤起需承担 ~80–200ms GPU/显示重连成本（短期高频使用由 60 秒 `Warm Standby` 吸收）；须在原型阶段对 Windows「高频收起 → `Headless` → 热键/二次启动（`AllowSetForegroundWindow`）重建 → 显式退出」执行压力与闪白冒烟验收。
