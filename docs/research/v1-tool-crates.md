# V1 28 工具纯 Rust 计算内核依赖选型与安全合规基线调研

- **关联议题**：[wangneal369/IT-Tools#13](https://github.com/wangneal369/IT-Tools/issues/13)（上游决策依赖：[wangneal369/IT-Tools#7](https://github.com/wangneal369/IT-Tools/issues/7)、[wangneal369/IT-Tools#10](https://github.com/wangneal369/IT-Tools/issues/10)、[ADR-0001](../adr/0001-adopt-gpui-kit-pinned-ui-layer.md)、[ADR-0003](../adr/0003-focus-triggered-hybrid-smart-detection.md)、[ADR-0004](../adr/0004-pure-rust-tool-framework-and-workspace-architecture.md)）
- **调研日期**：2026-11-27
- **架构硬约束**：
  1. **`devtools-tools` 与 `devtools-detect` 100% 零 GPUI、零 `tokio` / 异步运行时依赖**（纯同步/流式 Rust 函数 + `CancelFlag` 与进度回调）；
  2. **MSRV Rust 1.92+**（`edition = "2024"`，对齐 `gpui-kit v0.11.0` 工具链基线）；
  3. **许可证兼容宽松协议**（`MIT` / `Apache-2.0` / `BSD-3-Clause` / `ISC` / `Zlib` / `Unlicense` / `CC0-1.0` 等，含 `LGPL` / `GPL` / `MPL` / 非标准协议的候选须显式标红警告或排除）；
  4. **严禁强绑系统 OpenSSL 或额外 C/C++ SDK**（优先 100% 纯 Rust 实现；含 C/汇编后端的选项须核实三端构建影响与纯 Rust 替代方案）；
  5. **RustSec 与供应链安全洁净基线**：主动规避停更/归档/存在未修漏洞的 crate，并通过 `cargo deny` 守门。
- **信源原则**：本报告所有版本号、发布日期、90 日下载量、MSRV、License、Feature 配置、依赖树、源码实现细节与安全公告均于 **2026-11-27** 直接核验自 `crates.io` REST API（`https://crates.io/api/v1/crates/<name>`）、各项目 GitHub 官方仓库源码/`Cargo.toml`/Releases/Issues 及 RustSec Advisory Database（[`rustsec/advisory-db`](https://github.com/rustsec/advisory-db)）。

---

## 执行摘要（核心结论速览）

截至 **2026-11-27**，Rust 计算生态在过去 12 个月经历了 4 项深刻影响本项目选型的关键转折：

1. **纯 Rust 密码学与 JWT 正式跨越 Marvin Attack 与 `digest` 分裂拐点**：
   - RustCrypto [`rsa 0.10.0`](https://crates.io/crates/rsa) 已于 **2026-11-08** 发布正式版，完成向 `crypto-bigint` 迁移并落地恒定时间填充缓解（[`RustCrypto/RSA#626`](https://github.com/RustCrypto/RSA/issues/626) 关闭），正式修复困扰生态三年的 **Marvin Attack（[`RUSTSEC-2023-0071`](https://rustsec.org/advisories/RUSTSEC-2023-0071)，`patched = [">= 0.10.0"]`）**。
   - [`jsonwebtoken 12.0.0`](https://crates.io/crates/jsonwebtoken)（2026-11-13）的 `rust_crypto` 后端已全面升级至 **`rsa 0.10` + `sha2 0.11` + `hmac 0.13` + `p256 0.14` + `p384 0.14` + `ed25519-dalek 3`**，与 `md-5 0.11.0`、`sha1 0.11.0`、`blake3 1.8.8`（`traits-preview` → `digest 0.11.2`）、`x509-cert 0.3.1`、`p12-keystore 0.4.1` **100% 统一在 `digest 0.11` 与 `der 0.8` 生态**，实现全栈零 C 编译器、零 OpenSSL、零双份密码学依赖树。
2. **`chrono` 已正式进入 RustSec 停更清单，`jiff 1.0` + `croner 5.0` 形成零 `chrono` 时间链**：
   - [`RUSTSEC-2026-0543`](https://rustsec.org/advisories/RUSTSEC-2026-0543)（2026-09-14）已将 `chrono` 全版本标记为 `unmaintained` 并推荐迁移至 `jiff`；`gpui-kit v0.11.0` 已通过 [PR #4015](https://github.com/longbridge/gpui-kit/pull/4015) 切换至 **`jiff 1.0`**。
   - `devtools-tools` 选用 **`jiff 1.0.3`** 与 **`croner 5.0.1`**（关闭默认 `chrono`、开启 `features = ["std", "jiff"]`）可与 UI 层完全去重，且 `croner 5.0` 内置 5/6/7 段、Quartz 语法及可扩展 `describe::Language` Trait（天然支持内置英文 + 薄封装中文自然语言释义）。
   - **关键避坑**：`dateparser 0.3.1`、`cron 0.17.0`、`rxing 0.9.4` 均无条件依赖 `chrono 0.4`，`zxcvbn 3.1.1` 在 `wasm32` target 下依赖 `chrono 0.4.38`（会污染 `Cargo.lock` 触发 `cargo deny` 告警，上游 PR [`zxcvbn-rs#100`](https://github.com/shssoichiro/zxcvbn-rs/issues/100) 尚未合并），选型须针对性规避或隔离。
3. **`serde_yaml` 归档后的纯 Rust YAML 首选已收敛为 `serde-saphyr`**：
   - `serde_yaml`（2024-03 归档）与 `serde_yml`（[`RUSTSEC-2025-0068`](https://rustsec.org/advisories/RUSTSEC-2025-0068) 标记 `unsound` 并归档）严禁使用；`serde_yaml_ng`、`serde_norway` 及 YAML 组织官方维护的 `yaml_serde 0.10.7` 底层仍基于 `c2rust` 转译的 `unsafe-libyaml` / `libyaml-rs`。
   - **`serde-saphyr 2.2.0`**（`#![forbid(unsafe_code)]`、panic-free、18.26M/90d 下载、2026-11-18 发布、MIT OR Apache-2.0、MSRV 1.89）是当前最成熟的 100% 安全纯 Rust Serde YAML 替代方案；如需纯 AST 操作可辅以同源 `saphyr 0.4.1`（或 `yaml-rust2 0.13.2`）。
4. **2026 年新增高危 RustSec 漏洞划定强制版本下限**：
   - XML 必须锁定 **`quick-xml >= 0.41.0`**（推荐 `0.42.0`），以规避 [`RUSTSEC-2026-0194`](https://rustsec.org/advisories/RUSTSEC-2026-0194)（单标签重复属性 $O(N^2)$ CPU DoS）与 [`RUSTSEC-2026-0195`](https://rustsec.org/advisories/RUSTSEC-2026-0195)（`NsReader` 命名空间声明无界分配 OOM）。
   - URL 必须锁定 **`url >= 2.5.9`**，以规避 2026-11-20 披露的 [`RUSTSEC-2026-0592`](https://rustsec.org/advisories/RUSTSEC-2026-0592)（嵌套 `blob:` URL 在 `Url::origin()` 中触发不可恢复栈溢出）。
5. **供应链与边界缺陷精准排查结果**：
   - **JSONPath（RFC 9535）**：`jsonpath-rust 1.0.13` 存在未解决的 crates.io 发版溯源异常（[`jsonpath-rust#122`](https://github.com/besok/jsonpath-rust/issues/122)）与类型校验缺陷（`#86`）；`serde_json_path 0.7.2` 通过官方 CTS 测试，但存在末尾 `\u` panic 缺陷（[`serde_json_path#124`](https://github.com/hiltontj/serde_json_path/issues/124)）且依赖 `inventory 0.3.19`。建议 V1 采用 `serde_json_path 0.7.2` 配合入口字面量前置校验（规避 `#124`），并持续观察 `jsonpath-rfc9535` 成熟度。
   - **HTML / SQL 格式化**：HTML/XML 格式化首选 **`markup_fmt 0.28.0`**（MIT，纯 Rust，完整支持 HTML/XML 配置化排版）辅以 `quick-xml 0.42.0` 处理 XML 压缩与严格结构校验；SQL 格式化在 **`sqlformat 0.5.0`**（极简 2 依赖、久经验证但仅 3 方言）与 Deno 生态新秀 **`lax-sql 0.3.0`**（MIT、30 方言无关、支持 `Upper`/`Lower`/`Preserve` 关键字与 `Fill`/`Expanded` 风格）间存在明确取舍，推荐 V1 主用 `lax-sql 0.3.0`（或双轨对比验证）。
   - **PFX / PKCS#12 解密**：旧 `p12 0.6.3`（2022-02 停更）不支持 OpenSSL 3 默认 PBES2 AES-256 加密，完全不可用；**`p12-keystore 0.4.1`**（MIT/Apache-2.0，2.83M/90d 下载，2026-10 发布）同时覆盖现代 PBES2（HMAC-SHA256 + AES-256-CBC）与传统 PBES1（3DES-CBC / 40-bit RC2-CBC）并已修复恶意迭代次数 DoS（`#11`），唯一注意点是其传递依赖 `pkcs12 0.2.0-rc.0` 与 `cms 0.3.0-rc.1` 尚处 RC 阶段，需通过 `Cargo.lock` 锁版并跟进正式版。
6. **高度去重与轻量增量**：与 `gpui-kit v0.11.0` / `gpui-pre 0.3.14` 已有依赖树比对，**11 个核心基础 crate**（`serde`、`serde_json` 含 `preserve_order`、`indexmap`、`jiff 1.0`、`regex`、`similar`、`image 0.25`、`url`、`uuid`、`data-url`、`unicode-segmentation`、`num-traits`）可实现跨层复用；28 工具纯 Rust 计算内核相对 GPUI 桌面基线仅预计新增约 **2.5–4.5 MB** Strip 后 Release 二进制体积（开启 LTO 与 `image` / `uuid` / `jiff` feature 裁剪后）。

---

## 一、数据格式互转与格式化（JSON / YAML / TOML / XML / HTML / SQL / CSV）

本域覆盖 V1 的 **6 个核心工具**：#1 `数据格式互转`、#5 `JSON 转表格 / CSV`、#11 `JSON 格式化与压缩`、#12 `SQL 格式化`、#13 `XML / HTML 格式化`，以及 `devtools-detect` 的结构化文本探测。

### 1.1 JSON 与键序保持（`serde` + `serde_json` + `indexmap`）

| Crate | 最新稳定版（日期） | 90d 下载量 | MSRV | License | 核心评估 |
| --- | --- | ---: | --- | --- | --- |
| [`serde`](https://crates.io/crates/serde) | `1.0.229` (2026-07-18) | 465.1M | `1.56` | MIT OR Apache-2.0 | 生态基石，`gpui-kit` / `gpui-pre` 已内置。 |
| [`serde_json`](https://crates.io/crates/serde_json) | `1.0.154` (2026-10-11) | 468.4M | `1.71` | MIT OR Apache-2.0 | **`gpui-pre 0.3.14` 已启用 `features = ["preserve_order", "raw_value"]`**。 |
| [`indexmap`](https://crates.io/crates/indexmap) | `2.14.2` (2026-09-05) | 522.4M | `1.85` | Apache-2.0 OR MIT | 支撑 `serde_json/preserve_order` 与保序 Map 排序；`schemars 1.0` 已引入。 |

- **关键工程发现**：
  - `serde_json` 启用 `preserve_order` 后，`serde_json::Map` 底层从 `BTreeMap` 切换为 `IndexMap`。由于 `gpui-pre 0.3.14` 已在工作区统一激活 `serde_json/preserve_order`，**在 `devtools-tools` 中显式声明 `serde_json = { version = "1.0.154", features = ["preserve_order"] }` 零额外编译成本**，且使 #11 `JSON 格式化与压缩` 默认保留用户输入键序，仅在用户开启「键字母序排序」开关时对 `Value::Object` 递归执行 `sort_keys()`（`IndexMap::sort_keys` 或转 `BTreeMap`）。
  - #11 要求提供 2 空格 / 4 空格 / Tab 缩进与压缩（Minify）：使用 `serde_json::ser::PrettyFormatter::with_indent(b"  " | b"    " | b"\t")` 与 `serde_json::to_string` 即可零额外依赖实现；错误位置可通过 `serde_json::Error::line()` / `column()` 直接映射至 `ToolError::span`（ADR-0004）。
  - 注意：`serde_json::Value` 的大整数（超出 `i64`/`u64`）与精确小数若需在格式化时无损保留，可按需结合 `serde_json` 的 `arbitrary_precision` 评估；但由于 `arbitrary_precision` 为全局累加 Cargo feature 且会改变 `Value::Number` 序列化行为（可能影响 `gpui-kit` 内部反序列化），**V1 严禁开启 `serde_json/arbitrary_precision`**，若需数字精度保真可在格式化器中走 `serde_json::value::RawValue` 或轻量词法 Pretty-Printer。

### 1.2 YAML 生态深度对比（`serde_yaml` 归档后的纯 Rust 替代方案）

| Crate | 最新版（日期） | 90d 下载量 | MSRV | License | 底层实现与 `unsafe` | Serde 兼容 / 维护状态 | 结论 |
| --- | --- | ---: | --- | --- | --- | --- | --- |
| **[`serde-saphyr`](https://crates.io/crates/serde-saphyr)** | **`2.2.0`** (2026-11-18)<br>`3.0.0-rc.1` (2026-11-18) | **18.26M** | `1.89` | MIT OR Apache-2.0 | **100% 纯 Rust（`#![forbid(unsafe_code)]`）**，底层基于 `granit-parser`（原 `saphyr-parser` 演进分支） | 完整 Serde 双向（含多文档、锚点/别名、预算防 YAML Bomb、panic-free、行列诊断）；极活跃（2026-11-26 仍有提交） | **推荐主力（锁 `2.2.x`）** |
| [`yaml_serde`](https://crates.io/crates/yaml-serde) | `0.10.7` (2026-08-18) | 6.54M | `1.82` | MIT OR Apache-2.0 | 依赖 `libyaml-rs 0.3.0`（**`c2rust` 转译的 `unsafe` Rust**） | YAML 组织（`github.com/yaml/yaml-serde`）官方接手 `serde_yaml` 分叉，API 100% 兼容 | 备选（非安全纯 Rust，受 libyaml 1.1 语义限制） |
| [`saphyr`](https://crates.io/crates/saphyr) | `0.4.1` (2026-11-25) | 1.59M<br>(`saphyr-parser` 10.16M) | `1.85` | MIT OR Apache-2.0 | **100% 纯 Rust**（`yaml-rust2` 团队新一代 YAML 1.2 DOM/Emitter） | **无内置 Serde**，提供完整保序 YAML AST（`hashlink`）与 `YamlEmitter`；活跃 | 备选（适合无 Serde 约束的原生 YAML 格式化） |
| [`yaml-rust2`](https://crates.io/crates/yaml-rust2) | `0.13.2` (2026-11-21) | 25.49M | `1.85` | MIT OR Apache-2.0 | **100% 纯 Rust**（`yaml-rust` 社区接棒分叉） | **无 Serde**，仅维护稳定 API（新特性已转向 `saphyr`） | 仅作只读/兼容参考 |
| [`serde_yaml_ng`](https://crates.io/crates/serde_yaml_ng) | `0.10.0` (2024-05-26) | 18.55M | `1.64` | MIT | 依赖已停更的 `unsafe-libyaml 0.2.11` | 超过 2 年未发版（GitHub 最后推送 2025-09-14），事实停滞 | **排除** |
| [`serde_norway`](https://crates.io/crates/serde_norway) | `0.9.42` (2024-12-21) | 13.75M | `1.71.1` | MIT OR Apache-2.0 | 依赖 `unsafe-libyaml-norway 0.2.13` | 近 2 年未发版，仍基于 c2rust `unsafe` libyaml | **排除** |
| [`serde_yml`](https://crates.io/crates/serde_yml) | `0.0.13` (2026-05-27) | 3.80M | `1.85` | MIT OR Apache-2.0 | 曾因 `unsafe` 触发段错误 | **[`RUSTSEC-2025-0068`](https://rustsec.org/advisories/RUSTSEC-2025-0068) 标记 `unsound` 且已归档**（`0.0.13` 仅为弃用迁移垫片） | **严禁使用** |
| [`serde_yaml`](https://crates.io/crates/serde_yaml) | `0.9.34+deprecated` (2024-03-25) | 101.9M | `1.64` | MIT OR Apache-2.0 | `unsafe-libyaml` | **作者已归档弃用**（历史 [`RUSTSEC-2018-0005`](https://rustsec.org/advisories/RUSTSEC-2018-0005)） | **严禁使用** |

- **选型建议与工程细节**：
  - **推荐采用 `serde-saphyr = { version = "2.2.0", default-features = false, features = ["serialize", "deserialize"] }`**（在 `3.0.0` 正式稳定前锁定 `2.2.x`，暂不开启 `comments`/`miette`/`figment` 等重型可选特性）。
  - `serde-saphyr` 具备三个对桌面离线开发工具极为关键的特性：① `#![forbid(unsafe_code)]` 且经过 Miri 与持续 Fuzz 测试，杜绝用户输入畸形 YAML 导致进程崩溃（panic-free）；② 内置 `Budget` 限制（递归深度、别名展开倍数、总事件数），天然防「Billion Laughs」YAML 炸弹导致内存耗尽；③ 与 `serde_json::Value`（开启 `preserve_order`）无缝互转，直接支撑 #1 `JSON ↔ YAML ↔ TOML ↔ XML` 以及 `yaml format` 别名格式化。
  - **已知差异注意**：`serde-saphyr` 遵循 YAML 1.2（如 `yes`/`no`/`on`/`off` 默认按字符串而非布尔值解析，符合现代配置预期），且针对 YAML 非字符串键（如整数键 `{1: "a"}`）转 JSON 时需在转换层显式转为字符串键或返回可读 `ToolError`。

### 1.3 TOML（`toml` vs `toml_edit`）

| Crate | 最新稳定版（日期） | 90d 下载量 | MSRV | License | 核心差异与依赖 |
| --- | --- | ---: | --- | --- | --- |
| **[`toml`](https://crates.io/crates/toml)** | **`1.2.3+spec-1.1.0`** (2026-11-25) | 343.1M | `1.76` | MIT OR Apache-2.0 | 支持 TOML 1.1.0 规范；支持 `features = ["preserve_order"]`（复用 `indexmap 2`）；与 `serde_json::Value` 双向互转最简洁。 |
| [`toml_edit`](https://crates.io/crates/toml_edit) | `0.25.21+spec-1.1.0` (2026-11-25) | 310.6M | `1.76` | MIT OR Apache-2.0 | 完整保格式/保注释无损编辑 AST（CST）；底层与 `toml 1.2` 共享 `toml_parser` / `toml_writer` / `toml_datetime` / `winnow 1.0`。 |

- **选型建议**：V1 #1 `数据格式互转` 仅需 TOML ↔ JSON/YAML/XML 结构转换与美化输出，选用 **`toml = { version = "1.2.3", features = ["preserve_order"] }`** 即可（无需 `toml_edit` 的无损注释编辑复杂度）。需在工具文档/提示中显式处理 TOML 的语义边界：TOML 不存在 `null` 类型（JSON `null` 转 TOML 时提供「忽略 null 字段 / 转为 `""` / 报错」选项）且顶层必须为 Table（顶层数组需自动包裹或提示错误）。

### 1.4 XML 与 HTML 格式化（`quick-xml` / `roxmltree` / `markup_fmt` / `lax-markup`）

| Crate | 最新稳定版（日期） | 90d 下载量 | MSRV | License | 定位与关键核查点 |
| --- | --- | ---: | --- | --- | --- |
| **[`quick-xml`](https://crates.io/crates/quick-xml)** | **`0.42.0`** (2026-08-22) | 193.5M | `1.86` | MIT | 高性能流式 XML 读写器（仅强制依赖 `memchr`；`default = []`，**绝不开启 `async-tokio`**）。<br>**安全强制下限**：必须 **`>= 0.41.0`**，以修复 **[`RUSTSEC-2026-0194`](https://rustsec.org/advisories/RUSTSEC-2026-0194)**（单标签重复属性 $O(N^2)$ CPU DoS）与 **[`RUSTSEC-2026-0195`](https://rustsec.org/advisories/RUSTSEC-2026-0195)**（`NsReader` 命名空间声明无界分配 OOM）。 |
| [`roxmltree`](https://crates.io/crates/roxmltree) | `0.21.2` (2026-10-24) | 47.8M | `1.60` | MIT OR Apache-2.0 | 零依赖（除 `xmlparser`）、只读不可变 XML 1.0 DOM 树，提供精确行列号与严格合规校验，非常适合 XML → JSON/YAML/TOML 互转与 #13 严格 XML 语法校验。 |
| **[`markup_fmt`](https://crates.io/crates/markup_fmt)** | **`0.28.0`** (2026-11-16) | 65.6k | 未显式声明（edition 2024） | MIT | 纯 Rust 可配置 **HTML & XML** 美化格式化器（`g-plane/markup_fmt`，188★，7 个轻量依赖：`aho-corasick`/`anyhow`/`css_dataset`/`itertools`/`memchr`/`regex`/`tiny_pretty`），支持缩进宽度、Tab/Space、行宽、空白敏感度、注释排版与自闭合标签配置。 |
| [`lax-markup`](https://crates.io/crates/lax-markup) | `0.3.2` (2026-08-03) | 52.5k | 未显式声明（edition 2024） | MIT | Deno 核心成员 `bartlomieju/lax` 出品（驱动 `deno fmt`），仅 4 依赖（`lax-core`/`dprint-core`/`anyhow`/`serde`）；**容错极强**（不因残缺/非标准标签报错），但配置粒度较 `markup_fmt` 精简（仅 `line_width`/`use_tabs`/`indent_width`/`new_line_kind`），且较年轻（16★）。 |
| [`html5ever`](https://crates.io/crates/html5ever) / [`lol_html`](https://crates.io/crates/lol_html) | `0.40.1` / `3.0.3` | 65.5M / 2.6M | `1.85` / `1.85` | MIT OR Apache-2.0 / BSD-3-Clause | `html5ever` 为 WHATWG HTML5 解析器（注意 `gpui-base 0.11.0` 仍锁 `html5ever 0.27`，引入 `0.40` 会双份编译）且不带格式化器；`lol_html` 面向流式重写而非美化缩进。 |

- **选型建议**：
  - **XML 严格校验、压缩（Minify）与 XML ↔ JSON 互转**：采用 **`quick-xml 0.42.0`**（流式事件读写，支持 2/4/Tab 缩进与一键 Minify，且完美防 `RUSTSEC-2026-0194/0195`）+ **`roxmltree 0.21.2`**（面向 #1 XML 转 JSON/YAML/TOML 时的无歧义属性 `@attr` / 文本 `#text` DOM 映射与精确报错定位，避免 `quick-xml/serialize` 在混合节点/重复标签映射至动态 `serde_json::Value` 时的诸多限制）。
  - **HTML 格式化与压缩（#13）**：美化推荐 **`markup_fmt 0.28.0`**（纯 Rust、MIT、配置丰富、同时覆盖 HTML 与 XML 美化，且无需自研处理 `<pre>`/`<script>`/`<style>`/Void Elements 等 HTML 空白语义陷阱）；HTML 压缩可在安全保留 `<pre>`/`<textarea>`/`<script>`/`<style>` 的前提下基于轻量词法扫描实现；若实测中更看重对「残缺 HTML 片段」永不报错的宽容体验，可将 `lax-markup 0.3.2` 作为可平滑替换备选（并与 `lax-sql` 共享 `lax-core` + `dprint-core`）。

### 1.5 SQL 格式化（`sqlformat` vs `lax-sql` vs `sqruff-lib` vs `sqlparser`）

| Crate | 最新稳定版（日期） | 90d 下载量 | MSRV | License | 依赖体量 | 多方言支持与格式化能力 |
| --- | --- | ---: | --- | --- | --- | --- |
| **[`lax-sql`](https://crates.io/crates/lax-sql)** | **`0.3.0`** (2026-06-29) | 49.0k | 未显式声明（edition 2024） | MIT | **极轻（4 依赖）**：`lax-core`、`dprint-core`、`anyhow`、`serde` | **方言无关设计**：通过 sqlfluff **30 种方言 2,150 个真实用例**验证（含 PostgreSQL `$tag$`/`E'...'`、MySQL 反引号、T-SQL `[ident]`/`#temp`、占位符 `?`/`$1`/`:name`/`@var`）；永不改写/破坏业务 Token；支持 `keyword_case`（`Preserve`/`Upper`/`Lower`）、`clause_style`（`Fill`/`Expanded`）、`indent_width`/`use_tabs`/`line_width`；已用于 `deno fmt`。 |
| **[`sqlformat`](https://crates.io/crates/sqlformat)** | **`0.5.0`** (2025-10-09) | 5.32M | `1.84` | MIT OR Apache-2.0 | **极轻（2 依赖）**：`winnow 0.7`（GitHub `main` 已升 `1.0.3`）、`unicode_categories` | 移植自 `sql-formatter-plus`，`#![forbid(unsafe_code)]`；**方言枚举仅 3 种**（`Generic` / `PostgreSql` / `SQLServer`），对 MySQL 专有语法、ClickHouse、Snowflake、复杂 CTE/窗口函数存在已知排版瑕疵（34 open issues）；支持缩进、关键字大写、查询间空行与参数占位符替换。 |
| [`sqruff-lib`](https://crates.io/crates/sqruff-lib) | `0.44.0` (2026-10-30) | 720.3k | 未显式声明 | Apache-2.0 | **极重（25 直接依赖 + 方言表）**：拉入 `fancy-regex 0.19`、`getrandom 0.2`、`rayon`、`walkdir`、`ignore`、`toml 1.1` 等 | SQLFluff 的 Rust 重写，完整 AST Linter/Fixer（支持 15+ 方言），但定位是 CLI 代码规范检查器而非轻量文本格式化内核，编译耗时与二进制体积极大。 |
| [`sqlparser`](https://crates.io/crates/sqlparser) | `0.64.0` (2026-11-09) | 20.31M | 未显式声明 | Apache-2.0 | 中等（508 KB `.crate`） | Apache DataFusion SQL 解析器，支持 15+ 方言 AST，但 `Display` 仅做基础规范化输出（不保留注释、不支持复杂排版缩进、遇未支持方言语法直接解析失败）。 |
| [`sleek`](https://crates.io/crates/sleek) | `0.5.0` (2025-05-25) | 5.2k | `1.85` | MIT | 包装旧版 `sqlformat 0.3.5` + `clap` | CLI 工具封装，非库级首选。 |

- **选型建议**：
  - **推荐主选 `lax-sql 0.3.0`**（或将其与 `sqlformat 0.5.0` 在 V1 PoC 阶段用 MySQL / PostgreSQL / SQLite / T-SQL / Oracle 典型查询集对照验收后定版）。`lax-sql` 完美契合开发效率工具「粘贴任意方言 SQL 片段、保留全部注释与字面量、零误报、支持关键字大小写与紧凑/展开排版」的核心诉求；若团队更偏爱下载量基数更大的成熟 crate 且接受 3 方言覆盖，则可选 `sqlformat 0.5.0`（零风险备选）。明确排除 `sqruff-lib`（体量过重）与 `sqlparser`（丢注释且无法美化排版）。

### 1.6 CSV 与 JSON 转表格（`csv`）

- **唯一标准选型**：**[`csv 1.4.0`](https://crates.io/crates/csv)**（2025-10-17，69.5M/90d 下载，MSRV `1.73`，`Unlicense/MIT`，BurntSushi 维护，纯 Rust，完整遵循 RFC 4180）。
- 支撑 #5 `JSON 转表格 / CSV`：将 JSON 对象数组（含嵌套对象自动点号拍平如 `user.name` 或 JSON 串化）导出为标准 CSV/TSV（通过 `csv::WriterBuilder::new().delimiter(b',' | b'\t')`），同时支持 `devtools-detect` 后续扩展 CSV 识别。

---

## 二、日期、时间与 Cron

本域覆盖 V1 的 **2 个高频工具**：#2 `日期与时间戳转换`、#23 `Cron 表达式解析`。

### 2.1 `RUSTSEC-2026-0543` 核查与 `jiff` vs `chrono` + `chrono-tz` 深度权衡

#### 一手漏洞与生态现状核查
- **[`RUSTSEC-2026-0543`](https://rustsec.org/advisories/RUSTSEC-2026-0543)**（2026-09-14）：基于维护者 Dirkjan Ochtman 在 [`chronotope/chrono#1768`](https://github.com/chronotope/chrono/issues/1768)（2026-01-22 提出、2026-11-17 正式关闭）中的声明，RustSec 正式将 `chrono` 全版本标记为 **`informational = "unmaintained"`**（`patched = []`），明确建议迁移至 **`jiff`** 或 **`time`**；`chrono-tz` 同步进入软弃用。
- **`gpui-kit v0.11.0` 已完成迁移**：[`longbridge/gpui-kit#4015`](https://github.com/longbridge/gpui-kit/pull/4015)（2026-11-17 合并）已将 `gpui-base`、`gpui-component` 全部日期时间组件从 `chrono` 替换为 **`jiff = "1.0"`**（注：`Cargo.lock` 中目前仍会通过上游 Zed 的 `gpui-pre 0.3.14` 间接残留一份 `chrono 0.4`，待上游 Zed 移除；但这不影响我们在 `devtools-tools` / `devtools-detect` 中坚持零 `chrono` 红线）。
- **[`time 0.3.55`](https://crates.io/crates/time)**：虽仍活跃维护（已于 `0.3.47` 修复 [`RUSTSEC-2026-0009`](https://rustsec.org/advisories/RUSTSEC-2026-0009) RFC 2822 栈耗尽 DoS），但其**缺乏一等 IANA 时区数据库（TZDB）与 DST 感知日历算术支持**，难以满足 #2「多时区互转 + IANA 城市选择器 + 相对时间差」需求。

#### `jiff 1.0.3` vs `chrono 0.4.45` + `chrono-tz 0.10.4` 对比

| 维度 | **[`jiff 1.0.3`](https://crates.io/crates/jiff)**（推荐） | `chrono 0.4.45` + `chrono-tz 0.10.4` |
| --- | --- | --- |
| **维护与安全合规** | 极活跃（2026-11-24 发布，136.1M/90d 下载，MSRV `1.85`，`Unlicense OR MIT`），**零 RustSec 公告** | **`RUSTSEC-2026-0543` 标记 `unmaintained`**，触发 `cargo deny` 报错 |
| **与 `gpui-kit v0.11.0` 协同** | **100% 同版本去重**（`gpui-base` & `gpui-component` 已锁定 `jiff = "1.0"`），Calendar / DatePicker / TimeField 类型无缝互通 | 额外编译整套 `chrono-tz` + `chrono-tz-build`（构建期代码生成） |
| **时区数据（TZDB）打包体积** | 默认 `tzdb-bundle-platform`：**macOS / Linux 读取系统 `/usr/share/zoneinfo`（0 字节二进制增量）**；**Windows 自动内嵌 `jiff-tzdb 1.0.0`**（`concatenated-zoneinfo.dat` 实测仅 **207,040 字节 ≈ 202 KiB**，`.crate` 仅 62.8 KB）。也可开启 `tzdb-bundle-always` 保证三端离线完全一致且仅增 ~202 KiB。 | `chrono-tz 0.10.4` 在**所有平台**静态编译生成数千个 Rust 枚举与转换表（`.crate` 达 **794 KB**，编译期耗时明显增加，Release 体积显著大于 `jiff-tzdb`），且无法跟随 macOS/Linux 系统时区更新。 |
| **多格式解析与标准输出** | 原生内建 **RFC 3339 / ISO 8601 / RFC 9557（含 `[Asia/Shanghai]` IANA 注解）/ RFC 2822 / `strptime` / `strftime`**、Epoch 秒/毫秒/微秒/纳秒、人性化 `Span` 时长计算（如「3 天 2 小时前」） | 支持 RFC 3339/2822/`strptime`，缺少 RFC 9557 与内置 TZDB 感知的 `Span` 算术；若配 `dateparser 0.3.1` 同样受限于 `chrono` 停更。 |
| **API 正确性（DST / 歧义处理）** | 借鉴 TC39 Temporal，显式区分 `Timestamp`、`Zoned`、`civil::DateTime`，强制处理夏令时跳变/重叠歧义 | 历史 `LocalResult` / `NaiveDateTime` 易误用 |

- **选型建议与多格式解析策略**：
  - **锁定 `jiff = { version = "1.0.3", features = ["serde"] }`**（保留默认 `std`/`tz-system`/`tzdb-bundle-platform`/`tzdb-zoneinfo`/`tzdb-concatenated`；若需确保在精简 Linux 容器/无 `tzdata` 环境下仍可用完整 IANA 列表，可附加 `tzdb-bundle-always`——增量仅 202 KiB，性价比极高）。
  - **多格式智能解析（替代依赖 `chrono` 的 `dateparser`）**：在 `devtools-tools` 中编写约 120 行纯 `jiff` 分级解析器，按优先级依次尝试：① 纯整数/小数 Epoch（自动按位数判定秒 10 位 / 毫秒 13 位 / 微秒 16 位 / 纳秒 19 位）；② `jiff::Zoned::from_str`（RFC 9557）；③ `jiff::Timestamp::from_str`（RFC 3339 / ISO 8601）；④ `jiff::fmt::rfc2822::parse`（HTTP/Email 日期）；⑤ 常见非标日志格式 `strptime` 白名单（`%Y-%m-%d %H:%M:%S`、`%Y/%m/%d %H:%M:%S`、带毫秒/偏移等）。确定性远高于启发式正则库，且零 `chrono` 依赖。

### 2.2 Cron 表达式解析、未来 N 次触发与中英文自然语言释义

#### 调度解析与未来触发时间计算对比

| Crate | 最新稳定版（日期） | 90d 下载量 | MSRV | License | 时间库依赖 | 5/6 段与扩展语法 | 自然语言释义 |
| --- | --- | ---: | --- | --- | --- | --- | --- |
| **[`croner`](https://crates.io/crates/croner)** | **`5.0.1`** (2026-11-25) | **6.68M** | `1.85` | MIT | **可选 `jiff 1.0.1`**（关闭默认 `chrono`，**零其他必选依赖**） | **全覆盖**：5 段（POSIX/Vixie）、6 段（含秒）、7 段（含年）、`@daily` 等昵称、Quartz `L`/`W`/`#`、可选 `with_alternative_weekdays`、时区感知 `iter_from` | **内置 `describe()` + 公开 `describe::Language` Trait**（自带英文与瑞典语） |
| [`cron-parser`](https://crates.io/crates/cron-parser) | `0.13.0` (2026-11-25) | 1.54M | 未声明 | BSD-3-Clause | 强制 `jiff 1` | 标准 5 段（支持基本扩展），不支持 6 段秒级与 Quartz `L`/`W`/`#` | 无 |
| [`cronexpr`](https://crates.io/crates/cronexpr) | `1.7.2` (2026-11-16) | 18.3k | `1.88` | Apache-2.0 | 强制 **`jiff ^0.2.46`**（与 `jiff 1.0` **版本冲突双份编译**） | 支持 5/6/7 段与 `L`/`W`/`#` | 无 |
| [`cron`](https://crates.io/crates/cron) | `0.17.0` (2026-06-18) | 11.13M | 未声明 | MIT OR Apache-2.0 | **强制 `chrono ~0.4`**（违反零 `chrono` 红线） | 默认强要求 6/7 段（5 段不直接兼容） | 无 |
| [`saffron`](https://crates.io/crates/saffron) | `0.1.0` (2021-02-01) | 444.6k | 未声明 | **non-standard**（红牌） | 无 | 仅 5 段，已停更 5 年+ | 简单英文 |

#### 自然语言释义（中英文）Crate 调研与自研评估

| 方案 | 版本（日期） | 90d 下载量 | License | 依赖 | 中文支持 | 综合评估 |
| --- | --- | ---: | --- | --- | --- | --- |
| **[`croner 5.0.1`](https://crates.io/crates/croner) 内置 `English` + 自研中文 `Language` / 释义器** | `5.0.1` (2026-11-25) | 6.68M | MIT | **0 额外 crate** | **原生可控（中英文同源 AST）** | `croner` 源码 `src/describe/mod.rs` 已暴露完整 `pub trait Language`（含 `every_x_minutes`、`the_last_day_of_the_month`、`the_nth_weekday_of_the_month`、`day_of_week_names`、`month_names` 等约 30 个短语方法）。英文直接调用 `cron.describe()`；中文可先检验 `Language` Trait 的语序是否符合中文习惯（中文时间通常「大单位在前：每年 3 月、每周一至周五 09:00」，英文为「小单位在前：At 09:00, Monday through Friday, in March」），因此**基于 `croner` 已解析的 `CronPattern` / `CronComponent` 位图自研约 200–300 行中文自然语言渲染器**（配合单元测试覆盖 50+ 典型 5/6 段表达式）效果远胜硬套英文语序翻译！ |
| [`cron-descriptor`](https://crates.io/crates/cron-descriptor) | `0.1.2` (2026-07-27) | 13.2k | Apache-2.0 | 拉入 `lazy_static`、`once_cell`、`rust-i18n 1.1`（与 `gpui-kit` 的 `4.2` 重复）、`strfmt`、`thiserror 1.0` | **不支持中文**（`Cargo.toml` 仅 `["en", "es", "pt"]`） | 6★ 早期移植版，依赖老旧且无中文，**不推荐**。 |
| [`cronspeak`](https://crates.io/crates/cronspeak) | `0.1.1` (2026-06-22) | 515 | MIT OR Apache-2.0 | 零依赖 | **仅英文** | 0★ 极早期个人项目，缺乏社区验证，**不推荐**。 |
| [`cron_lingo`](https://crates.io/crates/cron_lingo) | `0.4.2` (2022-03-12) | 83 | MIT | — | 无 | 已停更 4 年+，**排除**。 |

- **最终推荐**：
  - **`croner = { version = "5.0.1", default-features = false, features = ["std", "jiff"] }`** 一站式解决 5/6/7 段解析、Quartz 扩展、时区感知未来 N 次触发时间（结合 `jiff::Zoned`）与英文自然语言释义，并基于 `croner` 解析出的结构化字段自研轻量中文释义模块（~250 LOC），实现 **零 `chrono`、零多余依赖、中英文双语地道表达**。

---

## 三、文本对比、正则与 JSONPath

本域覆盖 V1 的 **4 个核心工具**：#17 `文本对比`、#18 `JSON 语义对比`、#21 `正则表达式测试`、#22 `JSONPath 查询测试`。

### 3.1 行/词级文本对比（`similar` vs `imara-diff` vs `diffy`）

| Crate | 最新稳定版（日期） | 90d 下载量 | MSRV | License | 算法与粒度 | 性能与 `gpui-kit` 协同评估 |
| --- | --- | ---: | --- | --- | --- | --- |
| **[`similar`](https://crates.io/crates/similar)** | `3.2.1` (2026-10-24)<br>（`gpui-component 0.11.0` 使用 **`2.7`**） | 79.10M | `1.85` (`3.2.1`) | Apache-2.0 | Myers / Patience / LCS / **Histogram**（3.x）；原生支持**行级 + 词级/字符级行内 Diff（`unicode` feature）** + Unified Diff 生成 + Deadline 超时控制 | **关键发现**：`gpui-kit v0.11.0` 的虚拟化 `Diff` 视图（`crates/component/src/diff/`）**接收外部 Unified Diff / `DiffFile`，并在内部调用 `similar 2.7`（`features = ["unicode"]`）计算词级/字符级行内高亮**。<br>`similar 3.2.1` 带来 Myers 4×、Histogram 5× 提速并修复了 Histogram 栈溢出（`#105`），但引入 `3.2` 会与 `gpui-component` 的 `2.7` 双份编译（`similar` 本身仅 ~119 KB，双份开销很小）。 |
| [`imara-diff`](https://crates.io/crates/imara-diff) | `0.2.0` (2025-06-14) | 7.50M | `1.71` | Apache-2.0 | Myers / **Histogram**（源自 Git/Helix），高吞吐、低内存 | 面向行/Token 级极速 Diff，但**无内置 Unicode 词级/字素级行内 Diff 与 Unified Diff 输出辅助**，API 偏底层（需自行构造 `InternedInput` 与封装行内二次比对）。 |
| [`diffy`](https://crates.io/crates/diffy) | `0.5.2` (2026-08-31) | 6.80M | `1.85` | MIT OR Apache-2.0 | Myers，偏重 Patch 生成与应用 | 不含 Histogram/Patience 与词级高亮。 |

- **选型建议**：
  - **推荐在 `devtools-tools` 采用 `similar`**（支持 Myers + Histogram/Patience 算法切换、忽略空白/大小写预处理、Unified Diff 生成喂给 `gpui-kit` 虚拟化 `Diff` 控件，以及支持脱离 GPUI 独立输出行/词级 `ChangeTag` 供 CLI/单测复用）。
  - **版本策略**：若追求工作区绝对零重复依赖，可在 V1 先对齐 `gpui-component 0.11.0` 的 **`similar = { version = "2.7", features = ["unicode", "inline"] }`**；若需 Histogram 5× 性能与 `#105` 防栈溢出（针对万行以上大文本 Diff），则直接选用 **`similar = { version = "3.2.1", features = ["unicode", "inline"] }`**（`similar` 零必选依赖，编译增量 < 0.8s，且后续 `gpui-kit` 升级至 `similar 3` 后自动合并），推荐后者用于大文本场景并在计算循环中结合 `deadline` / `CancelFlag`。

### 3.2 结构化 JSON 语义 Diff 方案

| 方案 | 最新版（日期） | 90d 下载量 | License | 评估结论 |
| --- | --- | ---: | --- | --- |
| **自研基于 `serde_json::Value` 的递归语义 Diff 内核（~150–200 LOC）** | Workspace 内建 | — | 项目同源 | **强烈推荐**：完全匹配 #18「忽略键序与缩进格式噪声、输出结构化字段差异（`Added` / `Removed` / `Modified` / `TypeChanged`）、携带 RFC 6901 JSON Pointer 路径、支持可选数组顺序敏感/忽略集合比对、可选类型严格/宽松（如 `1` vs `1.0`）、支持 `CancelFlag` 中断大树遍历」的交互需求，**零额外依赖**，且可直接投影到 `gpui-kit` 的 `Tree` / `DataTable` / 归一化排序后喂入 `Diff` 视图。 |
| [`json-patch`](https://crates.io/crates/json-patch) | `5.0.0` (2026-11-04) | 43.38M | MIT/Apache-2.0 | 实现 RFC 6902（JSON Patch）与 RFC 7396（Merge Patch），其 `diff()` 产出的是「编辑指令序列（`add`/`remove`/`replace`）」，**不包含被替换字段的旧值（`old_value`）**，且拉入 `jsonptr`/`thiserror`；仅适合未来做 Patch 导出，不适合直接驱动并排语义对比 UI。 |
| [`json-structural-diff`](https://crates.io/crates/json-structural-diff) | `0.2.0` (2024-12-09) | 1.72M | MIT | 仅输出单个合并后的 Diff `Value`，依赖陈旧 `difflib 0.4` + `console`，表达力不足。 |
| [`assert-json-diff`](https://crates.io/crates/assert-json-diff) | `2.0.2` (2022-06-29) | 54.21M | MIT | 专为 `cargo test` 断言报错文本设计，不暴露结构化差异树。 |
| [`treediff`](https://crates.io/crates/treediff) | `5.0.0` (2024-03-27) | 4.30M | MIT/Apache-2.0 | 泛型访问者模式偏繁琐，近 2 年无更新。 |

- **工程建议**：采用 **纯 Rust 自研 `json_semantic_diff(&Value, &Value, &Options, &CancelFlag)`**，同时提供「规范化重排格式化（递归排序键后输出 Pretty JSON）」双视图：既支持结构化差异列表（按路径筛选新增/删除/修改），也能一键复用 `gpui-kit` 虚拟化 `Diff` 并排高亮呈现忽略格式噪声后的语义变更。

### 3.3 正则表达式引擎（`regex` vs `fancy-regex` vs `regress`）

| 维度 | **[`regex 1.13.2`](https://crates.io/crates/regex)** | **[`fancy-regex 0.20.2`](https://crates.io/crates/fancy-regex)** | [`regress 0.12.0`](https://crates.io/crates/regress) |
| --- | --- | --- | --- |
| **最新日期 / 90d 下载 / MSRV / License** | 2026-11-18 / 354.6M / `1.65` / MIT OR Apache-2.0 | 2026-11-11 / 117.3M / `1.66` / MIT | 2026-08-23 / 15.8M / 未声明 / MIT OR Apache-2.0 |
| **算法复杂度与抗 ReDoS** | **保证 $O(m \cdot n)$ 线性时间**（有限自动机 DFA/PikeVM），免疫指数级回溯 ReDoS | **混合架构（核心优势）**：解析 AST 后，**凡不含前后断言/反向引用的常规正则，自动委托给底层 `regex-automata` 线性时间执行**（源码 `RegexImpl::Wrap`）；仅在包含回溯特性时启用 VM（`RegexImpl::Fancy`）并由 **`backtrack_limit`**（默认 `1_000_000` 步）硬性兜底防挂死。 | 纯回溯引擎（模拟 JS/ECMA-262 语法），缺少内置线性自动机加速，超长输入性能弱于 `regex-automata`。 |
| **高级语法支持** | 不支持前后断言（`(?=...)`/`(?<=...)`）、反向引用（`\1`）、原子组 | **全支持**：正负先行/后行断言（含默认开启的 `variable-lookbehinds` 变长后行断言）、反向引用、条件分支、原子组、命名捕获组、可选 `oniguruma_mode` | 支持 JS 正则全集（含前后断言、反向引用、Unicode 属性） |
| **安全护栏与内存控制** | `RegexBuilder::size_limit`、`dfa_size_limit` | `RegexBuilder::backtrack_limit(limit)`、`delegate_size_limit`、`delegate_dfa_size_limit`；超出步数返回 `Error::RuntimeError(BacktrackLimitExceeded)` 而非死循环 | 需外部自行限长 |
| **协作取消（`CancelFlag`）** | `captures_iter` 每产出一次匹配可检查 `CancelFlag`；线性时间保证单次步进极快 | `captures_iter` 迭代间检查 `CancelFlag` + 将 `backtrack_limit` 设为合理阈值（如实时输入 `200_000`、手动模式 `1_000_000`），确保被取消后快速返回 | 同左，但无内置回溯步数硬限 |

- **选型建议（双引擎透明路由策略）**：
  - `regex 1.13.2` 已存在于 `gpui-kit` 依赖树中（且驱动 `devtools-detect` 的安全线性探测），**`devtools-detect` 必须严格仅用 `regex` 保证 ReDoS 免疫**。
  - #21 `正则表达式测试` 引入 **`fancy-regex = "0.20.2"`**（仅依赖 `regex-automata`/`regex-syntax`/`bit-set`，与 `regex` 高度共享底层 crate，几乎零额外体积）：
    - 默认提供「安全模式（Rust/RE2 线性时间，直接走 `regex`）」与「扩展模式（PCRE 风格：支持前后断言与反向引用，走 `fancy-regex`）」切换（或默认走 `fancy-regex`，利用其自动委托机制：常规模式无损享受线性时间，检测到回溯语法时在底栏状态徽章提示「回溯引擎 · 步数上限 200k」，触发 `BacktrackLimitExceeded` 时输出中文友好 `ToolError` 提示用户排查灾难性回溯）。
    - 注意：`fancy-regex 0.20.2` 覆盖了完整命名/编号捕获组字节区间（`CaptureLocations` / `Captures::get`），完美支撑多色匹配高亮与捕获组详情表。

### 3.4 RFC 9535 JSONPath 纯 Rust 实现对比

| Crate | 最新稳定版（日期） | 90d 下载量 | MSRV | License | 依赖特点 | RFC 9535 CTS 合规与实测隐患 |
| --- | --- | ---: | --- | --- | --- | --- |
| **[`serde_json_path`](https://crates.io/crates/serde_json_path)** | **`0.7.2`** (2025-02-03) | 1.19M | 未显式声明（edition 2021） | MIT | 依赖 `nom 7.1`、`serde_json`、`regex`、`thiserror`、**`inventory 0.3.19`**（用于内置函数扩展注册） | 官方 CI 持续跑 **JSONPath Compliance Test Suite (CTS)**，标准语法（滤镜、`length`/`count`/`match`/`search`/`value`、Normalized Path 输出）覆盖最全。<br>**两个一手隐患**：① 存在未发版修复的 **panic 缺陷 [`#124`](https://github.com/hiltontj/serde_json_path/issues/124)**（查询以未闭合 `\u` 结尾如 `$['\u` 会触发 `unreachable!` panic，在实时输入调试器中极易命中！）；② 依赖 `inventory`（见 ADR-0004 链接段风险；注：`gpui-pre 0.3.14` 本身已拉入 `inventory 0.3.19`，但仍须在 Windows Release LTO 下通过集成测试验证内置函数表未被裁剪）。 |
| [`jsonpath-rust`](https://crates.io/crates/jsonpath-rust) | `1.0.13` (2026-11-14) | 28.25M | 未声明 | MIT | `pest 2.7` + `pest_derive` + `regex` + `serde_json` + `thiserror` | v1.0 重写瞄准 RFC 9535 且支持 `query_with_path`；但一手核查发现 **严重供应链与规范隐患**：① **[`jsonpath-rust#122`](https://github.com/besok/jsonpath-rust/issues/122)** 曝光 **`v1.0.13` 发版提交不在 `main` 分支历史中且指向游离 commit**（供应链溯源风险红灯）；② **[`#86`](https://github.com/besok/jsonpath-rust/issues/86) / `#123`** 表明函数表达式类型校验仍有未修 RFC 9535 偏差。 |
| [`jsonpath-rfc9535`](https://crates.io/crates/jsonpath-rfc9535) | `0.1.2` (2026-10-09) | 1.4k | `1.89` | MIT OR Apache-2.0 | **极简洁**：仅 `nom 8` + `serde_json` + 可选 `regex`/`memchr`，**无 `inventory`**，`#![forbid(unsafe_code)]` + 禁用 `panic`/`unwrap` | 工程质量极高（2026-10 新星，零 panic lint 约束），但目前仅 0★ / 1.4k 下载、0.1.x 初期，生态检验不足。 |
| [`jsonpath_lib`](https://crates.io/crates/jsonpath_lib) | `0.3.0` (2021-06-03) | 859.6k | — | MIT | — | 停更 5 年+，早于 RFC 9535（2024），**排除**。 |

- **选型建议与必做护栏**：
  - **V1 推荐 `serde_json_path = "0.7.2"`**（理由：唯一经完整 RFC 9535 CTS 长期验证、无供应链异常、且原生提供 `NormalizedPath` 路径输出），**但必须落地两项强制工程护栏**：
    1. **前置拦截 `#124` Panic 陷阱**：在调用 `JsonPath::parse(expr)` 前校验字符串字面量中是否存在未满 4 位十六进制的尾部 `\u` 转义（并辅以边界单元测试 `$['\u`、`$["\u`），直接返回可恢复 `ToolError::InvalidSyntax`，确保键入过程绝不触发 panic；
    2. **Windows MSVC Release LTO 集成单测**：在 CI 中对 Release 构建断言 `match()` / `search()` / `length()` / `count()` / `value()` 五大 RFC 9535 内置函数全部可用（验证 `inventory` 符号未被裁剪）；同时密切关注 `serde_json_path` 后续版本或 `jsonpath-rfc9535` 成熟度作为长期无 `inventory` 替代。

---

## 四、安全、密码学与证书（严禁强绑系统 OpenSSL）

本域覆盖 V1 的 **5 个核心工具**：#14 `UUID / ULID 生成器`、#15 `密码与随机 Token 生成器`、#25 `JWT 解析与签发`、#26 `Hash / HMAC 与文件校验`、#27 `X.509 证书解析`。

### 4.1 纯 Rust JWT 离线签发与验签（HS / RS / PS / ES / EdDSA）

| 维度 | **[`jsonwebtoken 12.0.0`](https://crates.io/crates/jsonwebtoken) (`rust_crypto`)**（推荐） | `jsonwebtoken 12.0.0` (`aws_lc_rs`) | [`jwt-simple 0.15.1`](https://crates.io/crates/jwt-simple) | [`jwt-compact 0.8.0`](https://crates.io/crates/jwt-compact) |
| --- | --- | --- | --- | --- |
| **发布日期 / 90d 下载 / MSRV / License** | 2026-11-13 / **80.61M** / `1.88` / MIT | 同左（额外引入 `aws-lc-rs 1.19` + `aws-lc-sys`） | 2026-11-18 / 1.57M / `1.89` / ISC | 2024 / 142.5k / `1.65` / Apache-2.0 |
| **算法覆盖** | **HS256/384/512、RS256/384/512、PS256/384/512、ES256/384、EdDSA** + JWK 支持 | 同左 | HS/RS/PS/ES256/384/256K/EdDSA/ML-DSA + JWE | HS/RS/PS/ES/EdDSA |
| **是否需要 C 编译器 / 外部 SDK** | **否（100% 纯 Rust）**，三端 `cargo build` 零额外工具链 | **是**：Windows 需 CMake + NASM / Perl 或预编译绑定，且 `aws-lc-sys` 2026 年已披露 5 起 C 库 CVE（`RUSTSEC-2026-0044..0048`） | 默认纯 Rust（基于自研 `superboring 0.1.18`，内部仍用 **`rsa 0.10.0`**），可选 `boring` C 后端 | 纯 Rust，但已滞后 |
| **RSA Marvin Attack (`RUSTSEC-2023-0071`)** | **已彻底修复**：`Cargo.toml` 已升至 **`rsa = "0.10"`**（2026-11-08 正式版，`patched = [">= 0.10.0"]`） | 无受影响（AWS-LC C 实现） | 已修复（`superboring` 使用 `rsa 0.10.0`） | **仍受影响**：锁定旧版 `rsa ^0.9` 且强制依赖停更 `chrono ^0.4` |
| **依赖协同度** | **满分**：直接共享 **`base64 0.23`、`sha2 0.11`、`hmac 0.13`、`pem 4`、`zeroize 1.9`、`serde_json`** | 额外拉入整套 C 密码学库与 `aws-lc-sys` 编译耗时 | 独立体系（`coarsetime`、`ct-codecs`、`hmac-sha256`/`512`、`ed25519-compact`、`k256`、`blake2b_simd` 等 17 个必选依赖），与 RustCrypto/Hash/X.509 工具无法去重，且无通用 JWK 调试友好 API | 依赖旧 `sha2 0.10`/`hmac 0.12`/`chrono` |

- **选型建议与调试器细节**：
  - **锁定 `jsonwebtoken = { version = "12.0.0", default-features = false, features = ["rust_crypto", "use_pem"] }`**。
  - **关键优势**：无需引入 C 编译器（彻底告别 `aws-lc-sys` 在 Windows MSVC 下的 CMake/NASM 坑），已天然消灭 `RUSTSEC-2023-0071`（Marvin Attack），且所有底层密码学 crate 与 #26 Hash/HMAC 和 #27 X.509/PFX 100% 同代复用。
  - **离线 JWT 调试器（#25）开发注意**：
    - 仅解码 Header/Payload 展示 Claims 表格与过期时间（`exp`/`nbf`/`iat` 交由 `jiff::Timestamp::from_second` 格式化与倒计时）时，直接用 `base64 0.23` (`URL_SAFE_NO_PAD`) + `serde_json` 拆分三段解析，**无需密钥即可实时预览**；
    - 验签时显式允许用户按需开关 `exp`/`nbf` 校验（`Validation::validate_exp = false`），方便排查已过期令牌的签名真伪；严禁支持 `alg: "none"` 自动验签通过，须明确展示「无签名（Unsecured JWT）」警示态；
    - 密钥输入文本内存态使用 `zeroize::Zeroizing<String>` 管理，并按 ADR-0003 标记敏感输出排除系统剪贴板历史。

### 4.2 流式多算法 Hash / HMAC 与 `digest` 版本一致性

一手核查 **2026-11-27** RustCrypto 哈希生态，所有 V1 所需算法已**全员就绪 `digest 0.11.3`**，不再存在历史上 `0.10` / `0.11` 割裂或 `blake3` 掉队问题：

| Crate | 推荐版本（日期） | 90d 下载量 | MSRV | License | `digest` 版本 | 纯 Rust / 构建核查 |
| --- | --- | ---: | --- | --- | --- | --- |
| [`digest`](https://crates.io/crates/digest) | **`0.11.3`** (2026-05-03) | 463.3M | `1.85` | MIT OR Apache-2.0 | 核心 Trait | 100% 纯 Rust；提供统一 `DynDigest` / `Update` / `Mac` 流式抽象 |
| [`md-5`](https://crates.io/crates/md-5) | **`0.11.0`** (2026-03-27) | 163.7M | `1.85` | MIT OR Apache-2.0 | **`^0.11`** | 100% 纯 Rust（切忌引入 `uuid/v3` 以免拉入旧 `md-5 0.10`） |
| [`sha1`](https://crates.io/crates/sha1) | **`0.11.0`** (2026-03-27) | 247.2M | `1.85` | MIT OR Apache-2.0 | **`^0.11`** | 100% 纯 Rust（注意 crates.io 上 `0.10.7` 为后续补丁维护版，显式写 `"0.11.0"` 对齐 `p12-keystore` 与 `x509-cert`；勿与废弃的 `sha-1` 混淆） |
| [`sha2`](https://crates.io/crates/sha2) | **`0.11.2`** (2026-11-25) | 423.9M | `1.88` | MIT OR Apache-2.0 | **`^0.11`** | 100% 纯 Rust（含 SHA-256 / SHA-384 / SHA-512，支持运行时 CPU 指令集检测 `cpufeatures 0.3`） |
| [`hmac`](https://crates.io/crates/hmac) | **`0.13.0`** (2026-03-29) | 257.2M | `1.85` | MIT OR Apache-2.0 | **`^0.11.2`** | 100% 纯 Rust；同时被 `jsonwebtoken 12.0` 与 `p12-keystore 0.4` 复用 |
| [`blake3`](https://crates.io/crates/blake3) | **`1.8.8`** (2026-11-26) | 68.2M | 未声明（edition 2024） | CC0-1.0 OR Apache-2.0 OR Apache-2.0 WITH LLVM-exception | 可选 **`^0.11.2`**（`traits-preview`） | **构建核查**：`build.rs` 依赖 `cc 1.1` 在 x86_64 下优先编译手写 SIMD 汇编（性能极高且无外部 SDK 依赖），若未检测到 C 编译器**会自动平滑回退至纯 Rust 实现**；若需 100% 禁止调用 `cc` 可启用 `features = ["pure"]`（但桌面原生 Runner 均有 MSVC/Clang/GCC，保持默认即可获得 GB/s 级大文件哈希速度；同时 `blake3::Hasher` 原生支持 `new_keyed` 与 `update_reader`，无需强依赖不保 SemVer 的 `traits-preview` 即可统一封装）。 |
| [`hex`](https://crates.io/crates/hex) / [`constant_time_eq`](https://crates.io/crates/constant_time_eq) / [`zeroize`](https://crates.io/crates/zeroize) | `0.4.3` / `0.6.2` / `1.9.2` | 228M / 107M / 286M | `1.66` / `1.85` | MIT/Apache-2.0/CC0 | — | Hex 编解码、恒定时间校验值比对（防时序泄露，且已被 `blake3` 复用）、敏感密钥内存清零。 |

- **大文件流式与多算法单遍读取架构**：
  - 使用 64 KiB 栈/堆缓冲循环 `file.read(&mut buf)`，一次读取同时喂给选中的全部 `Hasher`（MD5/SHA1/SHA256/SHA512/BLAKE3），每轮检查 `CancelFlag`（即时响应取消）并按 ADR-0004 要求每 ~50 ms 节流回传已处理字节数与总字节数。

### 4.3 X.509 PEM / DER / PFX（PKCS#12）证书解析

#### X.509 解析：`x509-parser` vs `x509-cert` + `der`

| 维度 | **[`x509-parser 0.18.1`](https://crates.io/crates/x509-parser)** | **[`x509-cert 0.3.1`](https://crates.io/crates/x509-cert) + [`der 0.8.3`](https://crates.io/crates/der)** |
| --- | --- | --- |
| **发布日期 / 90d 下载 / MSRV / License** | 2026-02-05 / 79.35M / `1.67.1` / MIT OR Apache-2.0（`0.19.0-beta.1` 开发中） | **2026-11-20** / 31.72M / `1.87` / Apache-2.0 OR MIT（RustCrypto 官方） |
| **扩展解析友好度（SAN / KeyUsage / EKU / AIA / BasicConstraints）** | **极高**：内置开箱即用的 `ParsedExtension` 强类型枚举，直接提取 `SubjectAlternativeName`（DNS/IP/Email/URI）、`KeyUsage`、`ExtendedKeyUsage`、`AuthorityKeyIdentifier`、`SubjectKeyIdentifier`、`CRLDistributionPoints`、`Validity`、`oid-registry` 人类可读 OID 名称；宽容度高（可解析真实世界中轻微不规范的老旧企业证书）。 | 严格遵循 RFC 5280 DER 编解码，`const-oid 0.10`（`db` feature）提供 OID 名称，且与 `pkcs12`/`der 0.8`/`sha1 0.11` 100% 同源；但扩展字段提取略偏底层（需手工 `get_extension::<SubjectAltName>()` 逐项解码），且严格 DER 检查偶尔拒收非标老证书。 |
| **关键依赖注意** | 默认 `default = []` **纯 Rust 零 C 依赖**（切勿开启 `verify`→`ring` 或 `verify-aws`→`aws-lc-rs`！）；但必选依赖 `time 0.3`、`nom 7`、`lazy_static`；**注意 `p12-keystore 0.4.1` 自身已依赖 `x509-parser 0.18`**。 | 仅依赖 `const-oid 0.10`、`der 0.8`、`spki 0.8`（零 `time`、零 `nom`、零 `lazy_static`），体积更小。 |

#### PFX / PKCS#12 解密实证：`p12-keystore` vs `pkcs12` vs `p12`

| Crate | 最新版（日期） | 90d 下载量 | MSRV | License | 现代 PBES2（OpenSSL 3 默认 AES-256-CBC + HMAC-SHA256） | 传统 PBES1（OpenSSL 1.1 / Windows / Java 3DES & RC2-40） | 维护状态与一手实证 |
| --- | --- | ---: | --- | --- | :---: | :---: | --- |
| **[`p12-keystore`](https://crates.io/crates/p12-keystore)** | **`0.4.1`** (2026-10-13) | **2.83M** | `1.85` | MIT/Apache-2.0 | **完整支持**（基于 `pkcs5 0.8` `pbes2` + `sha2 0.11` + `hmac 0.13`） | **完整支持**（默认开启 `pbes1` feature，源码 `src/pbes1.rs` 实证实现 `TdesEde3` 3DES-CBC 与 `Rc2` 40-bit RC2-CBC + `sha1 0.11`） | **活跃维护**（2026-11-05 推送）；近期已修复恶意高迭代 DoS 与签发环（[`#11`](https://github.com/ancwrd1/p12-keystore/issues/11)）、支持 SHA-2 MAC（`#12`）与暴露底层 `Pkcs12Archive::from_pkcs12` 读取全部证书袋（`#13`）。<br>**风险提示**：① 依赖预发布 **`pkcs12 0.2.0-rc.0`** 与 **`cms 0.3.0-rc.1`**（需 `Cargo.lock` 精确锁定）；② 暂不支持极古老 MD5-based PBE 与无密码未加密 PFX。 |
| [`pkcs12`](https://crates.io/crates/pkcs12) | `0.2.0-rc.0` (2026-11-05)<br>（stable `0.1.0` 滞后） | 2.93M | `1.87` | Apache-2.0 OR MIT | 仅提供 RFC 7292 ASN.1 结构与 `kdf` 原语 | 仅原语，**需自行编排 PBES2/PBES1 解密、MAC 校验与 SafeBag 解包** | RustCrypto 官方底层库，正是 `p12-keystore 0.4.1` 的直接基石。 |
| [`p12`](https://crates.io/crates/p12) | `0.6.3` (2022-02-18) | 915.7k | `1.56` | MIT OR Apache-2.0 | **完全不支持**（`Cargo.toml` 无 `aes`/`pkcs5`/`sha2`） | 仅支持 RC2/3DES + SHA1 | **停更 4.7 年+**，依赖老旧 `yasna 0.5`/`hmac 0.12`/`cbc 0.1`，打不开任何 OpenSSL 3.x 生成的现代 `.pfx`/`.p12`，**彻底排除**。 |

- **选型建议**：
  - **推荐组合**：**`p12-keystore = "0.4.1"`**（默认 `pbes1` 开启，使用 `Pkcs12Archive::from_pkcs12` 完整提取 PFX 内所有叶证书与 CA 证书链，避免 `KeyStore` 严苛策略过滤无关联证书）+ **`x509-parser = { version = "0.18.1", default-features = false }`**（直接复用 `p12-keystore` 已有同版本依赖，零额外重复 crate，且开箱提供最完整的 Subject / Issuer / SAN / KeyUsage / EKU / 序列号 / 公钥算法与位长 / 有效期展示）+ **`pem = "4.0.0"`**（与 `jsonwebtoken 12.0` 复用，处理多证书 PEM Bundle、`.crt`/`.cer` 自动识别 PEM 与 DER）+ **`sha2 0.11` / `sha1 0.11`** 计算 SHA-256 / SHA-1 指纹。
  - 有效期判断统一转换为 `jiff::Timestamp` 与当前系统时间对比，展示「有效 / 即将过期（≤30 天）/ 已过期」直观状态徽章。

### 4.4 密码强度评估、随机数与 UUID / ULID

#### 密码与随机 Token 生成（#15）
- **加密级随机源**：**`rand = "0.10.4"`**（2026-11-15，MIT OR Apache-2.0，MSRV `1.85`）+ **`getrandom = "0.4.3"`**（与 `jsonwebtoken 12.0`、`p12-keystore 0.4`、`uuid 1.29`、`ulid 3.0` **全线统一在 `rand 0.10` / `getrandom 0.4`**，仅 `gpui-pre` 内部保留旧 `rand 0.9`）。生成密码必须走 CSPRNG（`rand::rngs::SysRng` / `ThreadRng`），使用拒绝采样（`SliceRandom` / `Uniform`）杜绝模偏差。
- **密码强度 / 熵值评估选型权衡**：
  - **[`zxcvbn 3.1.1`](https://crates.io/crates/zxcvbn)**（2026-04-24，2.81M/90d 下载，MIT，MSRV `1.66`，Dropbox zxcvbn 权威移植，内置常见密码字典、键盘路径、日期/重复/Leet 变体检测与破解次数估算）：
    - **一手重大合规发现**：核查 [`shssoichiro/zxcvbn-rs/Cargo.toml`](https://github.com/shssoichiro/zxcvbn-rs/blob/master/Cargo.toml) 发现，其 **`chrono = "0.4.38"` 仅位于 `[target.'cfg(target_arch = "wasm32")'.dependencies]`**，在 Windows / macOS / Linux 桌面端**并不会编译 `chrono`**；但 Cargo 会将所有 target 依赖写入 `Cargo.lock`，从而在启用 `cargo deny` 时触发 `RUSTSEC-2026-0543` 告警（上游已立项 [`zxcvbn-rs#100`](https://github.com/shssoichiro/zxcvbn-rs/issues/100)「Drop chrono dependency on wasm32」但尚未合并）。此外，`zxcvbn 3.1.1` 还会拉入旧版 `fancy-regex 0.18`（与 #21 的 `0.20.2` 双份编译）及 ~436 KB 词典数据，维护状态标记为 `passively-maintained`。
  - **[`passwords 3.1.19`](https://crates.io/crates/passwords)**（2026-10-18，203.8k/90d 下载，MIT，MSRV `1.85`，仅依赖 `rand 0.10`）：轻量无 `chrono`，但评分模型为简化启发式（缺乏 zxcvbn 级别的键盘邻接/字典/Leet 攻击评估与离线破解时间估算）。
  - **推荐策略**：
    - **若重视权威人类密码弱口令识别（对标 IT-Tools / Bitwarden 体验）**：选用 **`zxcvbn = { version = "3.1.1", default-features = false }`**（关闭 `builder` 省去 `derive_builder`），结合自研纯数学字符集熵值 $H = L \cdot \log_2(|\Sigma|)$ 展示（对随机生成密码，纯数学熵与 `zxcvbn` 破解时间互补），在 `deny.toml` 中对 `RUSTSEC-2026-0543` 仅限定记录说明（待 `zxcvbn-rs#100` 与 `gpui-pre` 清理 `chrono` 后收紧）；
    - **若追求 `devtools-tools` 依赖树 100% 绝对洁净（零 `chrono` 锁文件条目、零双份 `fancy-regex`）**：对随机生成的密码/Token 以**纯 Rust 香农熵 + 字符池熵值 + 常见模式扣分（重复/连号/键盘序/内置 Top-1000 弱口令表，~150 LOC）**替代，零额外依赖。**V1 推荐采用前者（`zxcvbn 3.1.1`）以保障产品级评估准确度**，因其原生桌面构建完全不编译 `chrono`，风险仅停留在 lockfile 元数据层面且可追溯至 `#100`。

#### UUID 与 ULID（#14）
| Crate | 推荐版本与精准 Feature | 90d 下载量 | MSRV | License | 避坑与去重要点 |
| --- | --- | ---: | --- | --- | --- |
| **[`uuid`](https://crates.io/crates/uuid)** | **`1.29.3`** (2026-11-17)<br>`features = ["v1", "v4", "v7", "serde"]` | 280.8M | `1.89` | Apache-2.0 OR MIT | 覆盖 V1 所需的 v1 / v4 / v7 / NIL，与 `gpui-pre` / `gpui-component` 同主版本复用。**关键裁剪**：V1 不含 v3/v5，**严禁开启 `v3`/`v5` feature**，否则会额外拉入旧版 `md-5 0.10`（与 `0.11` 冲突双份）及 `sha1_smol`。 |
| **[`ulid`](https://crates.io/crates/ulid)** | **`3.0.0`** (2026-07-16)<br>`features = ["rand", "serde"]` | 21.6M | 未声明 | MIT | 支持单调递增生成（`Generator::generate()`）、时间戳提取、与 `uuid::Uuid` 互转；仅依赖 `web-time 1`（与 `gpui-base` 复用）与 `rand 0.10`（全仓统一）。 |

---

## 五、二维码、图像处理、其余工具与工程协同

本域覆盖其余 **11 个工具**（#3 `进制转换`、#4 `颜色转换`、#6 `Base64 编解码`、#7 `URL 编解码与解析`、#8 `HTML 实体与 Unicode 编解码`、#9 `字符串转义 / 反转义`、#10 `GZip 压缩与解压`、#16 `二维码生成与解码`、#19 `文本分析与实用工具`、#20 `列表集合对比`、#24 `Chmod 权限计算器`、#28 `IPv4 / CIDR 子网计算`）以及全仓工程协同评估。

### 5.1 QR 码生成（PNG / SVG 导出 + WiFi 模板）

| Crate | 最新稳定版（日期） | 90d 下载量 | MSRV | License | 依赖与渲染能力 | 关键避坑 |
| --- | --- | ---: | --- | --- | --- | --- |
| **[`qrcode`](https://crates.io/crates/qrcode)** | **`0.15.0`** (2026-11-19) | **9.12M** | `1.85` | MIT OR Apache-2.0 | **零必选依赖**，可选 `image 0.25`（`default-features = false`）；原生支持 `svg` 与 `image::Luma<u8>` 输出（自定义前景色/背景色/静区/纠错等级 L/M/Q/H） | 最成熟稳定，与 `gpui-pre` 共享 `image 0.25`；建议配置 `default-features = false, features = ["std", "svg", "image"]`（关闭无用的 `pic`/`eps`）。 |
| [`qrcode-generator`](https://crates.io/crates/qrcode-generator) | `6.1.2` (2026-10-02) | 1.26M | `1.89` | MIT | 零必选依赖，支持标准 QR + Micro QR + rMQR，原生输出 PNG/SVG（注意其可选 `tokio` feature 严禁开启） | 现代活跃（edition 2024），如需 rMQR 可作为备选；但 API 对自定义前/背景色不如 `qrcode` 灵活。 |
| [`fast_qr`](https://crates.io/crates/fast_qr) | `0.14.0` (2026-09-01) | 94.8k | `1.59` | MIT | 支持美化圆点/Logo SVG；但 **`image` feature 强依赖 `resvg 0.45.1`**（通过光栅化 SVG 生成 PNG） | **避坑**：`gpui-pre 0.3.14` 已依赖 `resvg 0.46.0`，启用 `fast_qr/image` 会**拉入第二份重型 `resvg 0.45` 树**，显著拖慢编译与增大体积！若仅做标准 QR 导出，不推荐启用其 `image` 特性。 |

- **WiFi 配置模板**：标准格式 `WIFI:T:<WPA|WEP|nopass>;S:<SSID>;P:<PASS>;H:<true|false>;;`（转义 `\`, `;`, `,`, `:`, `"`）仅需 ~25 行纯 Rust 辅助函数，无需任何外部 crate。

### 5.2 QR 码解码（`rqrr` vs `rxing` vs `quircs`）

| 维度 | **[`rqrr 0.11.0`](https://crates.io/crates/rqrr)**（V1 推荐） | [`rxing 0.9.4`](https://crates.io/crates/rxing)（`main` 为 `0.10.0`） | [`quircs 0.10.3`](https://crates.io/crates/quircs) | [`bardecoder 0.5.0`](https://crates.io/crates/bardecoder) |
| --- | --- | --- | --- | --- |
| **发布日期 / 90d 下载 / MSRV / License** | 2026-09-02 / **1.40M** / `1.85.1` / `(MIT OR Apache-2.0) AND ISC` | 2026-11-08 / 310k / **`0.9.4` 为 `1.85`；GitHub `0.10.0` 升至 `1.95`（超 MSRV 红线！）** / Apache-2.0 | 2025-10-20 / 115k / `1.68` / MIT | 2023-07-29 / 14.7k / 未声明 / MIT |
| **依赖体量与源码尺寸** | **极轻（37.9 KB `.crate`）**：仅 2 个必选依赖（`g2p 1.0`、`lru 0.18`）+ 可选 `image`（`default-features = false`） | **极重（891.5 KB `.crate`）**：`0.9.4` **强制依赖停更 `chrono 0.4`**，且精确锁死 **`image =0.25.8`**（强迫全仓降级或冲突），默认 feature 拉入 `image/avif`（`ravif`）、`chrono-tz`、`fancy-regex 0.19`、`imageproc`、`rayon` 等海量依赖 | 轻（33.6 KB），3 依赖（`num-derive`/`num-traits`/`thiserror`），纯 Rust 移植 `quirc` | 停更 3 年+，识别率弱 |
| **识别率与抗畸变能力** | 基于 `quirc` 算法的纯 Rust 重写 + 社区长期改进（自适应阈值 + 透视变换），对**桌面截图、聊天窗口/网页粘贴图、适度倾斜/反色（预处理反转即可）与常规拍照 QR** 识别极快（毫秒级）且准确率高；但对严重模糊、极低对比度、非标圆点/强 Logo 遮挡艺术码弱于 ZXing。 | ZXing 全量 Rust 移植（`HybridBinarizer` + TryHarder 多尺度金字塔），对极端拍照/模糊/一维条码支持更强；但体量与依赖代价极大。 | 与 `rqrr` 同源 `quirc`，但下载量与维护活跃度均低于 `rqrr`（已 1 年未更新）。 | 弱 |

- **选型建议与识别率增强技巧**：
  - **V1 坚决选用 `rqrr = { version = "0.11.0", default-features = false }`**（直接向 `rqrr::PreparedImage::prepare_from_greyscale` 喂入 `image::GrayImage`，连 `rqrr/img` 特性都不必开）。
  - **零依赖识别率增强**：在解码流程中内置 3 步快速级联预处理（总耗时 < 15 ms）：① 原图灰度直解；② 失败时执行反色（覆盖深色模式白码黑底截图）；③ 失败时做一次 Otsu 全局二值化 + 添加 16px 白边静区（解决截图裁剪紧贴 QR 边缘导致无静区无法定位的高频痛点）。由此在保持 37 KB 极轻体积与零 `chrono` 洁净度的同时，覆盖桌面端 95%+ 扫码场景；`rxing` 待其 `0.10+` 彻底解除 `chrono` 且项目 MSRV 未来演进后再作为 V2 可选增强评估。

### 5.3 `image` Crate 按需 Feature 裁剪（杜绝多余格式与 C 依赖）

一手核查 [`image-rs/image/Cargo.toml`](https://github.com/image-rs/image/blob/main/Cargo.toml)（最新稳定版 **`0.25.10`**，2026-03-10，MSRV `1.88.0`，`MIT OR Apache-2.0`）：
- **默认开启的隐患**：`default = ["rayon", "default-formats"]`，而 `default-formats` 默认包含 **`avif`**（拉入重型 AV1 编码器 `ravif` + `rav1e`，显著拖慢编译时间与膨胀二进制；`gpui-base/Cargo.toml` 源码注释也专门警告避开 `rav1e`！），以及 V1 工具用不到的 `exr`、`tiff`、`hdr`、`pnm`、`ff`、`qoi`、`tga`。更严禁开启 `avif-native`（依赖 C 库 `libdav1d`）或 `nasm`（依赖外部汇编器）。
- **与 `gpui-pre 0.3.14` 的去重实证**：`gpui-pre 0.3.14` 声明 `image = { version = "^0.25.1", default-features = false, features = ["bmp", "dds", "exr", "ff", "gif", "hdr", "ico", "jpeg", "png", "pnm", "qoi", "rayon", "tga", "tiff", "webp"] }`（同样显式排除了 `avif`！）。
- **V1 `devtools-tools` 推荐最小裁剪配置**：
  ```toml
  image = { version = "0.25.10", default-features = false, features = ["png", "jpeg", "webp", "gif", "bmp", "ico"] }
  ```
  - 上述 6 个编解码后端（`png 0.18`、`zune-jpeg 0.5` + `jpeg-encoder`、`image-webp 0.2`、`gif 0.14`、内置 `bmp`/`ico`）经核实 **100% 为纯 Rust 实现、零 C 依赖**，完全覆盖 #6 `Base64 图片编解码`（含剪贴板位图转 PNG Data URI）与 #16 `QR 码 PNG 导出与任意常见图片扫码`，且在独立 `cargo test -p devtools-tools` 时编译速度极快、合入最终桌面二进制时与 `gpui-pre` 100% 同版本去重。

### 5.4 颜色空间转换（#4 `颜色转换`）

| Crate | 最新稳定版（日期） | 90d 下载量 | MSRV | License | 依赖与能力评估 |
| --- | --- | ---: | --- | --- | --- |
| **[`csscolorparser`](https://crates.io/crates/csscolorparser)** | **`0.9.2`** (2026-11-19) | 17.83M | `1.85` | MIT OR Apache-2.0 | 零分配 CSS Color 4 解析器（HEX 3/4/6/8、`rgb()`、`hsl()`、`hwb()`、`lab()`、`lch()`、`oklab()`、`oklch()`、148 个 CSS 命名色、相对颜色语法），仅依赖 `num-traits`（与 `gpui-kit` 复用）+ 可选 `phf`/`uncased`。自带 `to_css_hex()` / `to_css_rgb()` / `to_css_hsl()` / `to_hwb()` / `to_hsva()`。 |
| [`palette`](https://crates.io/crates/palette) | `0.7.7` (2026-08-02) | 29.10M | `1.71` | MIT OR Apache-2.0 | 专业级色度学库（sRGB / Linear / HSL / HSV / HWB / Lab / Oklab / Oklch / XYZ），但含过程宏 `palette_derive` 且**内置不含 CMYK**。 |
| [`color`](https://crates.io/crates/color) | `0.3.3` (2026-05-05) | 11.39M | `1.82` | Apache-2.0 OR MIT | Linebender 出品，**零必选依赖**，支持 CSS Color 4 解析与完整色彩空间数学（sRGB / HSL / HWB / Lab / Oklab / Oklch / Display P3），同样不含 CMYK。 |

- **选型建议**：V1 #4 `颜色转换` 核心需求为 **HEX / RGB / HSL / CMYK（及可选 HWB/HSV/OKLCH）双向互转与预览**。推荐仅引入轻量的 **`csscolorparser = "0.9.2"`** 负责全部 CSS 颜色字符串宽容解析与 HEX/RGB/HSL/HSV/HWB 互转，辅以 ~15 行标准设备无关公式实现 RGB ↔ CMYK 双向换算（$K = 1 - \max(R, G, B)$）与 WCAG 对比度计算，**无需额外拉入带 proc-macro 的 `palette`**（若 V2 扩展专业色域/调色板生成，可优先引入零依赖的 `linebender/color`）。

### 5.5 GZip 压缩与解压（#10 `GZip 压缩与解压`）

- **唯一推荐**：**[`flate2 1.1.11`](https://crates.io/crates/flate2)**（2026-11-06，255.3M/90d 下载，MSRV `1.67.0`，`MIT OR Apache-2.0`）。
- **纯 Rust 后端对比**：
  - **`miniz_oxide`（默认 `rust_backend`）**：`miniz_oxide 0.9.1`（`MIT OR Zlib OR Apache-2.0`）+ `crc32fast 1.2`，**100% 安全纯 Rust（无 C 编译器、无 `unsafe` 隐患）**，且已被 `png 0.18`（`image/png` 依赖）拉入工作区，**零额外依赖增量**。
  - **`zlib-rs`（`default-features = false, features = ["zlib-rs"]`）**：`zlib-rs 0.6.9`（2026-11-24，159.3M/90d 下载，`Zlib` 协议，历史 [`RUSTSEC-2024-0401`](https://rustsec.org/advisories/RUSTSEC-2024-0401) 已在 `>= 0.4.0` 修复），纯 Rust 重写但含 `unsafe` SIMD 优化，速度媲美 C `zlib-ng`。
- **选型建议**：#10 处理剪贴板文本/Base64 包装 GZip（KB–MB 级），**推荐锁定 `flate2 = { version = "1.1.11", default-features = false, features = ["miniz_oxide"] }`**（100% 复用 `png` 已有 `miniz_oxide`，零新增依赖、零 `unsafe`、零 C 编译器，并在解压侧对 `GzDecoder` 套用 `std::io::Read::take(max_output_bytes)` 严防 GZip 压缩炸弹 OOM）。

### 5.6 大整数进制转换与其余基础工具

| 工具场景 | 推荐 Crate 与版本（日期） | 90d 下载量 | MSRV | License | 替代对比与避坑说明 |
| --- | --- | ---: | --- | --- | --- |
| **#3 进制转换（2–36 + 大整数）** | **[`num-bigint 0.5.1`](https://crates.io/crates/num-bigint)** (2026-07-05) + **`num-traits 0.2.19`** | 240.0M / 338.4M | `1.60` | MIT OR Apache-2.0 | `BigInt::parse_bytes(s, radix)` 与 `to_str_radix(radix)` 原生覆盖 2–36 进制与任意精度整数（支持负号与位运算）；`num-traits` 与 `gpui-kit` 复用。<br>**红牌警告**：**[`malachite 0.15.0`](https://crates.io/crates/malachite)** 为 **`LGPL-3.0-only`** 且 MSRV **`1.100.0`**（双重违红线，严禁使用）；`dashu-int 0.6.2` 虽宽松合规但引入 4 个额外 crate，无必要替换 `num-bigint`。 |
| **#6 Base64 编解码** | **[`base64 0.23.1`](https://crates.io/crates/base64)** (2026-08-04) + **[`data-url 0.3.2`](https://crates.io/crates/data-url)** (2025-08-21) | 585.2M / 29.8M | `1.71` / `1.51` | MIT OR Apache-2.0 | `base64 0.23` 与 `jsonwebtoken 12.0` 完全对齐，覆盖 Standard / URL-safe / 无填充变体；`data-url 0.3` 与 `gpui-base 0.11.0` 完全去重，处理 `data:image/png;base64,...` 解析。 |
| **#7 URL 编解码与解析** | **[`url 2.5.9`](https://crates.io/crates/url)** (2026-11-20) + **`percent-encoding 2.3.2`** + **`form_urlencoded 1.2.3`** | 287.1M / 308.7M / 282.8M | `1.63` / `1.51` | MIT OR Apache-2.0 | **安全强制下限**：必须锁定 **`url >= 2.5.9`** 修复 **[`RUSTSEC-2026-0592`](https://rustsec.org/advisories/RUSTSEC-2026-0592)**（嵌套 `blob:` 导致 `Url::origin()` 栈溢出）。`url` 底层即使用 `percent-encoding` 与 `form_urlencoded`，支持 Query 参数表有序双向编辑且与 `gpui-pre` 去重。 |
| **#8 HTML 实体与 Unicode 编解码** | **[`html-escape 0.3.1`](https://crates.io/crates/html-escape)** (2026-11-08) | 16.28M | `1.58` | MIT | **零依赖**，支持完整 HTML5 命名/十进制/十六进制实体编解码；Unicode 码点（`U+XXXX` / `\uXXXX` / UTF-16 代理对 / UTF-8 字节）直接用纯 Rust `std` 实现（~80 LOC）。<br>**避坑**：切勿误选停更于 2017 年的旧版 `htmlescape 0.3.1`（含 `MPL-2.0`）。 |
| **#9 字符串转义 / 反转义** | **纯 Rust `std` 自研（~120 LOC）** | — | 1.92+ | 项目同源 | 覆盖 JSON / C-style / Rust / Regex / URL / Shell 等转义与反转义，可控精确报错偏移；**避免**引入 `unescaper 0.2.0`（含 `GPL-3.0-only` 双协议且功能单一）。 |
| **#19 文本分析与实用工具（12 种命名 + 统计）** | **[`convert_case 0.12.0`](https://crates.io/crates/convert_case)** (2026-08-27) + **[`unicode-segmentation 1.13.3`](https://crates.io/crates/unicode-segmentation)** + **[`deunicode 1.6.2`](https://crates.io/crates/deunicode)**（或 `slug 0.1.6`） | 183.2M / 196.3M / 21.4M | `-` / `1.85` / `1.66` | MIT / MIT OR Apache-2.0 / BSD-3-Clause | `convert_case 0.12` 仅依赖 `unicode-segmentation`（与 `gpui-kit` 去重），开箱覆盖 18+ 命名风格（camel/Pascal/snake/SCREAMING_SNAKE/kebab/COBOL/Train/Title/Flat/Alternating 等，比 `heck 0.5.0` 的 8 种更贴合 V1「12 种命名风格」）；`unicode-segmentation` 精确统计字素（Grapheme，正确计数 Emoji 与中文）、词数与行数；`deunicode`/`slug` 支撑含中文/音标字符串的 URL Slug 转换。 |
| **#20 列表集合对比 & #24 Chmod 权限计算** | **纯 Rust `std` + `indexmap`（复用）** | — | 1.92+ | 项目同源 | 零新增依赖：#20 用 `IndexSet` 实现保序交集/并集/A 独有/B 独有（支持去重、Trim、忽略大小写）；#24 用 `u16` 位掩码（`0o0000..=0o7777`，含 SUID/SGID/Sticky）完成 3×3 矩阵 ↔ 八进制 ↔ `rwxr-xr-x` ↔ `chmod 755` 双向换算。 |
| **#28 IPv4 / CIDR 子网计算** | **[`ipnet 2.12.2`](https://crates.io/crates/ipnet)** (2026-09-06) | **229.5M** | 未声明 | MIT OR Apache-2.0 | **零必选依赖**，原生支持 `Ipv4Net`、`hosts()`、`netmask()`、`hostmask()`、`broadcast()`、**`Ipv4Net::aggregate()`（将起止 IP 区间精准拆分为最小 CIDR 块集合）**，对比 `cidr 0.3.2`（8.7M）与 `ipnetwork 0.21.1`（23M，缺少内置聚合）优势明显；IP 整数/二进制/IPv6 映射（`::ffff:a.b.c.d`）用 `std::net::Ipv4Addr::to_bits()` / `to_ipv6_mapped()` 零依赖实现。 |

---

### 5.7 `devtools-detect` 轻量化依赖协同、全仓去重与编译/体积评估

#### 1. `devtools-detect` 轻量化依赖红线（落实 ADR-0003 & ADR-0004）
按 ADR-0004，`devtools-detect` 与 `devtools-tools` **同级互不依赖**，须确保探测引擎极速编译且 300 ms 硬超时内完成并发 DAG 探测。因此 **`devtools-detect` 严禁拉入任何重型工具依赖**（禁止引入 `image`、`rqrr`、`x509-parser`、`p12-keystore`、`jsonwebtoken`、`markup_fmt`、`lax-sql`、`serde-saphyr`、`zxcvbn`、`num-bigint`）：

| `devtools-detect` 允许的轻量依赖 | 用途（对应分层 DAG 探测器） | 是否零新增（与 `devtools-tools`/`gpui-kit` 共享） |
| --- | --- | --- |
| `devtools-core` | `DataType`、`Confidence`、`ParsedHint`（纯 `std`） | Workspace 内建 |
| `serde_json` | JSON / JSON Array / JWT Header&Payload 精准探测 | 共享 |
| `base64` | Base64 文本、JWT 三段、GZip（`H4sI` 魔数）探测 | 共享 |
| `url` (`>= 2.5.9`) | URL / Query 参数探测 | 共享 |
| `ipnet` | IPv4 / CIDR 探测 | 共享 |
| `jiff` (`1.0.3`) | ISO 8601 / RFC 3339 / RFC 2822 / Epoch 时间戳探测 | 共享 |
| `croner` (`std`, `jiff`) | 5/6 段 Cron 表达式结构合法性探测 | 共享 |
| `regex` (`1.13.2`) | 线性时间格式嗅探（HEX/UUID/ULID/Chmod/PEM 头部 `-----BEGIN CERTIFICATE-----`） | 共享 |
| **[`imagesize 0.15.0`](https://crates.io/crates/imagesize)**（2026-07-09，MIT，**零依赖**，24.8M/90d 下载） | **极速读取图片文件/Data URI 头部宽高与格式元数据**（O(1) 仅读前几百字节），落实 ADR-0003「探测期仅读头部元数据、> 20 MB 延迟加载」硬底线！ | 唯一轻量新增（29 KB，零传递依赖） |

> YAML / TOML / XML / HTML / SQL / X.509 / PFX 在 `devtools-detect` 中均采用**轻量启发式/魔数与语法骨架嗅探**（如 `-----BEGIN CERTIFICATE-----`、DER/PFX ASN.1 `0x30 0x82` 头部、`<?xml` / `<!DOCTYPE html>`、YAML 缩进键值特征），避免探测层承担全量解析开销。

#### 2. 与 `gpui-kit v0.11.0` / `gpui-pre 0.3.14` 依赖去重矩阵

通过一手拉取 `gpui-kit 0.11.0`、`gpui-component 0.11.0`、`gpui-base 0.11.0` 与 `gpui-pre 0.3.14` 全部正常依赖清单，确认以下核心 crate 在全仓可实现**精确主版本复用**：

| 共享 Crate | `gpui-kit` / `gpui-pre` 已有版本 | `devtools-tools` / `devtools-detect` 对齐建议 | 去重收益 |
| --- | --- | --- | --- |
| `serde` / `serde_json` | `1.0.219+` / `1.0.144+`（**已开 `preserve_order`、`raw_value`**） | `1.0.229` / `1.0.154` (`preserve_order`) | 100% 单份编译 |
| `indexmap` | `2.x`（经 `schemars` / `serde_json`） | `2.14.2` | 100% 单份编译 |
| `jiff` | **`1.0`**（`gpui-base` & `gpui-component`） | **`1.0.3`** | 100% 单份编译 + UI 日历类型互通 |
| `regex` | `1.x` | `1.13.2` | 100% 单份编译 |
| `image` | `0.25.x`（`default-features = false`，不含 `avif`） | `0.25.10`（`png`/`jpeg`/`webp`/`gif`/`bmp`/`ico`） | 100% 单份编译，避免 `ravif`/`dav1d` |
| `url` | `2.2+`（`gpui-pre`） | **`>= 2.5.9`**（强制升级 `Cargo.lock` 修复 `RUSTSEC-2026-0592`） | 100% 单份编译 + 顺带修补 UI 层漏洞 |
| `uuid` | `1.10+`（`gpui-component` / `gpui-pre`） | `1.29.3`（`v1`/`v4`/`v7`） | 100% 单份编译 |
| `data-url` | `0.3`（`gpui-base`） | `0.3.2` | 100% 单份编译 |
| `unicode-segmentation` | `1.12+`（`gpui-base` / `gpui-component`） | `1.13.3` | 100% 单份编译 |
| `num-traits` | `0.2`（`gpui-base` / `gpui-component`） | `0.2.19` | 100% 单份编译 |
| `thiserror` | `2.0.12+`（`gpui-pre`） | `2.0.x` | 100% 单份编译 |
| `similar` | `2.7`（`gpui-component`） | 对齐 `2.7`（零重复）或采用 `3.2.1`（5× Histogram 加速 + `#105` 修复，双份仅增 ~0.8s） | 权衡可控 |
| *已知跨层双版本差异（可接受，无需强行降级）* | `gpui-pre` 依赖 `rand 0.9`、`getrandom 0.3`、`chrono 0.4` | 计算内核统一在现代 **`rand 0.10`、`getrandom 0.4`、`jiff 1.0`** | 纯 Rust 轻量库双份体积 < 150 KB，且为未来 `gpui-pre` 升级铺路 |

#### 3. 整体编译耗时与二进制体积增幅评估

相比 `gpui-kit v0.11.0` + `gpui-pre 0.3.14` + 8 门 `tree-sitter-*` 桌面基线（clean Release 约 40–65 MB，全量首次编译约 2–4 分钟），**全部 28 个工具纯 Rust 计算内核的新增成本非常克制**：

- **新增外部独立 crate 数量**：剔除与 `gpui-kit` 共享的基础依赖后，`devtools-tools` 实际净增约 **28 个直接 crate**（合计约 65–80 个轻量传递 crate），**零重型 C/C++ 编译单元**（唯一含 `cc` 构建脚本的是 `blake3`，仅编译数个极小汇编/C 文件，耗时 < 2s）。
- **增量编译耗时估算**（以现代 8 核开发机估算）：
  - `devtools-tools` 独立首次 Debug 构建（`cargo test -p devtools-tools`）：约 **15–25 秒**（因完全脱离 GPUI/Win32/Metal/Vulkan，日常修改工具逻辑的增量单测可控制在 **1–3 秒**！）；
  - 对全应用首次 Release 构建的增量：约 **+20–35 秒**（其中主要耗时项为 `rsa 0.10` + `crypto-bigint`、`p256`/`p384`、`serde-saphyr`、`markup_fmt` 与 `zxcvbn` 静态词典数组）。
- **Release 二进制体积增幅估算**（配合 ADR-0001 `strip = "symbols"` + `lto = "thin"`）：
  - `serde-saphyr` + `toml` + `quick-xml` + `roxmltree` + `markup_fmt` + `lax-sql` + `csv`：约 **+0.8–1.2 MB**；
  - `jsonwebtoken 12` (`rust_crypto`) + `rsa 0.10` + `p256`/`p384` + `ed25519-dalek` + `md-5`/`sha1`/`sha2`/`hmac`/`blake3` + `p12-keystore` + `x509-parser`：约 **+1.0–1.5 MB**；
  - `zxcvbn 3.1` 词典表（约 +0.4–0.6 MB）+ `jiff-tzdb`（仅 Windows 约 +0.2 MB，macOS/Linux 为 0）+ `fancy-regex` / `serde_json_path` / `qrcode` / `rqrr` / `csscolorparser` / `num-bigint` / `croner` / 其余零依赖工具（约 +0.5–0.8 MB）；
  - **总计净增约 2.7–4.3 MB**（不足桌面 GPUI 基线的 10%），且无任何外部 DLL / `.so` / `.dylib` 运行时动态库依赖。
- **构建优化建议**：
  - 在根 `Cargo.toml` 使用 `[workspace.dependencies]` 统一锁定全部推荐版本；
  - 在 `[profile.dev.package]` 中除 ADR-0001 的 UI 核心外，可对 `rsa`、`crypto-bigint`、`blake3`、`rqrr`、`zune-jpeg`、`miniz_oxide` 开启 `opt-level = 2`（防止 Debug 模式下 RSA 2048/4096 验签与大文件/大图测试卡顿数秒）；
  - 在 `devtools-tools/Cargo.toml` 按 ADR-0004 预留 7 分类 Cargo features（默认 `all-tools` 全开），方便未来面向极简 CLI 子集按需裁剪。

---

## 六、V1 全部 28 个工具纯 Rust 依赖选型总表

严格对照 [Issue #7](https://github.com/wangneal369/IT-Tools/issues/7) 锁定的 7 大分类 28 个工具逐项映射（所有推荐项均满足 **零 GPUI、零 `tokio`、MSRV ≤ 1.89 < 1.92、宽松协议、无系统 OpenSSL**）：

| 分类 | # | 工具名称（中 / 英） | 推荐核心 Crate 与 Feature 配置 | 许可 / MSRV | 去重与工程实施备注 |
| :--- | :-: | :--- | :--- | :--- | :--- |
| **1. 转换（Converters）** | 1 | **数据格式互转**<br>JSON / YAML / TOML / XML | `serde_json = "1.0.154"` (`preserve_order`)<br>`serde-saphyr = "2.2.0"` (`default-features = false, features = ["serialize", "deserialize"]`)<br>`toml = "1.2.3"` (`preserve_order`)<br>`roxmltree = "0.21.2"` + `quick-xml = "0.42.0"` (`default-features = false`) | MIT OR Apache-2.0 / MIT<br>MSRV ≤ `1.89` | `serde_json` 与 `gpui-pre` 去重；统一以保序 `serde_json::Value` 作中间桥接模型，TOML `null`/顶层数组与 XML `@attr`/`#text` 做显式转换选项。 |
| | 2 | **日期与时间戳转换**<br>Date & Timestamp | `jiff = "1.0.3"` (`serde`，可选 `tzdb-bundle-always`) | Unlicense OR MIT<br>MSRV `1.85` | **100% 复用 `gpui-kit`**；自研 ~120 LOC 多格式分级解析（替代含 `chrono` 的 `dateparser`）；彻底规避 `RUSTSEC-2026-0543`。 |
| | 3 | **进制转换**<br>Number Base | `num-bigint = "0.5.1"` + `num-traits = "0.2.19"` | MIT OR Apache-2.0<br>MSRV `1.60` | 支持任意大整数 2–36 进制与位运算；`num-traits` 复用；严禁使用 LGPL `malachite`。 |
| | 4 | **颜色转换**<br>Color Converter | `csscolorparser = "0.9.2"` + 自研 CMYK/WCAG 公式 | MIT OR Apache-2.0<br>MSRV `1.85` | 支持 CSS Color 4（HEX/RGB/HSL/HWB/OKLCH/命名色），零分配，省去 `palette` 宏编译成本。 |
| | 5 | **JSON 转表格 / CSV**<br>JSON to Table / CSV | `serde_json = "1.0.154"` (`preserve_order`)<br>`csv = "1.4.0"` | MIT/Apache-2.0, Unlicense/MIT<br>MSRV `1.73` | 拍平嵌套对象键并同时输出结构化二维模型（供 `gpui-kit` `DataTable`）与 RFC 4180 CSV/TSV。 |
| **2. 编解码（Encoders / Decoders）** | 6 | **Base64 编解码**<br>文本 / 图片与文件 | `base64 = "0.23.1"`<br>`data-url = "0.3.2"`<br>`image = "0.25.10"` (`default-features = false, features = ["png", "jpeg", "webp", "gif", "bmp", "ico"]`) | MIT OR Apache-2.0<br>MSRV `1.88` | `base64` 与 `jsonwebtoken` 对齐；`data-url` / `image` 与 `gpui-kit` 去重；按 `MatchedDataChain` 自动切文本/图片模式。 |
| | 7 | **URL 编解码与解析**<br>URL Encoder & Parser | `url = "2.5.9"`（**强制 `>= 2.5.9`**）<br>`percent-encoding = "2.3.2"`<br>`form_urlencoded = "1.2.3"` | MIT OR Apache-2.0<br>MSRV `1.63` | 修复 `RUSTSEC-2026-0592`；与 `gpui-pre` 去重；支撑 Query 表双向编辑与逐行编解码。 |
| | 8 | **HTML 实体与 Unicode**<br>HTML & Unicode | `html-escape = "0.3.1"` + 纯 `std` Unicode 码点互转 | MIT<br>MSRV `1.58` | 零传递依赖；禁用已停更 9 年的 `htmlescape`。 |
| | 9 | **字符串转义 / 反转义**<br>Escape / Unescape | **纯 Rust `std` 自研**（~120 LOC） | 项目同源 | 零依赖，覆盖 JSON / C / Rust / Shell / Regex 转义，避开 `unescaper` 的 GPL 双协议。 |
| | 10 | **GZip 压缩与解压**<br>GZip Compress / Decompress | `flate2 = "1.1.11"` (`default-features = false, features = ["miniz_oxide"]`)<br>`base64 = "0.23.1"` | MIT OR Apache-2.0<br>MSRV `1.67` | 100% 安全纯 Rust，复用 `image/png` 的 `miniz_oxide 0.9`；解压须配 `Read::take` 防 GZip Bomb。 |
| **3. 格式化（Formatters）** | 11 | **JSON 格式化与压缩**<br>JSON Formatter | `serde_json = "1.0.154"` (`preserve_order`) + `indexmap = "2.14.2"` | MIT OR Apache-2.0<br>MSRV `1.85` | 支持 2/4/Tab 缩进、键字母序排序、Minify、行列错位 `SourceSpan` 定位。 |
| | 12 | **SQL 格式化**<br>SQL Formatter | **`lax-sql = "0.3.0"`**（备选 `sqlformat = "0.5.0"`） | MIT（备选 MIT OR Apache-2.0）<br>ed. 2024 | 方言无关（通过 30 方言 2,150 测试集），支持关键字大写/小写/保留与 `Fill`/`Expanded` 缩进；零重依赖。 |
| | 13 | **XML / HTML 格式化**<br>XML & HTML Formatter | `markup_fmt = "0.28.0"`（HTML/XML 美化）<br>`quick-xml = "0.42.0"`（**`>= 0.41.0`**，XML 压缩/严格校验）<br>（备选 `lax-markup = "0.3.2"`） | MIT<br>MSRV `1.86` | `quick-xml >= 0.41` 修复 `RUSTSEC-2026-0194/0195`；`markup_fmt` 原生兼顾 HTML5 空白敏感规则与 XML 美化。 |
| **4. 生成器（Generators）** | 14 | **UUID / ULID 生成器**<br>UUID & ULID Generator | `uuid = "1.29.3"` (`features = ["v1", "v4", "v7", "serde"]`)<br>`ulid = "3.0.0"` (`features = ["rand", "serde"]`) | Apache-2.0 OR MIT / MIT<br>MSRV `1.89` | 与 `gpui-kit` 去重 `uuid`；严禁开 `v3`/`v5` 防拉旧 `md-5 0.10`/`sha1_smol`；`ulid` 共享 `rand 0.10` / `web-time`。 |
| | 15 | **密码与随机 Token 生成器**<br>Password & Token | `rand = "0.10.4"` + `getrandom = "0.4.3"`<br>`zxcvbn = "3.1.1"` (`default-features = false`) + 自研数学熵值 | MIT OR Apache-2.0 / MIT<br>MSRV `1.85` | CSPRNG 无偏采样；`zxcvbn` 仅在 wasm32 声明 `chrono`（桌面不编译，详见风险表 `zxcvbn-rs#100`）。 |
| | 16 | **二维码生成与解码**<br>QR Code Encoder / Decoder | 生成：`qrcode = "0.15.0"` (`default-features = false, features = ["std", "svg", "image"]`) + 自研 WiFi 载荷<br>解码：`rqrr = "0.11.0"` (`default-features = false`) + `image = "0.25.10"` | MIT OR Apache-2.0 / `(MIT OR Apache-2.0) AND ISC`<br>MSRV `1.88` | 避开 `fast_qr/image` 双份 `resvg 0.45` 冲突与 `rxing 0.9.4` 的 `chrono` + `image =0.25.8` 锁死；解码内建原图→反色→白边静区 3 步预处理提升截图识别率。 |
| **5. 文本与对比（Text）** | 17 | **文本对比**<br>Text Diff | `similar = "3.2.1"` (`features = ["unicode", "inline"]`)（或对齐 `gpui-component` 用 `2.7`） | Apache-2.0<br>MSRV `1.85` | 产出 Unified Diff 与行/词级片段喂给 `gpui-kit` 虚拟化 `Diff` 视图；3.2.1 提供 5× Histogram 提速与 `#105` 防栈溢出。 |
| | 18 | **JSON 语义对比**<br>JSON Diff | **纯 Rust 自研递归语义 Diff**（基于 `serde_json::Value` + `indexmap`） | 项目同源 | 零新增依赖；忽略键序与格式噪声，输出精确 JSON Pointer 增删改与规范化双栏文本。 |
| | 19 | **文本分析与实用工具**<br>Text Analyzer & Utilities | `convert_case = "0.12.0"`<br>`unicode-segmentation = "1.13.3"`<br>`deunicode = "1.6.2"`（或 `slug = "0.1.6"`） | MIT / MIT OR Apache-2.0 / BSD-3-Clause<br>MSRV `1.85` | `unicode-segmentation` 复用；覆盖 12+ 命名转换、Unicode 字素/词/行频统计、去重排序与 LF/CRLF 转换。 |
| | 20 | **列表集合对比**<br>List Comparer | **纯 Rust `std` + `indexmap = "2.14.2"`** | 项目同源 | 零新增依赖；保序交集/并集/差集计算。 |
| **6. 调试与开发（Testers & Dev）** | 21 | **正则表达式测试**<br>Regex Tester | `regex = "1.13.2"`（安全模式 & 探测）<br>`fancy-regex = "0.20.2"`（扩展回溯模式） | MIT OR Apache-2.0 / MIT<br>MSRV `1.66` | 常规正则自动委托 `regex-automata` 线性时间执行；前后断言/反向引用启用 `backtrack_limit` + `delegate_size_limit` + `CancelFlag` 防 ReDoS。 |
| | 22 | **JSONPath 查询测试**<br>JSONPath Tester | `serde_json_path = "0.7.2"`（含前置 `\u` 护栏规避 `#124`） | MIT | RFC 9535 CTS 全覆盖，避开 `jsonpath-rust 1.0.13` 溯源异常（`#122`）；CI Release LTO 必测内置函数（防 `inventory` 裁剪）。 |
| | 23 | **Cron 表达式解析**<br>Cron Parser | `croner = "5.0.1"` (`default-features = false, features = ["std", "jiff"]`) + 自研中文释义器 | MIT<br>MSRV `1.85` | **零 `chrono`**，与 `jiff 1.0` 无缝对接，覆盖 5/6/7 段 + Quartz `L`/`W`/`#` + 英文内置 `describe()` + 同源 AST 中文地道释义。 |
| | 24 | **Chmod 权限计算器**<br>Chmod Calculator | **纯 Rust `std` 位运算自研**（~100 LOC） | 项目同源 | 零依赖；完整支持 r/w/x + SUID/SGID/Sticky 双向联动。 |
| **7. 安全与网络（Crypto & Network）** | 25 | **JWT 解析与签发**<br>JWT Debugger | `jsonwebtoken = "12.0.0"` (`default-features = false, features = ["rust_crypto", "use_pem"]`)<br>`base64 = "0.23.1"` + `jiff = "1.0.3"` + `zeroize = "1.9.2"` | MIT<br>MSRV `1.88` | **100% 纯 Rust 无 C 编译器**，内置 **`rsa 0.10.0`** 已彻底修复 `RUSTSEC-2023-0071`；覆盖 HS/RS/PS/ES/EdDSA 离线签发与验签。 |
| | 26 | **Hash / HMAC 与文件校验**<br>Hash & Checksum | `digest = "0.11.3"`、`md-5 = "0.11.0"`、`sha1 = "0.11.0"`、`sha2 = "0.11.2"`、`hmac = "0.13.0"`、`blake3 = "1.8.8"`、`hex = "0.4.3"`、`constant_time_eq = "0.6.2"` | MIT OR Apache-2.0 / CC0<br>MSRV `1.88` | 全线对齐 `digest 0.11`，与 JWT / PFX 零重复；单遍 64 KiB 分块流式读取 + `CancelFlag` + ~50 ms 节流进度；`blake3` 支持自动回退或 `pure` feature。 |
| | 27 | **X.509 证书解析**<br>Certificate Decoder | `x509-parser = "0.18.1"` (`default-features = false`)<br>`p12-keystore = "0.4.1"`（默认含 `pbes1`）<br>`pem = "4.0.0"` + `sha2 = "0.11.2"` + `sha1 = "0.11.0"` | MIT OR Apache-2.0<br>MSRV `1.85` | 同时解密现代 OpenSSL 3 PBES2 AES-256 与传统 3DES/RC2-40 PFX；严禁开启 `x509-parser` 的 `verify`/`verify-aws`；彻底淘汰停更 `p12 0.6.3`。 |
| | 28 | **IPv4 / CIDR 子网计算**<br>IPv4 Subnet & CIDR | `ipnet = "2.12.2"` + `std::net` | MIT OR Apache-2.0 | 零必选依赖；原生支持掩码/可用范围与起止 IP 聚合为最小 CIDR 块列表（`Ipv4Net::aggregate`）。 |

---

## 七、关键 RustSec、供应链、构建与许可证风险汇总

### 7.1 必须严格守门的安全、合规与供应链风险清单

| 风险级别 | 涉及 Crate / 编号 | 一手实证详情 | 项目强制应对与 `deny.toml` 守门条款 |
| :---: | --- | --- | --- |
| **高（禁入）** | `serde_yaml` / `serde_yml` (`RUSTSEC-2025-0068`) / `yaml-rust` (`RUSTSEC-2024-0320`) | `serde_yaml` 已归档；`serde_yml` 因 `unsafe` 段错误标记 `unsound` 并归档；`yaml-rust` 停更。 | `deny.toml` `bans.deny` 显式拉黑以上三者；统一使用 `serde-saphyr 2.2.0`。 |
| **高（禁入）** | `malachite 0.15.0` / `saffron 0.1.0` / `unescaper 0.2.0` | `malachite` 为 **`LGPL-3.0-only`** 且 MSRV **`1.100.0`**；`saffron` 为 `non-standard` 协议；`unescaper` 含 `GPL-3.0-only` 双协议。 | `deny.toml` `licenses.allow` 仅放行宽松 SPDX 列表（`MIT`/`Apache-2.0`/`BSD-3-Clause`/`ISC`/`Zlib`/`Unlicense`/`CC0-1.0`/`Unicode-3.0` 等），自动阻断 Copyleft 协议。 |
| **高（强制下限）** | `quick-xml < 0.41.0` (`RUSTSEC-2026-0194`, `0195`)<br>`url < 2.5.9` (`RUSTSEC-2026-0592`)<br>`rsa < 0.10.0` (`RUSTSEC-2023-0071`) | `quick-xml` 存在属性 $O(N^2)$ CPU DoS 与 `NsReader` 内存耗尽；`url <= 2.5.8` 在嵌套 `blob:` 上触发进程级栈溢出；`rsa < 0.10` 存在 Marvin Attack 时序侧信道。 | `Cargo.toml` 与 `Cargo.lock` 强制锁定 `quick-xml >= 0.42.0`、`url >= 2.5.9`、`rsa >= 0.10.0`（经 `jsonwebtoken 12.0`），CI 执行 `cargo deny check advisories` 零容忍阻断。 |
| **高（禁入）** | `p12 0.6.3` / `openssl` / `aws-lc-rs` (`RUSTSEC-2026-0044..0048`) / `ring` | `p12` 停更 4.7 年不支持 OpenSSL 3 PBES2；`openssl`/`aws-lc-sys`/`ring` 均引入 C/C++ 编译链、系统库或高频 C 层 CVE。 | `devtools-tools` / `devtools-detect` 禁止任何 `-sys` C 加密绑定；`x509-parser` 禁开 `verify`/`verify-aws`；`jsonwebtoken` 仅开 `rust_crypto`。 |
| **中高（供应链）** | `jsonpath-rust 1.0.13` (`#122`) & `serde_json_path 0.7.2` (`#124`, `inventory`) | `jsonpath-rust 1.0.13` crates.io 包无法溯源至 `main` 分支；`serde_json_path 0.7.2` 遇末尾 `\u` 会 panic 且依赖 `inventory`。 | 放弃 `jsonpath-rust 1.0.13`；选用 `serde_json_path 0.7.2` 并加前置输入护栏拦截未闭合 `\u`，同时在 Windows MSVC Release LTO CI 中验证全部内置函数可用。 |
| **中（传播放行）** | `chrono` (`RUSTSEC-2026-0543`) 间接残留 | 虽 `devtools-tools` / `devtools-detect` 自身零 `chrono`，但上游 `gpui-pre 0.3.14` 仍直接依赖 `chrono 0.4`，且 `zxcvbn 3.1.1` 在 `wasm32` target 声明 `chrono 0.4.38`（`zxcvbn-rs#100`），会导致 `Cargo.lock` 命中 `RUSTSEC-2026-0543`。 | 禁止工作区任何 crate 直接依赖 `chrono` / `chrono-tz` / `dateparser` / `cron` / `rxing 0.9.4`；在 `deny.toml` 中仅对 `RUSTSEC-2026-0543` 添加**带原因与退出条件**的临时 ignore，待 Zed 与 `zxcvbn-rs#100` 清理后立即撤除。 |
| **中（版本锁定）** | `p12-keystore 0.4.1`（传递依赖 `pkcs12 0.2.0-rc.0` / `cms 0.3.0-rc.1`） | RustCrypto `pkcs12 0.2` 与 `cms 0.3` 尚处 RC，存在后续小幅升级可能；另不支持 MD5 PBE 与未加密 PFX。 | 随仓提交 `Cargo.lock` 精确锁死版本；对 MD5 PBE / 无加密 PFX 解析失败输出清晰中文提示（告知受限算法而非报内部错误）；跟踪 `p12-keystore` 升级至正式版 `pkcs12 0.2`。 |
| **中（大输入 DoS）** | 剪贴板/文件输入防护（GZip 炸弹、YAML 炸弹、正则回溯、超大图像） | 离线桌面应用同样会遇到用户误粘贴畸形载荷导致 OOM 或主线程/后台卡死。 | ① `serde-saphyr` 保持默认 `Budget`；② `flate2` 解压强制 `Read::take(64 MiB)` 上限；③ `fancy-regex` 配置 `backtrack_limit(200_000)`；④ `devtools-detect` 使用 `imagesize` 仅读头字节，落实 ADR-0003 256 KB / 2 MB / 20 MB 截断与 ADR-0004 > 1 MB 手动运行阈值。 |

### 7.2 推荐根 `[workspace.dependencies]` 关键配置草案

```toml
[workspace.dependencies]
# --- 1. 数据格式与格式化 ---
serde = { version = "1.0.229", features = ["derive"] }
serde_json = { version = "1.0.154", features = ["preserve_order"] }
indexmap = "2.14.2"
serde-saphyr = { version = "2.2.0", default-features = false, features = ["serialize", "deserialize"] }
toml = { version = "1.2.3", features = ["preserve_order"] }
quick-xml = { version = "0.42.0", default-features = false }
roxmltree = "0.21.2"
markup_fmt = "0.28.0"
lax-sql = "0.3.0"
csv = "1.4.0"

# --- 2. 日期、时间与 Cron ---
jiff = { version = "1.0.3", features = ["serde"] }
croner = { version = "5.0.1", default-features = false, features = ["std", "jiff"] }

# --- 3. Diff、正则与 JSONPath ---
similar = { version = "3.2.1", features = ["unicode", "inline"] }
regex = "1.13.2"
fancy-regex = "0.20.2"
serde_json_path = "0.7.2"

# --- 4. 安全、密码学、证书与标识 ---
jsonwebtoken = { version = "12.0.0", default-features = false, features = ["rust_crypto", "use_pem"] }
digest = "0.11.3"
md-5 = "0.11.0"
sha1 = "0.11.0"
sha2 = "0.11.2"
hmac = "0.13.0"
blake3 = "1.8.8"
hex = "0.4.3"
constant_time_eq = "0.6.2"
zeroize = { version = "1.9.2", features = ["derive"] }
x509-parser = { version = "0.18.1", default-features = false }
p12-keystore = "0.4.1"
pem = "4.0.0"
rand = "0.10.4"
getrandom = "0.4.3"
zxcvbn = { version = "3.1.1", default-features = false }
uuid = { version = "1.29.3", features = ["v1", "v4", "v7", "serde"] }
ulid = { version = "3.0.0", features = ["rand", "serde"] }

# --- 5. 二维码、图像与基础工具 ---
qrcode = { version = "0.15.0", default-features = false, features = ["std", "svg", "image"] }
rqrr = { version = "0.11.0", default-features = false }
image = { version = "0.25.10", default-features = false, features = ["png", "jpeg", "webp", "gif", "bmp", "ico"] }
imagesize = "0.15.0"
csscolorparser = "0.9.2"
flate2 = { version = "1.1.11", default-features = false, features = ["miniz_oxide"] }
num-bigint = "0.5.1"
num-traits = "0.2.19"
base64 = "0.23.1"
data-url = "0.3.2"
url = "2.5.9"
percent-encoding = "2.3.2"
form_urlencoded = "1.2.3"
html-escape = "0.3.1"
convert_case = "0.12.0"
unicode-segmentation = "1.13.3"
deunicode = "1.6.2"
ipnet = "2.12.2"
thiserror = "2.0"
```

---

## 八、一手参考信源目录

### 8.1 RustSec Advisory Database（一手安全公告）
- `chrono` 停更公告：[RUSTSEC-2026-0543](https://github.com/rustsec/advisory-db/blob/main/crates/chrono/RUSTSEC-2026-0543.md) · 上游维护者声明：[chronotope/chrono#1768](https://github.com/chronotope/chrono/issues/1768)
- `rsa` Marvin Attack（`>= 0.10.0` 修复）：[RUSTSEC-2023-0071](https://github.com/rustsec/advisory-db/blob/main/crates/rsa/RUSTSEC-2023-0071.md) · 上游关闭跟踪：[RustCrypto/RSA#626](https://github.com/RustCrypto/RSA/issues/626)
- `quick-xml` DoS（`>= 0.41.0` 修复）：[RUSTSEC-2026-0194](https://github.com/rustsec/advisory-db/blob/main/crates/quick-xml/RUSTSEC-2026-0194.md) · [RUSTSEC-2026-0195](https://github.com/rustsec/advisory-db/blob/main/crates/quick-xml/RUSTSEC-2026-0195.md)
- `url` 栈溢出（`>= 2.5.9` 修复）：[RUSTSEC-2026-0592](https://github.com/rustsec/advisory-db/blob/main/crates/url/RUSTSEC-2026-0592.md)
- `serde_yml` Unsound 归档：[RUSTSEC-2025-0068](https://github.com/rustsec/advisory-db/blob/main/crates/serde_yml/RUSTSEC-2025-0068.md) · `serde_yaml`：[RUSTSEC-2018-0005](https://github.com/rustsec/advisory-db/blob/main/crates/serde_yaml/RUSTSEC-2018-0005.md) · `yaml-rust`：[RUSTSEC-2024-0320](https://github.com/rustsec/advisory-db/blob/main/crates/yaml-rust/RUSTSEC-2024-0320.md)
- `time` RFC 2822 DoS：[RUSTSEC-2026-0009](https://github.com/rustsec/advisory-db/blob/main/crates/time/RUSTSEC-2026-0009.md)
- `aws-lc-sys` 2026 系列漏洞：[RUSTSEC-2026-0044](https://github.com/rustsec/advisory-db/blob/main/crates/aws-lc-sys/RUSTSEC-2026-0044.md) / [0045](https://github.com/rustsec/advisory-db/blob/main/crates/aws-lc-sys/RUSTSEC-2026-0045.md) / [0046](https://github.com/rustsec/advisory-db/blob/main/crates/aws-lc-sys/RUSTSEC-2026-0046.md) / [0047](https://github.com/rustsec/advisory-db/blob/main/crates/aws-lc-sys/RUSTSEC-2026-0047.md) / [0048](https://github.com/rustsec/advisory-db/blob/main/crates/aws-lc-sys/RUSTSEC-2026-0048.md)
- `ring` 公告：[RUSTSEC-2025-0009](https://github.com/rustsec/advisory-db/blob/main/crates/ring/RUSTSEC-2025-0009.md) · [RUSTSEC-2025-0010](https://github.com/rustsec/advisory-db/blob/main/crates/ring/RUSTSEC-2025-0010.md)
- `zlib-rs` 历史修复（`>= 0.4.0`）：[RUSTSEC-2024-0401](https://github.com/rustsec/advisory-db/blob/main/crates/zlib-rs/RUSTSEC-2024-0401.md)

### 8.2 上游 GitHub 源码、PR 与 Issues（一手实证）
- **UI 层基线与去重**：[`longbridge/gpui-kit`](https://github.com/longbridge/gpui-kit)（`Cargo.toml`、`crates/base/Cargo.toml`、`crates/component/Cargo.toml`、`crates/component/src/diff/`）、[PR #4015: Replace chrono with jiff](https://github.com/longbridge/gpui-kit/pull/4015)、[`gpui-pre 0.3.14` crates.io](https://crates.io/crates/gpui-pre/0.3.14)
- **数据格式与格式化**：[`bourumir-wyngs/serde-saphyr`](https://github.com/bourumir-wyngs/serde-saphyr)、[`yaml/yaml-serde`](https://github.com/yaml/yaml-serde)、[`yaml/libyaml-rs`](https://github.com/yaml/libyaml-rs)、[`saphyr-rs/saphyr`](https://github.com/saphyr-rs/saphyr)、[`dtolnay/serde-yaml`](https://github.com/dtolnay/serde-yaml)、[`toml-rs/toml`](https://github.com/toml-rs/toml)、[`tafia/quick-xml`](https://github.com/tafia/quick-xml)、[`g-plane/markup_fmt`](https://github.com/g-plane/markup_fmt)、[`bartlomieju/lax`](https://github.com/bartlomieju/lax)（`lax-sql` / `lax-markup`）、[`shssoichiro/sqlformat-rs`](https://github.com/shssoichiro/sqlformat-rs)、[`quarylabs/sqruff`](https://github.com/quarylabs/sqruff)
- **日期、时间与 Cron**：[`BurntSushi/jiff`](https://github.com/BurntSushi/jiff)（`COMPARE.md`、`crates/jiff-tzdb/concatenated-zoneinfo.dat`）、[`Hexagon/croner-rust`](https://github.com/Hexagon/croner-rust)（`src/describe/mod.rs`）、[`cflockhart/cron-descriptor-rust`](https://github.com/cflockhart/cron-descriptor-rust)、[`trananhtung/cronspeak`](https://github.com/trananhtung/cronspeak)
- **Diff、正则与 JSONPath**：[`mitsuhiko/similar`](https://github.com/mitsuhiko/similar)（`CHANGELOG.md` 3.2.1）、[`pascalkuthe/imara-diff`](https://github.com/pascalkuthe/imara-diff)、[`fancy-regex/fancy-regex`](https://github.com/fancy-regex/fancy-regex)（`src/lib.rs` `RegexBuilder`）、[`hiltontj/serde_json_path`](https://github.com/hiltontj/serde_json_path)（[`#124`](https://github.com/hiltontj/serde_json_path/issues/124)）、[`besok/jsonpath-rust`](https://github.com/besok/jsonpath-rust)（[`#122`](https://github.com/besok/jsonpath-rust/issues/122)、[`#86`](https://github.com/besok/jsonpath-rust/issues/86)）、[`joshuaclayton/jsonpath-rfc9535`](https://github.com/joshuaclayton/jsonpath-rfc9535)、[RFC 9535: JSONPath](https://www.rfc-editor.org/rfc/rfc9535.html)
- **安全、密码学与证书**：[`Keats/jsonwebtoken`](https://github.com/Keats/jsonwebtoken)（`Cargo.toml`、`src/crypto/rust_crypto/mod.rs`）、[`jedisct1/rust-jwt-simple`](https://github.com/jedisct1/rust-jwt-simple)、[`jedisct1/rust-superboring`](https://github.com/jedisct1/rust-superboring)、[`RustCrypto/RSA`](https://github.com/RustCrypto/RSA)、[`RustCrypto/hashes`](https://github.com/RustCrypto/hashes)、[`BLAKE3-team/BLAKE3`](https://github.com/BLAKE3-team/BLAKE3)（`Cargo.toml`、`build.rs`）、[`rusticata/x509-parser`](https://github.com/rusticata/x509-parser)、[`RustCrypto/formats`](https://github.com/RustCrypto/formats)（`x509-cert`、`pkcs12`、`der`）、[`ancwrd1/p12-keystore`](https://github.com/ancwrd1/p12-keystore)（`src/pbes1.rs`、`src/keystore.rs`、[`#11`](https://github.com/ancwrd1/p12-keystore/issues/11)）、[`hjiayz/p12`](https://github.com/hjiayz/p12)、[`shssoichiro/zxcvbn-rs`](https://github.com/shssoichiro/zxcvbn-rs)（[`#100`](https://github.com/shssoichiro/zxcvbn-rs/issues/100)）
- **QR、图像与基础工具**：[`kennytm/qrcode-rust`](https://github.com/kennytm/qrcode-rust)、[`ErwanVIVIEN/fast_qr`](https://github.com/erwanvivien/fast_qr)、[`magiclen/qrcode-generator`](https://github.com/magiclen/qrcode-generator)、[`WanzenBug/rqrr`](https://github.com/WanzenBug/rqrr)、[`rxing-core/rxing`](https://github.com/rxing-core/rxing)、[`image-rs/image`](https://github.com/image-rs/image)、[`rust-lang/flate2-rs`](https://github.com/rust-lang/flate2-rs)、[`mazznoer/csscolorparser-rs`](https://github.com/mazznoer/csscolorparser-rs)、[`Ogeon/palette`](https://github.com/Ogeon/palette)、[`linebender/color`](https://github.com/linebender/color)、[`rust-num/num-bigint`](https://github.com/rust-num/num-bigint)
