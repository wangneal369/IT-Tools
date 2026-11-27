//! Assembles the OS integration surfaces into one GPUI-free service.
//!
//! `PlatformService::start` runs on the UI main thread (tray icons require it)
//! and owns the tray, global hotkey, autostart adapter and IPC server. Every
//! native event becomes a [`PlatformCommand`] on the shared channel; the
//! workbench drains that channel on its own executor.

use async_channel::Receiver;

use crate::autostart::AutostartAdapter;
use crate::bridge::{PlatformBridge, PlatformCommand};
use crate::hotkey::{GlobalHotkey, HotkeyStatus};
use crate::ipc::{Instance, IpcCommand, IpcServer};
use crate::tray::{TrayError, TrayHandle};

/// Everything the platform layer needs from startup.
pub struct PlatformStartup {
    /// Already-acquired single-instance state (the check runs before GPUI).
    pub instance: Instance,
    /// Shortcut from `AppSettings.hotkeys.global_toggle`.
    pub global_shortcut: Option<String>,
}

/// All OS integrations for one running application.
pub struct PlatformService {
    commands: Option<Receiver<PlatformCommand>>,
    hotkey: GlobalHotkey,
    autostart: AutostartAdapter,
    tray: Option<TrayHandle>,
    tray_error: Option<String>,
    ipc: Option<IpcServer>,
}

impl PlatformService {
    /// Build the tray, register the hotkey and serve IPC commands.
    pub fn start(startup: PlatformStartup) -> Self {
        let mut bridge = PlatformBridge::new();
        let sender = bridge.sender();

        let (tray, tray_error) = match crate::tray::build(sender.clone()) {
            Ok(tray) => (Some(tray), None),
            Err(err) => {
                eprintln!("[devtools] system tray unavailable: {err}");
                (None, Some(tray_error_message(&err)))
            }
        };

        let mut hotkey = GlobalHotkey::new(sender.clone());
        let _ = hotkey.apply(startup.global_shortcut.as_deref());

        let ipc = match startup.instance {
            Instance::Primary(guard) => {
                let sender = sender.clone();
                Some(crate::ipc::serve(guard, move |command| {
                    let _ = sender.try_send(command_from_ipc(command));
                }))
            }
            Instance::Existing => None,
        };

        Self {
            commands: bridge.take_receiver(),
            hotkey,
            autostart: AutostartAdapter::for_current_exe(),
            tray,
            tray_error,
            ipc,
        }
    }

    /// Take the single command consumer. Returns `None` after the first call.
    pub fn take_commands(&mut self) -> Option<Receiver<PlatformCommand>> {
        self.commands.take()
    }

    pub fn hotkey_status(&self) -> HotkeyStatus {
        self.hotkey.status().clone()
    }

    pub fn hotkey_last_error(&self) -> Option<&str> {
        self.hotkey.last_error()
    }

    /// Live re-register; returns the observed status (may be `Degraded`).
    pub fn apply_hotkey(&mut self, shortcut: Option<&str>) -> HotkeyStatus {
        self.hotkey.apply(shortcut)
    }

    pub fn autostart(&self) -> &AutostartAdapter {
        &self.autostart
    }

    /// Whether a tray icon exists. On Linux without a StatusNotifierItem host
    /// this is false and close must behave like Quit unless the user forced
    /// residency (`platform.linux_force_tray_without_sni`).
    pub fn tray_available(&self) -> bool {
        self.tray.is_some()
    }

    pub fn tray_error(&self) -> Option<&str> {
        self.tray_error.as_deref()
    }

    /// Ordered teardown before `cx.quit()` (ADR-0002): IPC first, then hotkey,
    /// then the tray icon.
    pub fn shutdown(&mut self) {
        if let Some(mut ipc) = self.ipc.take() {
            ipc.shutdown();
        }
        self.hotkey.shutdown();
        if let Some(mut tray) = self.tray.take() {
            tray.shutdown();
        }
    }
}

impl Drop for PlatformService {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Map a forwarded IPC command onto the shared platform command set.
pub fn command_from_ipc(command: IpcCommand) -> PlatformCommand {
    match command {
        IpcCommand::Toggle => PlatformCommand::Toggle,
        IpcCommand::Show => PlatformCommand::Show,
        IpcCommand::OpenSettings => PlatformCommand::OpenSettings,
        IpcCommand::Quit => PlatformCommand::Quit,
    }
}

fn tray_error_message(err: &TrayError) -> String {
    err.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_commands_map_one_to_one() {
        assert_eq!(
            command_from_ipc(IpcCommand::Toggle),
            PlatformCommand::Toggle
        );
        assert_eq!(command_from_ipc(IpcCommand::Show), PlatformCommand::Show);
        assert_eq!(
            command_from_ipc(IpcCommand::OpenSettings),
            PlatformCommand::OpenSettings
        );
        assert_eq!(command_from_ipc(IpcCommand::Quit), PlatformCommand::Quit);
    }
}
