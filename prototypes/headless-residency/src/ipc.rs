//! Windows single-instance guard + Named Pipe IPC + AllowSetForegroundWindow handoff.
//! Throwaway prototype for Issue #12.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IpcCommand {
    Show,
    Toggle,
    Silent,
    Quit,
}

impl IpcCommand {
    pub fn as_byte(self) -> u8 {
        match self {
            Self::Show => b'S',
            Self::Toggle => b'T',
            Self::Silent => b'L',
            Self::Quit => b'Q',
        }
    }

    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            b'S' => Some(Self::Show),
            b'T' => Some(Self::Toggle),
            b'L' => Some(Self::Silent),
            b'Q' => Some(Self::Quit),
            _ => None,
        }
    }
}

#[cfg(windows)]
mod win {
    use super::IpcCommand;
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::thread;
    use std::time::Duration;
    use windows::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, ERROR_PIPE_CONNECTED, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE,
    };
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_GENERIC_WRITE,
        FILE_SHARE_NONE, FlushFileBuffers, OPEN_EXISTING, PIPE_ACCESS_DUPLEX, ReadFile, WriteFile,
    };
    use windows::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PIPE_READMODE_BYTE,
        PIPE_TYPE_BYTE, PIPE_WAIT, WaitNamedPipeW,
    };
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow;
    use windows::core::PCWSTR;

    pub struct SingleInstanceGuard {
        mutex: HANDLE,
    }

    // SAFETY: HANDLE is an OS kernel handle held until process exit.
    unsafe impl Send for SingleInstanceGuard {}

    impl Drop for SingleInstanceGuard {
        fn drop(&mut self) {
            unsafe {
                let _ = CloseHandle(self.mutex);
            }
        }
    }

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }

    fn user_scope_tag() -> String {
        let user = std::env::var("USERNAME").unwrap_or_else(|_| "default".into());
        user.chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect()
    }

    fn mutex_name() -> String {
        format!(
            "Local\\io.github.wangneal369.devtools.proto.{}",
            user_scope_tag()
        )
    }

    fn pipe_name() -> String {
        format!(
            "\\\\.\\pipe\\io.github.wangneal369.devtools.proto.{}",
            user_scope_tag()
        )
    }

    pub enum AcquireOutcome {
        Primary(SingleInstanceGuard),
        ForwardedToExisting {
            primary_pid: u32,
            allow_fg_ok: bool,
        },
        SilentNoopExisting,
    }

    /// Acquire the per-user mutex. If another instance already holds it, connect via
    /// Named Pipe, read its PID, call `AllowSetForegroundWindow(pid)`, send the command,
    /// and tell the caller to exit immediately without initializing GPUI.
    pub fn acquire_or_forward(cmd: IpcCommand) -> Result<AcquireOutcome, String> {
        let mname = to_wide(&mutex_name());
        let mutex = unsafe { CreateMutexW(None, false, PCWSTR(mname.as_ptr())) }
            .map_err(|e| format!("CreateMutexW failed: {e}"))?;
        let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;

        if !already_exists {
            return Ok(AcquireOutcome::Primary(SingleInstanceGuard { mutex }));
        }

        // Release duplicate handle right away.
        unsafe {
            let _ = CloseHandle(mutex);
        }

        if cmd == IpcCommand::Silent {
            // Per Issue #6: --silent on existing instance exits silently without popping window.
            return Ok(AcquireOutcome::SilentNoopExisting);
        }

        let pname = to_wide(&pipe_name());
        let mut last_err = String::new();
        for _ in 0..40 {
            let handle = unsafe {
                CreateFileW(
                    PCWSTR(pname.as_ptr()),
                    (FILE_GENERIC_READ | FILE_GENERIC_WRITE).0,
                    FILE_SHARE_NONE,
                    None,
                    OPEN_EXISTING,
                    FILE_ATTRIBUTE_NORMAL,
                    None,
                )
            };
            match handle {
                Ok(h) if h != INVALID_HANDLE_VALUE => {
                    let mut pid_buf = [0u8; 4];
                    let mut read = 0u32;
                    let read_res = unsafe { ReadFile(h, Some(&mut pid_buf), Some(&mut read), None) };
                    let primary_pid = if read_res.is_ok() && read == 4 {
                        u32::from_le_bytes(pid_buf)
                    } else {
                        0
                    };

                    let allow_fg_ok = if primary_pid != 0 {
                        unsafe { AllowSetForegroundWindow(primary_pid).is_ok() }
                    } else {
                        false
                    };

                    let cmd_buf = [cmd.as_byte()];
                    let mut written = 0u32;
                    let write_res =
                        unsafe { WriteFile(h, Some(&cmd_buf), Some(&mut written), None) };
                    unsafe {
                        let _ = FlushFileBuffers(h);
                        let _ = CloseHandle(h);
                    }
                    if write_res.is_ok() && written == 1 {
                        return Ok(AcquireOutcome::ForwardedToExisting {
                            primary_pid,
                            allow_fg_ok,
                        });
                    }
                    last_err = format!("WriteFile failed: {write_res:?}");
                }
                Ok(_) => {
                    last_err = "INVALID_HANDLE_VALUE".into();
                }
                Err(e) => {
                    last_err = format!("CreateFileW failed: {e}");
                    let _ = unsafe { WaitNamedPipeW(PCWSTR(pname.as_ptr()), 50) };
                }
            }
            thread::sleep(Duration::from_millis(25));
        }

        Err(format!("Failed to forward IPC command to primary instance: {last_err}"))
    }

    /// Spawn a background thread serving the Named Pipe and forwarding commands to GPUI.
    pub fn spawn_ipc_server(tx: async_channel::Sender<IpcCommand>) {
        let pname = pipe_name();
        thread::Builder::new()
            .name("devtools-proto-ipc".into())
            .spawn(move || {
                let wide = to_wide(&pname);
                let my_pid = std::process::id().to_le_bytes();
                loop {
                    let pipe = unsafe {
                        CreateNamedPipeW(
                            PCWSTR(wide.as_ptr()),
                            PIPE_ACCESS_DUPLEX,
                            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
                            4,
                            64,
                            64,
                            0,
                            None,
                        )
                    };
                    if pipe == INVALID_HANDLE_VALUE {
                        thread::sleep(Duration::from_millis(100));
                        continue;
                    }

                    let connected = match unsafe { ConnectNamedPipe(pipe, None) } {
                        Ok(()) => true,
                        Err(e) => e.code() == ERROR_PIPE_CONNECTED.to_hresult(),
                    };

                    if connected {
                        let mut written = 0u32;
                        let _ = unsafe { WriteFile(pipe, Some(&my_pid), Some(&mut written), None) };
                        let _ = unsafe { FlushFileBuffers(pipe) };

                        let mut buf = [0u8; 1];
                        let mut read = 0u32;
                        if unsafe { ReadFile(pipe, Some(&mut buf), Some(&mut read), None) }.is_ok()
                            && read == 1
                        {
                            if let Some(cmd) = IpcCommand::from_byte(buf[0]) {
                                let _ = tx.try_send(cmd);
                            }
                        }
                    }

                    unsafe {
                        let _ = DisconnectNamedPipe(pipe);
                        let _ = CloseHandle(pipe);
                    }
                }
            })
            .expect("spawn ipc thread");
    }
}

#[cfg(windows)]
pub use win::{AcquireOutcome, acquire_or_forward, spawn_ipc_server};
