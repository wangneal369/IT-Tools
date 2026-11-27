//! DevTools 桌面入口（薄，ADR-0004）。
//!
//! Responsibilities, in order:
//! 1. Parse the shell-control flags (`--silent` / `--toggle` / `--show` /
//!    `--settings` / `--quit-running`).
//! 2. Take the per-user single-instance socket **before** touching GPUI. If a
//!    primary instance already runs, forward one command and exit 0.
//! 3. Load `config.toml` / `window-state.toml` through the platform store.
//! 4. Hand everything to `devtools-ui`, which runs the GPUI application.

use devtools_core::{AppSettings, WindowState};
use devtools_platform::{
    ConfigPaths, ConfigStore, Instance, IpcCommand, IpcName, LoadOutcome, ipc,
};

/// Reverse-DNS base; `IpcName` appends the current user.
const IPC_BASE: &str = "io.github.wangneal369.devtools";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Request {
    /// No args: open the workbench.
    Show,
    /// `--toggle`: smart toggle (also the Sway/wlroots fallback).
    Toggle,
    /// `--show`: idempotent raise.
    ShowExplicit,
    /// `--settings`: raise with the settings drawer open.
    Settings,
    /// `--silent`: no windows at startup.
    Silent,
    /// `--quit-running`: ask a running instance to quit.
    QuitRunning,
}

impl Request {
    fn ipc_command(self) -> IpcCommand {
        match self {
            Self::Show | Self::ShowExplicit => IpcCommand::Show,
            Self::Toggle => IpcCommand::Toggle,
            Self::Settings => IpcCommand::OpenSettings,
            Self::Silent => IpcCommand::Toggle,
            Self::QuitRunning => IpcCommand::Quit,
        }
    }
}

fn main() {
    let request = match parse_args() {
        Ok(Some(request)) => request,
        Ok(None) => return,
        Err(message) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
    };

    let ipc_name = IpcName::user_scoped(IPC_BASE);

    // Single-instance check FIRST: this must never initialize GPUI.
    let instance = match ipc::acquire(&ipc_name) {
        Ok(Instance::Primary(guard)) => {
            // No other instance exists. `--quit-running` is a clean no-op.
            if request == Request::QuitRunning {
                return;
            }
            Instance::Primary(guard)
        }
        Ok(Instance::Existing) => {
            if request == Request::Silent {
                // Per contract: --silent on an existing instance exits silently.
                return;
            }
            if let Err(err) = ipc::send(&ipc_name, request.ipc_command()) {
                eprintln!("[devtools] 无法转发命令到已运行实例：{err}");
            }
            return;
        }
        Err(err) => {
            eprintln!("[devtools] 单实例 IPC 初始化失败：{err}");
            std::process::exit(1);
        }
    };

    let paths = ConfigPaths::standard().unwrap_or_else(|| {
        ConfigPaths::from_dir(std::env::temp_dir().join("io.github.wangneal369.devtools"))
    });
    let store = ConfigStore::new(paths);
    let (settings, settings_notice) = load_settings(&store);
    let window_state = load_window_state(&store);

    let start = match request {
        Request::Silent => devtools_ui::StartMode::Silent,
        Request::Settings => devtools_ui::StartMode::ShowSettings,
        Request::Show | Request::ShowExplicit | Request::Toggle => {
            devtools_ui::StartMode::ShowWindow
        }
        Request::QuitRunning => unreachable!("handled above"),
    };

    devtools_ui::run(devtools_ui::AppBoot {
        store,
        settings,
        settings_notice,
        window_state,
        instance,
        start,
    });
}

/// Returns `Ok(None)` when help was printed.
fn parse_args() -> Result<Option<Request>, String> {
    let mut request = Request::Show;
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--silent" => request = Request::Silent,
            "--toggle" => request = Request::Toggle,
            "--show" => request = Request::ShowExplicit,
            "--settings" => request = Request::Settings,
            "--quit-running" => request = Request::QuitRunning,
            "-h" | "--help" => {
                println!("{USAGE}");
                return Ok(None);
            }
            other => return Err(format!("未知参数 `{other}`\n\n{USAGE}")),
        }
    }
    Ok(Some(request))
}

const USAGE: &str = "\
DevTools — 离线开发效率工具集

用法: devtools [选项]

选项:
  (无)            打开主工作台（已有实例则唤起）
  --show          幂等唤起并聚焦主工作台
  --toggle        智能切换：未在前台时唤起，已在前台时收起到托盘
  --settings      唤起主工作台并打开设置
  --silent        静默启动：不创建窗口，仅驻留托盘 / 热键 / 单实例 IPC
  --quit-running  请求已运行的实例有序退出（无实例时直接退出）
  -h, --help      显示本帮助";

fn load_settings(store: &ConfigStore) -> (AppSettings, Option<String>) {
    match store.load_settings() {
        Ok(LoadOutcome::Loaded(settings)) => (settings, None),
        Ok(LoadOutcome::NotFoundDefault(settings)) => (settings, None),
        Ok(LoadOutcome::RecoveredFromInvalid {
            value,
            backup_path,
            reason,
        }) => (
            value,
            Some(format!(
                "配置文件损坏，已备份到 {} 并回退默认配置（{reason}）",
                backup_path.display()
            )),
        ),
        Ok(LoadOutcome::UnsupportedFutureVersion {
            found,
            supported,
            path,
        }) => (
            AppSettings::default(),
            Some(format!(
                "配置文件 {} 的版本 {found} 高于支持的 {supported}，请升级应用；本次以默认配置启动",
                path.display()
            )),
        ),
        Err(err) => (
            AppSettings::default(),
            Some(format!("读取配置失败，使用默认配置：{err}")),
        ),
    }
}

fn load_window_state(store: &ConfigStore) -> WindowState {
    match store.load_window_state() {
        Ok(outcome) => outcome.into_value().unwrap_or_default(),
        Err(err) => {
            eprintln!("[devtools] 读取窗口状态失败，使用默认值：{err}");
            WindowState::default()
        }
    }
}
