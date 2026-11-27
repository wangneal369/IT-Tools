//! GPUI-free command bridge between OS integrations and the UI shell (ADR-0002).
//!
//! Tray clicks, global hotkeys and second-instance IPC all converge onto a single
//! [`PlatformCommand`] channel. Native callbacks never borrow GPUI state; they
//! only push onto this unbounded channel, which the workbench drains on its own
//! executor. This keeps `devtools-platform` free of both `gpui` and `tokio`.

use async_channel::{Receiver, Sender, unbounded};

/// High-level intent produced by any OS integration surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlatformCommand {
    /// 智能切换：未在前台时唤起聚焦，已在前台时收起到托盘。
    Toggle,
    /// 幂等唤起并聚焦主工作台。
    Show,
    /// 唤起主工作台并打开设置面板。
    OpenSettings,
    /// 有序退出：先销毁托盘/热键/IPC，再退出进程。
    Quit,
}

/// Owned pair of the bridge endpoints.
///
/// `PlatformBridge` deliberately exposes a plain channel instead of a callback
/// trait object: the UI can `select`/retain it inside its own async executor
/// without the platform layer knowing anything about that executor.
#[derive(Debug, Clone)]
pub struct PlatformBridge {
    sender: Sender<PlatformCommand>,
    receiver: Option<Receiver<PlatformCommand>>,
}

impl PlatformBridge {
    pub fn new() -> Self {
        let (sender, receiver) = unbounded();
        Self {
            sender,
            receiver: Some(receiver),
        }
    }

    /// A cloneable producer handed to tray/hotkey/IPC callbacks.
    pub fn sender(&self) -> Sender<PlatformCommand> {
        self.sender.clone()
    }

    /// Take the single consumer side.
    pub fn take_receiver(&mut self) -> Option<Receiver<PlatformCommand>> {
        self.receiver.take()
    }

    /// Non-blocking push used by native callbacks.
    pub fn send(&self, command: PlatformCommand) {
        let _ = self.sender.try_send(command);
    }
}

impl Default for PlatformBridge {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_delivers_commands_without_blocking() {
        let mut bridge = PlatformBridge::new();
        let rx = bridge.take_receiver().expect("first take succeeds");
        bridge.send(PlatformCommand::Toggle);
        bridge.send(PlatformCommand::OpenSettings);

        assert_eq!(rx.try_recv(), Ok(PlatformCommand::Toggle));
        assert_eq!(rx.try_recv(), Ok(PlatformCommand::OpenSettings));
        assert!(rx.try_recv().is_err());
    }
}
