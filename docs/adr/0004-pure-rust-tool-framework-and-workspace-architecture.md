---
status: accepted
---

# 采用 7-Crate 零 GPUI 分层、双轨工具扩展与无异步运行时计算内核架构

V1 工程采用 **7 个严格单向依赖的分层 Cargo Workspace**（`devtools-core`、`devtools-detect`、`devtools-tools`、`devtools-search`、`devtools-platform` 均 100% 零 GPUI 依赖，仅 `devtools-ui` 引入 `gpui-kit`，`devtools-app` 为薄二进制入口），通过 **显式集中静态清单** 注册内置工具与探测器；全部 28 个工具的 **工具内核（Tool Kernel）** 以原生强类型、**零 `tokio` / 零 `async` 运行时依赖**的纯同步/流式 Rust 函数编写（长耗时操作配合轻量 `CancelFlag` 与进度回调），在 UI 层采用 **「声明式标准模板（7 个纯文本双栏工具）+ 原生 GPUI 自定义视图组合共享积木（21 个富交互工具）」双轨扩展**，并以独立于 `Window` 的纯 Rust **标签会话（Tab Session）** 作为唯一权威内存态，由 GPUI 视图按需水合与销毁重建。选用该架构是为了严格兑现 [ADR-0001](0001-adopt-gpui-kit-pinned-ui-layer.md) 的框架隔离边界与 [ADR-0002](0002-two-stage-headless-background-residency.md) 的关窗销毁/深休眠幂等重建要求，同时兼顾常规工具低样板、复杂工具原生交互质感、单实例 `--toggle` 零 UI 初始化秒退、无链接器陷阱的确定性构建以及未来 CLI 零摩擦复用。

## Considered Options

- **按 7 分类拆分 7 个 `devtools-tools-*` crate 或每工具独立 crate（28+ crate）**：跨分类工具间存在大量基础逻辑复用（如 JWT 复用 Base64/JSON、GZip 复用 Base64），过早细拆会引入额外公共 crate 或依赖缠绕；V1 28 工具阶段以单一 `devtools-tools` 按 7 分类分模块并预留 Cargo feature 裁剪的综合成本最低。
- **使用 `inventory` / `linkme` 分布式链接器宏自动注册工具与探测器**：虽然新增工具可少改一行注册清单，但在 Windows MSVC、LTO 与静态库裁剪下存在链接段符号被静默裁掉的跨端风险，且注册顺序不确定仍需额外权重排序，排除。
- **构建 DevToys 式覆盖全部 28 个工具的通用声明式 UI Widget DSL 解释器**：适用于支持外部第三方动态插件的平台，但 V1 为纯编译期内置 28 工具且高度强调原型定案的原生 GPUI 精细交互（如 44px 聚焦主栏、虚拟化 Diff、正则多色高亮、色板、Chmod 3×3 矩阵、Query 双向表），抽象通用 Widget DSL 会造成严重泄漏与双倍维护成本，排除。
- **全仓引入 `tokio` 并把 `Tool` Trait 定义为 `async fn`**：V1 为 100% 纯离线 CPU 与本地文件流式计算，CPU 密集任务在 `tokio` 下本就需包裹 `spawn_blocking`，强绑 `tokio` 会拖慢编译、增加二进制体积并与 GPUI `BackgroundExecutor` 形成双线程池冗余，排除。
- **在窗口销毁后保留 `gpui-kit` `Editor`/`InputState` 实体跨 `Window` 长期存活**：窗口级 UI 控件通常持有 `FocusHandle`、窗口订阅或布局缓存，在 `热待机` 与 `深休眠` 重建窗口时易引发悬空状态或显存残留，且无法脱离 GPUI 独立单测工作区状态机，排除。

## Consequences

- **严格的 Crate 单向边界与同级解耦**：
  - `devtools-core` 保持零重依赖，仅定义领域契约、轻量 `std` `ParsedHint`、`ToolDescriptor`、`ToolCapabilities`（V1 全部单测强制断言离线）、`ToolError` 与 `CancelFlag`；
  - `devtools-detect` 与 `devtools-tools` 互不依赖（同级依赖 `devtools-core`），避免探测引擎拉入重型工具依赖并支持并行编译；
  - `devtools-search` 与 `devtools-platform` 均隔离于 GPUI 之外；`devtools-app` 先完成 CLI 参数（`--toggle`/`--show`/`--silent`）与单实例抢占转发，命中已有实例立即退出而不初始化 GPUI。
- **双轨 UI 扩展纪律**：`StandardTransformTool` 严格收敛为 4 类选项原语（`ModeSwitch` / `EnumSelect` / `Toggle` / `IntStepper`）与单输入→单输出文本双栏，仅覆盖 V1 的 7 个纯文本转换/格式化/编解码工具；其余 21 个工具一律编写原生 GPUI 视图并组合 `devtools-ui` 共享积木（`ToolHeaderBar`、`ToolStatusBar`、`CodePane`、`SplitWorkspace`、`OutputActionBar`、`FileDropZone`、`KeyValueTable`、`CheatsheetPopover`），禁止将标准模板膨胀为通用 UI DSL。
- **强类型数据注入与防覆盖统一接口**：所有工具会话统一实现 `is_empty()`、`content_fingerprint()`、`accept_input(ToolLaunchInput)` 与 `relay_output() -> Option<RelayPayload>`，通过 `MatchedDataChain` + `ParsedHint` 精确支持双向自动切向、指定字段落位（如 `HashDigest` 预填至比对框）与单跳 `工具接力`。
- **后台调度、可恢复错误与内存护栏**：
  - 所有工具计算由壳层统一派发至 GPUI `BackgroundExecutor` 并以每标签页单调 `generation: u64` + `CancelFlag` 防止乱序回写；
  - 实时编辑采用分级防抖（显式触发/注入 0 ms、≤ 64 KB ~100 ms、64 KB–1 MB ~250–300 ms、> 1 MB 切为手动运行对齐 1 MB 高亮降级阈值），耗时 > 150 ms 才在底栏展示进度与取消按钮（流式文件哈希按 ~50 ms 节流回传进度）；
  - 编辑期临时语法错误保留上一份有效输出并标记 `Stale`（同时底栏与输入区展示带可选 `SourceSpan` 的结构化 `ToolError` 并锁定「复制并收起」），避免输入期白屏闪烁；
  - `TabSession` 采用 `Arc<str>` / `Arc<[u8]>` 共享大载荷快照，大文件仅持路径与流式结果，窗口打开时水合 UI、关窗前同步回写并彻底释放窗口级实体。
