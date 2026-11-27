# DevTools（V1）产品与技术架构总览规格

> **定位**：基于纯 Rust + GPUI 原生构建的跨平台常驻后台开发效率桌面客户端（Windows / macOS 一等体验，Linux 核心可用），融合 `IT-Tools` 的高频离线广度与 `DevToys` 的桌面交互和 **智能识别** 理念。  
> **状态**：V1 规格已全部定案（对应 [Wayfinder 地图 #1](https://github.com/wangneal369/IT-Tools/issues/1) 及子票 `#2`–`#17`）。本文档为 `main` 分支实施总入口，核心术语遵循 [GLOSSARY.md](../../GLOSSARY.md)，不可逆架构决策与权衡详见 `docs/adr/`。

---

## 1. 产品身份、平台优先级与范围边界

| 维度 | V1 锁定规格 |
| :--- | :--- |
| **产品名称与标识** | 产品名 **`DevTools`**；三端统一 reverse-DNS 标识 **`io.github.wangneal369.devtools`**；桌面可执行文件名 **`devtools`** |
| **语言与检索** | 界面中文为主（技术专有名词保留英文）；**快速命令面板**与侧边栏支持中英 / 缩写 / 拼音加权模糊检索 |
| **平台优先级** | Windows（Direct3D 11 + `fxc.exe`）、macOS（Metal）为一等体验；Linux（`gpui_wgpu` Vulkan，X11 + Wayland）保证核心可用 |
| **离线与隐私基线** | **100% 纯离线计算内核**、零静默联网、零遥测、零剪贴板历史落盘、零工具载荷与密钥落盘 |
| **工具规模** | 7 大扁平单主 **工具分类**，精选 **28 个**域聚合高频核心 **工具** |

**明确不纳入 V1（Out of Scope）**：内嵌 WebView / 兼容 DevToys `.NET` 插件；运行时动态插件（WASM/dylib）与插件市场；独立数据处理 CLI 交付（仅保持 **工具内核** 与 GPUI 解耦预留边界）；CyberChef 式多节点管道（V1 仅支持只读输出单跳 **工具接力**）；主动发起网络请求的本机网络诊断工具（顺延 V2）；后台剪贴板历史管理器与独立双壳浮条；自动更新、i18n 多语言、云同步/账号与遥测。

---

## 2. 权威架构决策与一手资产索引

### 2.1 架构决策记录（ADRs）
- **[ADR-0001](../adr/0001-adopt-gpui-kit-pinned-ui-layer.md)**：`longbridge/gpui-kit = 0.11.0`（默认 `gpui-pre = 0.3.14` 后端）、**MSRV Rust 1.98+**（`edition = "2024"`）、按里程碑锁版升级、精选 8 门 `tree-sitter-*` + 1 MB / 50 KB 大文本降级、原生 UI 字体 + 内嵌 JetBrains Mono（默认关连字）。
- **[ADR-0002](../adr/0002-two-stage-headless-background-residency.md)**：`QuitMode::Explicit` + 三态两阶段后台休眠（`活跃态` ↔ 60s `热待机` ↔ `深休眠 / Headless`）、无锁 `async_channel` 事件总线、三端抢焦与平台护栏。
- **[ADR-0003](../adr/0003-focus-triggered-hybrid-smart-detection.md)**：零后台轮询的「获焦自动检测 + 显式智能粘贴」双通路、混合剪贴板适配层（`Files > Image > Text`）、后台并发分层置信度 DAG 与密码管理器零读取护栏。
- **[ADR-0004](../adr/0004-pure-rust-tool-framework-and-workspace-architecture.md)**：7-Crate 严格单向 Cargo Workspace、显式集中静态清单注册、零 `tokio` 纯同步/流式 **工具内核**、双轨 UI 扩展（7 个标准模板 + 21 个原生 GPUI 自定义视图）与窗口无关权威 **标签会话**。
- **[ADR-0005](../adr/0005-local-config-and-persistence-boundary.md)**：`directories::ProjectDirs::from("io.github", "wangneal369", "devtools")`、`config.toml`（仅用户偏好，开机自启以 OS 为唯一源）与 `window-state.toml`（窗口几何及可选 **标签骨架恢复**）分文件原子写盘、损坏 `*.invalid-<timestamp>` 备份与零工具载荷落盘红线。

### 2.2 一手调研与原型归档分支（不合入 `main`）
- 调研分支：`research/gpui-ui-stack`（`#2`）、`research/gpui-background-residency`（`#3`）、`research/it-tools-devtoys-inventory`（`#4`）、`research/v1-tool-crates`（`#13`）。
- 原型分支：`prototype/workbench-shell`（`#9`，A+B 融合壳层可交互原型）、`prototype/headless-residency`（`#12`，Windows 两阶段 `Headless` + 托盘热键 + 单实例 IPC 实机原型）。

### 2.3 7-Crate 严格单向 Workspace 拓扑
```text
devtools-app (薄入口: 先行 CLI --toggle/--show/--silent 单实例转发秒退，不初始化 GPUI)
  ├── devtools-ui        (唯一引入 gpui-kit; A+B 融合工作台、共享积木、模板渲染器、28 工具视图)
  ├── devtools-platform  (OS 集成: tray-icon/muda, global-hotkey/ashpd, 单实例 IPC, auto-launch, arboard, TOML 原子配置 I/O)
  ├── devtools-search    (nucleo-matcher 中英/缩写/拼音模糊检索)
  ├── devtools-detect    (零 GPUI; 分层 DAG 引擎 + 20 组探测器; 与 devtools-tools 同级互不依赖)
  ├── devtools-tools     (零 GPUI / 零 tokio; 28 工具纯计算内核，按 7 分类设 Cargo feature)
  └── devtools-core      (零重依赖; 领域契约、AppSettings/WindowState、ParsedHint、ToolError、CancelFlag)
```

---

## 3. 常驻后台、主工作台壳层与资源红线

### 3.1 生命周期、唤醒与平台契约
- **三态休眠**：关窗（`on_window_should_close` 返回 `false` + `window.remove_window()`，规避 Zed `#66369`）→ 保存几何并进入 **热待机**（全部 **标签会话** 留在窗口无关 `Entity`）→ 空闲 **60s** 后调用 `request_windowing(WindowingRequest::Headless)` 进入 **深休眠**（Windows `detach_gpu()` 停 `VSyncProvider`；macOS 默认混合模式收起瞬间切 `Accessory`；Linux 持剪贴板选区且无剪贴板管理器接管时暂缓断连）。
- **静默启动避坑**：开机自启默认关闭，开启后走 **静默启动**——必须使用 `application().with_quit_mode(QuitMode::Explicit).with_windowing(WindowingRequest::Headless)`，**严禁**调用 `gpui_kit::platform::headless()`（永久不可切回窗口态）。
- **单实例 IPC 与 CLI 控制参数**：无参（开窗或唤醒聚焦）、`--silent`（静默进深休眠；已有实例则静默退出）、`--toggle`（**智能切换**，兼作 Linux Sway/wlroots 快捷键兜底）、`--show`（幂等唤起聚焦）。Windows 采用 `CreateMutexW` + Named Pipe 回传 PID + `AllowSetForegroundWindow(pid)`；macOS 用 `on_reopen` + Unix Socket；Linux 用 Session D-Bus 透传 `XDG_ACTIVATION_TOKEN`（回退 `$XDG_RUNTIME_DIR` Socket）。
- **快捷键、托盘与三层 `Esc`**：
  - 默认全局热键：Win/Linux `Ctrl+Alt+Space`，macOS `Control+Option+Space`（禁单键/媒体键；Wayland 禁用 `global-hotkey`，切 `ashpd` Portal + `--toggle`）；
  - 托盘左键 **智能切换**、右键 6 组零剪贴板静态菜单（`显示/隐藏`、`快速搜索命令面板`、`收藏工具 ▸` 最多 10 项/无收藏回退最近 5 项、`设置...`、`开机自启`、`退出`）；Linux 强制 `tray-icon` 禁用 GTK 仅用 `ksni` + `muda (snapshot)`，无 SNI 宿主默认关窗降级直接退出；
  - `Esc` 三层退避：① 关顶层浮层 → ② 退编辑器次级状态 → ③ 主画布常态收起到托盘；`Ctrl/Cmd+W` 关最后标签保留窗口回 **工作台首页**；
  - 有序退出（托盘「退出」/ `Cmd+Q` / `WM_ENDSESSION`）：取消后台任务 → 销毁窗口与显式 Drop 托盘（先 `set_visible(false)`）/热键句柄 → `cx.quit()`（规避 Zed `#66382`）。

### 3.2 A + B 融合主工作台壳层（`#9` 原型定案）
- **可折叠双态侧边栏**：宽屏默认 `~196px` 展开树状导航（首页 / 收藏 / 7 分类默认折叠）；一键或小窗 `≤ 820px` 自动收拢为 `~46px` 图标轨 + 零挤压浮层抽屉（Flyout Drawer）。
- **双模式页签栏**：默认 **多行等宽网格**（标题栏下方，最多 3 行纵向滚动，标题栏居中放 `Ctrl/Cmd+K` 入口）↔ 一键切换 **单行融合滚动**（融入标题栏最大化代码高度）。
- **聚焦工具画布**：顶部收敛为单行 `44px` 主栏（左侧名称+实例号+模式下拉；右侧分栏切换 `左右/上下/单栏`、多开 `+`、收藏 `☆`、**工具接力** `发送到 ▸` 与整屏唯一高权重主按钮「**复制并收起**」）；贴边 `1px` 分栏、暖炭灰 Dark / 柔和纸白 Light 双主题（默认跟随系统）、单色 `1.6px` 线性 SVG 与底部单行 `26px` 全局状态底栏。

### 3.3 Windows Release 实测与 V1 验收预算红线（`#12` 定标）

| 指标 | `热待机`（60s 宽限期） | `深休眠 (Headless)`（长驻态） | `--silent` 静默冷启（未开窗） |
| :--- | :--- | :--- | :--- |
| **稳态 CPU（单核）** | `≤ 1.0%`（实测 `0.31%`） | **`≤ 0.1%`**（实测 `0.000%`） | **`≤ 0.1%`** |
| **内存（Private / RSS）** | RSS `≤ 100 MB` | Private `≤ 40 MB` / RSS `≤ 65 MB` | Private `≤ 20 MB` / RSS `≤ 35 MB` |
| **GPU 专用显存** | `≤ 120 MB` | **`0 MB`**（实测 `0.00 MB`） | **`0 MB`** |
| **唤醒至首帧可交互（P95）** | **`≤ 100 ms`**（实测 `62.3 ms`） | **`≤ 220 ms`**（实测 `161.7 ms`；IPC 端到端 `≤ 280 ms`） | 同深休眠 |

---

## 4. 智能识别、快速命令面板与工具接力

- **双触发通路（严禁后台轮询）**：① 主工作台获焦异步执行一次；② 显式智能粘贴（工作台内 `Ctrl/Cmd+Shift+V` 或 **工作台首页** `Ctrl/Cmd+V`，作为 macOS 15.4+/26 拒权或关自动检测时的零弹窗降级）。结合剪贴板变更序号/内容摘要去重与「自我复制抑制」指纹，绝不自推、不抢输入框焦点。
- **混合剪贴板与分层 DAG（`devtools-detect`）**：按 `Files > Image > Text` 读取（Win/macOS 优先 `arboard 3.7`，Linux 获焦优先 GPUI 内置读）；后台线程池并发执行，软目标 `< 30 ms`、硬超时 `300 ms`；4 级置信度 `Exact (100) > High (75) > Medium (50) > Low (25)`，`Medium+` 触发顶栏轻提示条（≤ 3 项）+ 命令面板首组置顶（≤ 5 项）+ 侧边栏全位置微高亮（自动跳转默认关）。
- **20 组 V1 探测器**：
  - `Files` 根：`ImageFile`（≤ 20 MB）、`CertificateFile`（拒私钥）、`File`；`Image` 根：`Image`（`imagesize` 仅读头元数据）；
  - `Text` 根：`Jwt`、`CertificatePem`（拒私钥/CSR）、`Base64Image`、`GZipBase64`（`H4sI`）、`Json` → `JsonArray`（拒纯标量）、`XmlHtml`、`Yaml`、`Toml`、`Url` / `UrlEncoded`、`DateTime` / `UnixTimestamp`（10/13 位限定 1975–2100）、`Cron`、`Ipv4Cidr`、`Color`（需 `#` 或函数形式）、`Base64Text`（≥ 8 字符且合法 UTF-8）、`HtmlUnicode`、`EscapedText`（豁免 `C:\...` 与 `\\...`）、`Sql`、`Chmod`、`NumberBase`（无前缀裸十进制降 `Low`）、`HashDigest`（严格 32/40/64/128 位十六进制）。
- **6 条显式互斥压制**：① `Jwt`/`GZipBase64`/`Base64Image`/`CertificatePem` 压制 `Base64Text`；② `Json`/`JsonArray` 压制 `Yaml`/`EscapedText`；③ `Url` 压制 `HtmlUnicode`/`Base64Text`；④ `UnixTimestamp` 压制 `NumberBase`；⑤ `HashDigest` 压制 `Base64Text` 与无前缀 `NumberBase`；⑥ `strict` 模式叶子抑制父级泛化工具（`JsonArray` 例外保留 `JSON 格式化` 紧随 `JSON 转表格` 之后）。
- **防覆盖与工具接力**：携带数据打开已有标签时，仅当输入为空或同指纹才复用，否则自动新建独立实例（`#1 / #2`）；只读输出区「发送到 ▸」复用 `relay` 模式 DAG（排除自身 + 追加一级父类型）执行单跳传递。
- **四道隐私与大负载护栏**：① 密码管理器隐藏标记零读取跳过（macOS `ConcealedType`/`TransientType`/`AutoGeneratedType`/`1Password`、Win `ExcludeClipboardContentFromMonitorProcessing`/`CanIncludeInClipboardHistory == 0`、Linux `x-kde-passwordManagerHint=secret`）；② 私钥/高危云密钥前缀跳过自动提示且预览脱敏；③ 敏感工具输出默认写排除剪贴板历史标记；④ 文本 256 KB 全量 / 2 MB 64 KB 魔数嗅探 / > 2 MB 跳过、图片 > 20 MB 延迟、单文件 > 100 MB 仅匹配流式哈希。

---

## 5. 本地配置、持久化边界与热生效

- **存储路径与双文件拆分**：`directories::ProjectDirs::from("io.github", "wangneal369", "devtools")` 下的 `config_dir()`：
  - **`config.toml`（`schema_version = 1`）**：仅用户偏好——主题、代码字号/连字/本机等宽字体、侧边栏折叠、页签栏网格↔单行、**收藏**有序 `ToolId`、`最近使用` 显示开关、**标签骨架恢复**开关（默认关）、全局/工作台快捷键、关窗/最小化/`Esc` 收托盘语义、窗口唤起位置策略、智能识别开关与自动跳转模式、敏感复制排除历史、macOS Dock 三态、Linux 无托盘强制常驻、Wayland portal 热键展示文案。**开机自启不写入 `config.toml`，以 OS `auto-launch::is_enabled()` 为唯一事实源**。
  - **`window-state.toml`**：程序自动记录的窗口宽高/坐标/最大化，以及开启 **标签骨架恢复** 时的有序 `ToolId` 列表与 `active_tab_index`。
- **写盘与损坏回退**：同目录临时文件 + 原子 `rename` 写盘；读取/校验失败重命名为 `*.invalid-<timestamp>` 后以默认配置冷启动并一次性提示，禁止静默合并猜测；高版本 `schema_version` 提示升级应用。
- **绝对禁止落盘黑名单**：任意工具输入/输出/中间载荷、`TabSession`、剪贴板正文/指纹/探测缓存、JWT/HMAC/私钥/PFX 口令、Hash 预期值、崩溃转储用户正文；**`最近使用` 仅驻进程内存（最近 5 项，彻底退出清空）**。`devtools-core` 对 `TabSession` 与剪贴板指纹结构禁止实现磁盘 `Serialize` 写盘路径。
- **生效分级**：外观/编辑器/智能识别/`Esc` 立即生效；关窗/最小化/唤起位置下一次生效；全局热键与开机自启保存时注销→重注册，失败回滚 UI 保证与 OS 一致。

---

## 6. V1 28 工具规格总表

> **统一执行契约（ADR-0004）**：全部工具纯同步/流式运行于 GPUI `BackgroundExecutor`，每标签单调 `generation: u64` + `CancelFlag` 防乱序；防抖分级（显式/注入 0 ms、≤ 64 KB ~100 ms、64 KB–1 MB ~250–300 ms、> 1 MB 切手动运行）；耗时 > 150 ms 在底栏展示进度与取消；编辑期临时语法错保留上次有效输出并标记 `Stale`。

| 分类 | # | 工具 | UI 轨道 | 核心 Crate | 关键选项 / 交互与安全/降级边界 |
| :--- | :-: | :--- | :--- | :--- | :--- |
| **1. 转换** | 1 | **数据格式互转** | 标准模板 | `serde_json` / `serde-saphyr 2.2` / `toml 1.2` / `roxmltree` + `quick-xml >= 0.42` | JSON/YAML/TOML/XML 互转、缩进 2/4、排序键；TOML 遇 `null`/顶层数组严格报错给提示；XML `@attr`/`#text`；YAML 多文档合并数组 |
| | 2 | **日期与时间戳转换** | 自定义视图 | `jiff 1.0` | 单入口解析 ISO/RFC/常见日期/纯数字时间戳（≤10/11–13/14–16/≥17 位自动判秒/毫秒/微秒/纳秒可覆写）；本地+UTC+IANA 时区双向联动 |
| | 3 | **进制转换** | 自定义视图 | `num-bigint 0.5` | 2/8/10/16 四联同步 + 自定义 2–36 进制；大小写、前缀、分组空格；**4096-bit** 上限；默认数学带符号 + 可选 8/16/32/64-bit 补码 |
| | 4 | **颜色转换** | 自定义视图 | `csscolorparser 0.9` | 色块 + HSV/Alpha 拾色器；双向 HEX/RGB/HSL/HWB/OKLCH/CMYK（Alpha < 100% 自动带透明通道）+ WCAG 对 `#FFF`/`#000` 对比度徽章 |
| | 5 | **JSON 转表格 / CSV** | 自定义视图 | `serde_json` + `csv 1.4` | 容错单对象/标量数组；对象点号拍平（分隔符可配）、数组字段紧凑 JSON 串；`DataTable` 排序；超 **50k 单元格**截断预览但全量导出 CSV（可选 BOM）/TSV/MD |
| **2. 编解码** | 6 | **Base64 编解码** | 自定义视图 | `base64 0.23` + `data-url` + `image 0.25` | 文本（标准/URL-safe、`=` 填充、逐行）+ 图片/文件（Data URI、缩略预览、复制图片/另存）；**20 MB** 单文件上限 |
| | 7 | **URL 编解码与解析** | 自定义视图 | `url >= 2.5.9` + `percent-encoding` + `form_urlencoded` | 结构拆解 + `KeyValueTable` 双向 Query（`Vec` 保序支持同名重复键，`%20`/`+` 可选）+ 组件级/完整 URL Percent 逐行编解码 |
| | 8 | **HTML 实体与 Unicode** | 标准模板 | `html-escape 0.3` + `std` | 编码/解码；HTML 实体 / `\uXXXX` / CSS `\XXXXXX` / `U+XXXX`；仅非 ASCII 开关 |
| | 9 | **字符串转义 / 反转义** | 标准模板 | `std` 自研 | 转义/反转义；JSON / C-Rust / Shell-POSIX / Regex 风格；逐行开关；智能识别豁免 Windows 路径 |
| | 10 | **GZip 压缩与解压** | 标准模板 | `flate2 (miniz_oxide)` + `base64` | 压缩/解压；Base64/Hex；级别 1–9；底栏压缩率；**GZip Bomb 硬上限 32 MB 或 100× 膨胀比** |
| **3. 格式化** | 11 | **JSON 格式化与压缩** | 标准模板 | `serde_json (preserve_order)` + `indexmap` | 美化/压缩；2/4/Tab 缩进；键字母序排序（默认保序）；禁 `arbitrary_precision`；行列 `SourceSpan` |
| | 12 | **SQL 格式化** | 标准模板 | `lax-sql 0.3` | 关键字大写/小写/保留；展开/紧凑布局；缩进 2/4；30 方言通用免选方言 |
| | 13 | **XML / HTML 格式化** | 标准模板 | `markup_fmt 0.28` + `quick-xml >= 0.42` | 自动/XML/HTML；美化/压缩；缩进 2/4/Tab；未闭合标签报 `InvalidSyntax` 保 `Stale` |
| **4. 生成器** | 14 | **UUID / ULID 生成器** | 自定义视图 | `uuid 1.29`（禁 v3/v5）+ `ulid 3.0` | v4（默认）/v7/v1/NIL/ULID；批量 `1–1000`；大小写/连字符/行/JSON/引号输出；附单行版本与时间戳反解区 |
| | 15 | **密码与随机 Token** | 自定义视图 | `rand 0.10` + `getrandom 0.4` + `zxcvbn 3.1` | 密码（4–128、自定义符号、排易混字符、每类保底 1 位）+ Token（16/32/64B，Hex/B64URL/B32）；批量 `1–100`；香农熵+强度；**强制 `sensitive_output = true`** |
| | 16 | **二维码生成与解码** | 自定义视图 | `qrcode 0.15` + `rqrr 0.11` + `image 0.25` | 生成（文本/URL + WiFi 模板，L/M/Q/H，SVG/PNG 导出与复制图片）；解码（拖拽/粘贴，`rqrr` 原图→反色→白边 3 步预处理 + 工具接力） |
| **5. 文本对比** | 17 | **文本对比** | 自定义视图 | `similar 3.2` + `gpui-kit` `Diff` | 并排/行内、忽略空白/大小写、折叠未变行、词级高亮、差异跳转、Unified Diff；> **1 MB** / **20k 行**切手动并关词级高亮，> **10 MB** 截断 |
| | 18 | **JSON 语义对比** | 自定义视图 | `serde_json` + `indexmap` 自研 | 忽略键序/空白噪声，数组按序（可选无序）；JSON Pointer 差异列表 + 规范化键排序双栏对比 + 复制差异报告 |
| | 19 | **文本分析与实用工具** | 自定义视图 | `convert_case 0.12` + `unicode-segmentation` + `deunicode` | 左输入 + 右三标签：12 种命名实时复制 / 行去重排序反转修剪 `LF↔CRLF` / Unicode 字素字节词行段落与 Top-20 词频 |
| | 20 | **列表集合对比** | 自定义视图 | `std` + `indexmap` | 双输入 A/B；分隔符可配、修剪/去重/忽略大小写；`IndexMap` 保序四视图（交集/并集/仅 A/仅 B）+ 计数徽章与复制 |
| **6. 调试开发** | 21 | **正则表达式测试** | 自定义视图 | `regex 1.13` + `fancy-regex 0.20` | 透明双引擎（常规自动线性，可选严格安全模式）；`i/m/s/x/U/g`、多色高亮、捕获组表、替换预览、速查；防 ReDoS 实时 **200k** / 手动 **1M** 步 + `CancelFlag` + 渲染限 **5k** 条 |
| | 22 | **JSONPath 查询测试** | 自定义视图 | `serde_json_path 0.7` | 严格 RFC 9535；**前置拦截未闭合 `\u` 规避 `#124` panic**；规范化路径列表 ↔ 匹配值数组双视图；区分合法 0 命中与语法错；速查抽屉 |
| | 23 | **Cron 表达式解析** | 自定义视图 | `croner 5.0 (std, jiff)` | 自动兼容 5/6 段（含 Quartz `L`/`W`/`#`/`?`）；时区切换；未来触发 `5/10/20/50`（默认 10）；中文主 + 英文副自然语言释义 + 预设与速查 |
| | 24 | **Chmod 权限计算器** | 自定义视图 | `std` 自研 | 3×3 `r/w/x` 矩阵 + 特殊位（SUID/SGID/Sticky，全 0 输出 3 位、非 0 切 4 位）；八进制/符号双向联动与 `chmod` 命令一键复制 |
| **7. 安全网络** | 25 | **JWT 解析与签发** | 自定义视图 | `jsonwebtoken 12 (rust_crypto)` + `zeroize` | 零密钥解析 + Claims 时效徽章；离线验签与签发 HS/RS/PS/ES/EdDSA（纯 Rust 已修 Marvin Attack）；密钥内存 `zeroize`；**`sensitive_output = true`** |
| | 26 | **Hash / HMAC 与文件校验** | 自定义视图 | `digest 0.11` 家族 + `blake3` + `constant_time_eq` | 文本 + 大文件 64 KiB 单遍流式（~50 ms 节流进度 + 取消）；MD5/SHA1/SHA256/SHA512/BLAKE3 + HMAC；预期摘要比对框（自动接 `HashDigest` 恒定时间比对） |
| | 27 | **X.509 证书解析** | 自定义视图 | `x509-parser 0.18` + `p12-keystore 0.4` + `pem` | PEM/`.crt`/`.cer`/`.der`/`.pfx`/`.p12`（PBES2 + 传统 PFX）；加密口令仅内存 `zeroize`；**严拒私钥明文展示**；证书链/SAN/时效徽章/SHA-256 & SHA-1 指纹；禁联网 |
| | 28 | **IPv4 / CIDR 子网计算** | 自定义视图 | `ipnet 2.12` + `std::net` | 三格式自动解析（CIDR / IP+掩码 / 起止 IP 聚合）；掩码步进器、`/31`&`/32` 特例、地址属性、二/十/十六进制；仅附只读 IPv4-mapped IPv6（`::ffff:...`） |

---

## 7. 构建、打包分发、CI 门禁与三端真机验收基线

- **工具链与 Profile**：`rust-toolchain.toml` 锁定 **`Rust 1.98`**（`edition = "2024"`），提交 `Cargo.lock`；Release 启用 `opt-level = 3`、`lto = "thin"`、`codegen-units = 1`、`strip = "symbols"`、`panic = "abort"`，单二进制体积红线 **未压缩 `≤ 45 MB` / 压缩分发包 `≤ 22 MB`**；Dev 对重计算与 UI 核心依赖设 `opt-level = 2`；Windows Release 强依赖 Windows 10/11 SDK `fxc.exe`。
- **GitHub Releases 三端产物**（全附 `SHA256SUMS.txt`）：
  - **Windows (`x86_64`)**：安装器（`.msi` 或 NSIS `.exe`）+ 便携 `.zip`（初期无签名附 SmartScreen 说明，预留证书 Secret 槽位）；
  - **macOS (`aarch64`，按需 `x86_64`/Universal2)**：`DevTools.app` + `.dmg`（`CFBundleIdentifier = io.github.wangneal369.devtools`、`LSUIElement = false`；无证书做 ad-hoc 签名并回退 `LaunchAgent --silent`，预留 Developer ID + `notarytool` 槽位）；
  - **Linux (`x86_64`)**：`.deb` + `.tar.gz`（含 `install.sh`、reverse-DNS `io.github.wangneal369.devtools.desktop` 与图标，依赖可用 Vulkan 驱动）。
- **CI 双层门禁**：
  1. **PR 快速门禁**：`cargo fmt --check`、`cargo clippy --workspace --all-targets -- -D warnings`、零 GPUI 底层全量单测（含离线能力、禁止 `TabSession` 序列化、`serde_json_path` `\u` 护栏、GZip Bomb 32 MB 与 ReDoS 上限断言）、`cargo deny check`（`advisories` 零容忍且仅对 `gpui-pre`/`zxcvbn` wasm32 间接残留的 `RUSTSEC-2026-0543` 设带退出条件 ignore；`licenses` 仅宽松 SPDX；`bans` 拉黑 `serde_yaml`/`serde_yml`/`yaml-rust`/`openssl`/`aws-lc-sys`/`p12`/`single-instance`/`jsonpath-rust`/`gpui 0.2.2` 与直接依赖 `chrono`；`sources` 仅 crates.io）；
  2. **三端原生矩阵与 Release LTO 验证**：`windows-latest`（+ `--release` 实跑 `serde_json_path` 5 大内置函数验活 `inventory` 符号）、`macos-latest`、`ubuntu-24.04`（验证 `ksni` 零 GTK 依赖）。
- **发版前人工真机验收 Gate**：
  - **Windows**：复验 `#12` 资源/延迟红线、UIPI 管理员前景窗容错、微软拼音 CJK IME 组合态、密码管理器跳过与敏感复制不入 `Win+V`；
  - **macOS (14/15/26)**：后台热键跨 App 抢焦、`NSPasteboard` 拒权降级、签名 `.app` `SMAppService` 静默启动零闪窗、Dock 三态瞬时隐身、原生中文 IME；
  - **Linux (X11 + Wayland GNOME/KDE/Sway)**：GNOME/KDE Wayland 获焦剪贴板与 `ashpd` Portal 热键、Sway `--toggle`、无 SNI 托盘降级退出、持剪贴板选区暂缓切 `Headless`、Vulkan 与 Fcitx5/IBus IME。

---

## 8. V1 实施路线图（垂直切片）

| 里程碑 | 范围摘要 | 依赖 |
| :--- | :--- | :--- |
| **M1 — Workspace 脚手架与合规底座** | 7-crate Cargo Workspace、`rust-toolchain.toml` (`1.98`)、`deny.toml`、GitHub Actions CI、`devtools-core` 契约与 `AppSettings`/`WindowState`、`devtools-platform` 原子配置 I/O 与损坏回退 | 无（首期起点） |
| **M2 — 桌面常驻壳层与主工作台骨架** | `devtools-platform` 托盘/热键/单实例 IPC/自启、`devtools-ui` A+B 融合工作台与设置页、`devtools-app` 两阶段 `Headless` 生命周期与 CLI 控制参数 | M1 |
| **M3 — 智能识别、快速命令面板与工具接力** | `devtools-detect` 20 组 DAG 探测器 + 6 条压制 + 隐私护栏、`devtools-search` 模糊检索、顶栏轻提示条 / 命令面板 / 智能粘贴 / 防覆盖多开 / 单跳工具接力 | M2 |
| **M4a — 7 个标准模板工具** | `StandardTransformTool` 模板渲染器 + 工具 #1、#8、#9、#10、#11、#12、#13 内核与单测 | M3 |
| **M4b — 9 个转换/编解码/生成器自定义视图** | 工具 #2–#7、#14–#16 内核、原生 GPUI 视图与单测 | M3 |
| **M4c — 12 个文本对比/调试开发/安全网络自定义视图** | 工具 #17–#28 内核、原生 GPUI 视图与单测 | M3 |
| **M5 — 三端打包分发、LTO 验活与真机发版验收** | Windows/macOS/Linux 分发产物、Windows Release LTO `serde_json_path` 验活、三端真机验收清单闭环 | M4a, M4b, M4c |
