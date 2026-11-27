---
status: accepted
---

# 采用 TOML 壳层配置、原子写盘损坏回退与「零工具载荷落盘」持久化边界

V1 本地状态通过 **`directories::ProjectDirs::from("io.github", "wangneal369", "devtools")`** 落在三端平台标准用户配置目录，按职责拆分为 **`config.toml`（`schema_version = 1`）** 与 **`window-state.toml`**：前者仅持久化显式白名单内的用户偏好（主题、代码字体/字号/连字、侧边栏折叠、页签栏网格↔单行模式、收藏顺序、全局/工作台快捷键、常驻与窗口唤起策略、智能识别开关、macOS Dock 策略与 Linux 降级偏好；**开机自启状态以 OS `auto-launch::is_enabled()` 为唯一事实源，不在 `config.toml` 另存副本**），后者仅记录程序自动维护的窗口几何（宽高/坐标/最大化）及可选 **标签骨架恢复** 的有序 `ToolId` 列表与活跃标签索引；**`最近使用` 与全部 `标签会话` 内容仅驻进程内存**，彻底退出即清空。默认冷启动只恢复壳层偏好并打开工作台首页；开启标签骨架恢复时也仅恢复空标签实例。写盘采用同目录临时文件 + 原子替换（rename），读盘损坏时重命名为 `*.invalid-<timestamp>` 并以默认配置冷启动，同时在主工作台一次性提示。设置变更按域分级生效（外观/编辑器/智能识别立即生效；关窗与最小化语义下一次关窗生效；全局热键与开机自启保存时注销重注册并对齐 OS 状态，失败回滚 UI）。选用该边界是因为 [ADR-0002](0002-two-stage-headless-background-residency.md) 与 [ADR-0003](0003-focus-triggered-hybrid-smart-detection.md) 已将敏感工作现场与剪贴板管线锁定在内存态，任何 session 序列化或跨重启 Recents 都会正面违反零遥测、零剪贴板历史与密码管理器零读取的产品红线，且日后难以在无迁移痛苦下撤回。

## Considered Options

- **JSON 单文件或 SQLite 本地库承载工具草稿/会话**：便于崩溃恢复与跨重启续作，但无法从架构上证明用户敏感输入未落盘，且与「彻底退出清空现场」语义冲突，排除。
- **跨重启持久化 `最近使用` 列表**：仅 `ToolId` 看似低敏，但会构成可推断使用轨迹的持久日志，与 Recents「非历史管理器」定位不符，排除。
- **在 `config.toml` 中双写 `auto_launch` 布尔值**：用户通过 Windows 任务管理器（`StartupApproved`）或 macOS 登录项调整后会与本地配置文件产生双源漂移，违反 `#6` 实时对齐 OS 约定，排除。
- **损坏配置静默删除字段或部分合并**：会在未知缺失键下产生不可预测行为，排除；改用原子写盘 + 损坏具名备份 + 默认冷启动。

## Consequences

- **严格遵循 [ADR-0004](0004-pure-rust-tool-framework-and-workspace-architecture.md) Crate 分层**：`devtools-core` 仅定义纯数据模型 `AppSettings` / `WindowState` 与显式持久化白名单，禁止对 `TabSession` 或剪贴板指纹结构实现 `Serialize` 写盘路径（以单测/编译门闩守线）；`directories` 路径解析、`toml` 编解码、原子写盘与 `*.invalid-<timestamp>` 备份恢复统一归属 `devtools-platform`。
- **设置 UI 须标注每项为「立即生效」或「保存后重新注册」**；开机自启与热键状态始终以 OS 注册结果为准，失败不得留下与 OS 不一致的勾选态。
- **用户偏好与自动记录状态严格分文件**：重置或清理 `window-state.toml` 不会丢失用户偏好，可选标签骨架恢复仅记录有序 `ToolId` 与活跃标签索引，绝不恢复各标签页的输入、输出与局部选项。
