# M5 三端真机发版验收清单

Issue [#24](https://github.com/wangneal369/IT-Tools/issues/24) 的人工 Gate。自动化部分见
`.github/workflows/release.yml` 与 `scripts/verify-release-windows.ps1`；资源红线定义见
[v1-spec §3.3](../specs/v1-spec.md)；Linux 剪贴板已知上游限制见
[m3-clipboard-validation](m3-clipboard-validation.md)。

## 0. 验收前置

| 项 | 要求 |
| :--- | :--- |
| 构建 | 同一 commit，`tag` 或 `workflow_dispatch` 产出的 Release 包 |
| 记录 | 每条记录 OS 版本、硬件、包 SHA-256、实测值/截图路径 |
| 判定 | `PASS` / `FAIL` / `ACCEPTED`（FAIL 必须附 issue；ACCEPTED 需注明理由与版本范围） |
| 隐私 | 全程断网或抓包确认零外联；退出后确认无工具载荷、剪贴板正文落盘 |

## 1. Windows 10/11 x86_64

| ID | 项目 | 操作与期望 | 结果 |
| :--- | :--- | :--- | :--- |
| W1 | `#12` 资源红线 | 按 §3.3 采集：热待机 CPU `≤ 1.0%`、RSS `≤ 100 MB`；Headless CPU `≤ 0.1%`、Private `≤ 40 MB`/RSS `≤ 65 MB`、GPU `0 MB`；`--silent` 冷启 CPU `≤ 0.1%`、Private `≤ 20 MB`/RSS `≤ 35 MB`。任务管理器/GPU-Z 各稳定 60s 后读数 | |
| W2 | 唤醒延迟 | 热待机唤醒至首帧可交互 P95 `≤ 100 ms`；Headless `≤ 220 ms`；IPC 端到端 `≤ 280 ms`。至少 20 次采样 | |
| W3 | UIPI 容错 | 非管理员运行 DevTools，前台为管理员窗口时按 `Ctrl+Alt+Space`、托盘切换：不崩溃、不假死，失败时有可理解提示；`AllowSetForegroundWindow` 路径正常 | |
| W4 | 微软拼音 CJK IME | 在工具输入框输入长句并切换候选，组合态不丢字、候选窗跟随光标；`Esc` 先退 IME/浮层再收窗，不误关标签 | |
| W5 | 密码管理器零读取 | 从 1Password/Bitwarden 复制密码（含 `ExcludeClipboardContentFromMonitorProcessing` / `CanIncludeInClipboardHistory=0`）：无检测提示、无预览、无历史 | |
| W6 | 敏感复制排除历史 | 密码生成器、JWT 输出复制后按 `Win+V` 不出现；普通文本正常进入历史 | |
| W7 | 托盘/热键/单实例 | 托盘左键智能切换、右键 6 组菜单；重复启动仅唤起原实例；`--toggle`/`--show`/`--silent`/`--quit-running` 行为符合 §3.1 | |
| W8 | 有序退出 | 托盘「退出」、`WM_ENDSESSION`：后台任务取消、托盘/热键先隐藏再销毁、进程归零，无残留 `devtools.exe` | |
| W9 | Release LTO 本机 | `pwsh -File scripts/verify-release-windows.ps1` 全绿；`release_lto_jsonpath` 8 项通过；exe `≤ 45 MiB`、zip `≤ 22 MiB` | |

## 2. macOS 14 / 15.4+ / 26（aarch64）

| ID | 项目 | 操作与期望 | 结果 |
| :--- | :--- | :--- | :--- |
| M1 | 后台热键抢焦 | 其他 App 全屏/前台时 `Control+Option+Space` 唤起并聚焦，不抢输入框焦点；重复触发幂等 | |
| M2 | `NSPasteboard` 拒权降级 | macOS 15.4+/26 首次读取：拒绝系统授权后不崩溃、不弹二次窗；显式智能粘贴（工作台 `Ctrl/Cmd+Shift+V`、首页 `Ctrl/Cmd+V`）可用；拒绝态下自动检测静默关闭 | |
| M3 | `SMAppService` 静默启动 | 签名 `.app` 注册登录项后重启：`--silent` 进 Headless，零闪窗、Dock 无闪现；登录项可在系统设置中关闭且与 App 状态一致。ad-hoc 签名包需记录用户手动放行步骤 | |
| M4 | Dock 三态 | Regular / Accessory / 隐藏三态切换瞬时无残影；窗口关闭后按配置进入热待机或 Headless | |
| M5 | 原生中文 IME | 系统拼音输入法组合态、候选窗定位、`Esc` 三层退避正确；`Cmd+W` 关最后标签回首页 | |
| M6 | 剪贴板私有标记 | 敏感复制附 `ConcealedType`/`TransientType`；从钥匙串/1Password 复制受保护内容零读取 | |
| M7 | 产物完整性 | `.app` 的 `CFBundleIdentifier = io.github.wangneal369.devtools`、`LSUIElement = false`；`.dmg` 可挂载；`.tar.gz` 可解包启动 | |

## 3. Linux x86_64（X11 + Wayland：GNOME / KDE / Sway）

| ID | 项目 | 操作与期望 | 结果 |
| :--- | :--- | :--- | :--- |
| L1 | X11 剪贴板 | 文件/图片/文本优先级正确；`TARGETS` 与 KDE 隐藏提示先于正文；INCR 大载荷拒绝读取且不卡死；换选区 owner 后行为正确 | |
| L2 | Wayland `wl-paste` | 已安装时 MIME/隐藏提示先查后读、300 ms 超时生效；未安装时返回「不可读取」安全失败，不回落 XWayland 未校验读取 | |
| L3 | GNOME Wayland 降级 | Mutter 无 data-control 时获焦自动读取安全失败、零提示；显式智能粘贴同样安全失败并给出说明（上游限制，见 M3 文档）。记录 GNOME 版本 | |
| L4 | KDE secret 标记 | 敏感输出写入后 `x-kde-passwordManagerHint=secret` 存在；Klipper 不记录；密码管理器内容零读取 | |
| L5 | `ashpd` Portal 热键 | GNOME/KDE Wayland 首次授权后热键可用、拒绝后提示清晰；Wayland 下不启用 `global-hotkey` | |
| L6 | Sway `--toggle` | 绑定 `exec devtools --toggle` 可智能切换；无 SNI 宿主时关窗按配置直接退出，无孤儿进程 | |
| L7 | 托盘降级 | KDE StatusNotifier 正常；GNOME 无 SNI：菜单不可用但应用不崩溃，退出路径可用（托盘「退出」或 `--quit-running`） | |
| L8 | 持选区暂缓 Headless | 本应用持有剪贴板选区且无剪贴板管理器接管时，60s 后不切 Headless；被接管后正常切换 | |
| L9 | Vulkan + IME | `vulkaninfo` 可用；GPU 路径无黑屏；Fcitx5 与 IBus 中文组合态、候选窗、`Esc` 三层退避正常 | |
| L10 | 产物完整性 | `.tar.gz` 内 `devtools`、`install.sh`、`io.github.wangneal369.devtools.desktop`、同名 SVG；`install.sh` 安装后桌面项可启动、图标显示 | |

## 4. 跨端通用

| ID | 项目 | 期望 | 结果 |
| :--- | :--- | :--- | :--- |
| C1 | 离线与零遥测 | 断网全功能可用；抓包/防火墙日志无主动外联 | |
| C2 | 零载荷落盘 | `config.toml` / `window-state.toml` 仅偏好与几何；无工具输入/输出、JWT、私钥、剪贴板正文 | |
| C3 | 配置损坏回退 | 破坏 `config.toml` 后冷启动生成 `*.invalid-<timestamp>` 并以默认值启动，一次性提示 | |
| C4 | 体积红线 | 三端二进制 `≤ 45 MiB`、压缩包 `≤ 22 MiB`；`SHA256SUMS.txt` 与产物一致 | |

## 5. 签署

| 平台 | 验收人 | 日期 | 版本 / commit | 结论 | 缺陷 issue |
| :--- | :--- | :--- | :--- | :--- | :--- |
| Windows | | | | | |
| macOS 14 | | | | | |
| macOS 15.4+/26 | | | | | |
| Linux X11 | | | | | |
| Linux Wayland GNOME | | | | | |
| Linux Wayland KDE | | | | | |
| Linux Wayland Sway | | | | | |

全部条目 `PASS`，或 `ACCEPTED` 已链接 issue 并注明版本范围，方可发布 `v1.0.0`。
