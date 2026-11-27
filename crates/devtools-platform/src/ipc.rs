//! Single-instance IPC over an `interprocess` local socket (ADR-0002 / spec §3.1).
//!
//! The first process binds a per-user local socket and serves newline-framed
//! commands on a dedicated thread. Later processes connect, forward one command
//! and exit before ever touching GPUI. On Unix filesystem sockets a corpse left
//! by a crashed process is removed before rebinding; on Linux abstract-namespace
//! sockets the kernel cleans up on process exit.
//!
//! Wire format: one UTF-8 command per line (`toggle`, `show`, `open-settings`,
//! `quit`). The reserved token `shutdown` tears down the server thread and is
//! never produced by a client.

use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use interprocess::local_socket::{
    GenericFilePath, GenericNamespaced, ListenerOptions, Stream, prelude::*,
};

/// The closed set of commands a second instance may forward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IpcCommand {
    Toggle,
    Show,
    OpenSettings,
    Quit,
}

impl IpcCommand {
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Toggle => "toggle",
            Self::Show => "show",
            Self::OpenSettings => "open-settings",
            Self::Quit => "quit",
        }
    }

    pub fn from_wire(value: &str) -> Option<Self> {
        match value {
            "toggle" => Some(Self::Toggle),
            "show" => Some(Self::Show),
            "open-settings" => Some(Self::OpenSettings),
            "quit" => Some(Self::Quit),
            _ => None,
        }
    }
}

/// Reserved token that stops the server thread.
const SHUTDOWN_TOKEN: &str = "shutdown";
const CONNECT_ATTEMPTS: u32 = 40;
const CONNECT_RETRY_DELAY: Duration = Duration::from_millis(25);

/// A stable, user-scoped local socket identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpcName(String);

impl IpcName {
    /// Build a name from a reverse-DNS base, e.g.
    /// `io.github.wangneal369.devtools`.
    ///
    /// Windows named pipes are machine-global, so the current user is appended
    /// to avoid cross-user collisions.
    pub fn user_scoped(base: &str) -> Self {
        let user = std::env::var("USERNAME")
            .or_else(|_| std::env::var("USER"))
            .unwrap_or_else(|_| "user".to_owned());
        let sanitized: String = user
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        Self(format!("{base}.{sanitized}"))
    }

    /// Explicit name (tests use a pid/nanos-unique value).
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Filesystem path used when the platform has no namespaced sockets.
    fn filesystem_path(&self) -> PathBuf {
        std::env::temp_dir().join(format!("{}.sock", self.0))
    }

    fn uses_namespace(&self) -> bool {
        GenericNamespaced::is_supported()
    }
}

/// Whether this process owns the socket or another instance already does.
#[derive(Debug)]
pub enum Instance {
    /// This process is the primary instance and owns the bound listener.
    Primary(InstanceGuard),
    /// Another instance answered the socket.
    Existing,
}

/// Keeps the primary listener alive until [`serve`] consumes it.
#[derive(Debug)]
pub struct InstanceGuard {
    name: IpcName,
    listener: Option<LocalSocketListener>,
}

impl InstanceGuard {
    pub fn name(&self) -> &IpcName {
        &self.name
    }
}

/// A running IPC server thread.
#[derive(Debug)]
pub struct IpcServer {
    name: IpcName,
    handle: Option<JoinHandle<()>>,
}

impl IpcServer {
    /// Stop the accept loop and join its thread.
    pub fn shutdown(&mut self) {
        let _ = send_wire(&self.name, SHUTDOWN_TOKEN);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("ipc io error: {0}")]
    Io(#[from] io::Error),
    #[error("no running DevTools instance answered the socket")]
    NotRunning,
}

/// Try to become the primary instance.
///
/// When the socket address is already taken we probe it with a plain connect:
/// a successful connect means another instance is alive; a failed connect means
/// a stale Unix socket file, which is removed before rebinding.
pub fn acquire(name: &IpcName) -> Result<Instance, IpcError> {
    match create_listener(name) {
        Ok(listener) => Ok(Instance::Primary(InstanceGuard {
            name: name.clone(),
            listener: Some(listener),
        })),
        Err(err) if socket_taken(&err) => {
            if connect_stream(name, 4).is_ok() {
                return Ok(Instance::Existing);
            }

            remove_stale_socket(name);
            match create_listener(name) {
                Ok(listener) => Ok(Instance::Primary(InstanceGuard {
                    name: name.clone(),
                    listener: Some(listener),
                })),
                // Another instance won the race for the freshly cleaned
                // address; do not clobber it.
                Err(retry) if socket_taken(&retry) => Ok(Instance::Existing),
                Err(retry) => Err(IpcError::Io(retry)),
            }
        }
        Err(err) => Err(IpcError::Io(err)),
    }
}

/// Forward one command to the primary instance.
pub fn send(name: &IpcName, command: IpcCommand) -> Result<(), IpcError> {
    send_wire(name, command.as_wire()).map_err(|err| {
        if err.kind() == io::ErrorKind::NotFound || err.kind() == io::ErrorKind::ConnectionRefused {
            IpcError::NotRunning
        } else {
            IpcError::Io(err)
        }
    })
}

/// Serve commands from the primary listener on a background thread.
pub fn serve(guard: InstanceGuard, sink: impl FnMut(IpcCommand) + Send + 'static) -> IpcServer {
    let name = guard.name.clone();
    let Some(listener) = guard.listener else {
        return IpcServer { name, handle: None };
    };
    let handle = thread::Builder::new()
        .name("devtools-ipc".to_owned())
        .spawn(move || {
            let mut sink = sink;
            for connection in listener.incoming() {
                let Ok(connection) = connection else {
                    continue;
                };
                let mut reader = BufReader::new(connection);
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    continue;
                }
                let trimmed = line.trim();
                if trimmed == SHUTDOWN_TOKEN {
                    break;
                }
                if let Some(command) = IpcCommand::from_wire(trimmed) {
                    sink(command);
                }
            }
        })
        .expect("spawn devtools-ipc thread");

    IpcServer {
        name: guard.name,
        handle: Some(handle),
    }
}

fn send_wire(name: &IpcName, wire: &str) -> io::Result<()> {
    let mut stream = connect_stream(name, CONNECT_ATTEMPTS)?;
    stream.write_all(wire.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

/// Windows reports an occupied named pipe as `ERROR_ACCESS_DENIED` (5) or
/// `ERROR_SHARING_VIOLATION` (32) instead of `AddrInUse`; treat both as "the
/// address is taken" so the probe path runs on every platform.
fn socket_taken(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::AddrInUse
        || err.kind() == io::ErrorKind::PermissionDenied
        || matches!(err.raw_os_error(), Some(5) | Some(32))
}

fn create_listener(name: &IpcName) -> io::Result<LocalSocketListener> {
    if name.uses_namespace() {
        let value = name.as_str().to_owned();
        ListenerOptions::new()
            .name(value.as_str().to_ns_name::<GenericNamespaced>()?)
            .create_sync()
    } else {
        let path = name.filesystem_path();
        ListenerOptions::new()
            .name(path.to_fs_name::<GenericFilePath>()?)
            .create_sync()
    }
}

fn connect_stream(name: &IpcName, attempts: u32) -> io::Result<Stream> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, "socket never attempted");
    for attempt in 0..attempts.max(1) {
        let result = if name.uses_namespace() {
            let value = name.as_str().to_owned();
            value
                .as_str()
                .to_ns_name::<GenericNamespaced>()
                .and_then(|name| Stream::connect(name))
        } else {
            let path = name.filesystem_path();
            path.to_fs_name::<GenericFilePath>()
                .and_then(|name| Stream::connect(name))
        };
        match result {
            Ok(stream) => return Ok(stream),
            Err(err) => last = err,
        }
        if attempt + 1 < attempts {
            thread::sleep(CONNECT_RETRY_DELAY);
        }
    }
    Err(last)
}

/// Remove a Unix filesystem socket left behind by a crashed process.
fn remove_stale_socket(name: &IpcName) {
    if name.uses_namespace() {
        return;
    }
    let path = name.filesystem_path();
    if path.exists() {
        let _ = std::fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    fn unique_name(tag: &str) -> IpcName {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        IpcName::new(format!(
            "io.github.wangneal369.devtools.test.{tag}.{}.{nanos}.{seq}",
            std::process::id()
        ))
    }

    fn wait_for<T>(rx: &async_channel::Receiver<T>) -> Option<T> {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Ok(value) = rx.try_recv() {
                return Some(value);
            }
            thread::sleep(Duration::from_millis(10));
        }
        None
    }

    #[test]
    fn wire_format_round_trips_every_command() {
        for command in [
            IpcCommand::Toggle,
            IpcCommand::Show,
            IpcCommand::OpenSettings,
            IpcCommand::Quit,
        ] {
            assert_eq!(IpcCommand::from_wire(command.as_wire()), Some(command));
        }
        assert_eq!(IpcCommand::from_wire("bogus"), None);
        assert_eq!(IpcCommand::from_wire(SHUTDOWN_TOKEN), None);
    }

    #[test]
    fn ipc_roundtrip_acquire_forward_and_release() {
        let name = unique_name("roundtrip");

        let guard = match acquire(&name).expect("first acquire") {
            Instance::Primary(guard) => guard,
            Instance::Existing => panic!("fresh socket must be primary"),
        };

        let (tx, rx) = async_channel::unbounded::<IpcCommand>();
        let mut server = serve(guard, move |command| {
            let _ = tx.try_send(command);
        });

        match acquire(&name).expect("second acquire") {
            Instance::Existing => {}
            Instance::Primary(_) => panic!("second acquire must see the primary instance"),
        }

        send(&name, IpcCommand::OpenSettings).expect("forward OpenSettings");
        assert_eq!(wait_for(&rx), Some(IpcCommand::OpenSettings));

        send(&name, IpcCommand::Toggle).expect("forward Toggle");
        assert_eq!(wait_for(&rx), Some(IpcCommand::Toggle));

        server.shutdown();
        drop(server);

        // After the primary releases the socket a new primary can bind it.
        match acquire(&name).expect("reacquire after shutdown") {
            Instance::Primary(_guard) => {}
            Instance::Existing => panic!("socket should be free after shutdown"),
        }
    }

    #[test]
    fn sending_without_a_server_reports_not_running() {
        let name = unique_name("missing");
        let err = send(&name, IpcCommand::Show).expect_err("no server");
        assert!(matches!(err, IpcError::NotRunning | IpcError::Io(_)));
    }
}
