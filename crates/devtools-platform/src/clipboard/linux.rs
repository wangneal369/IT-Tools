use super::*;
use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{ConnectionExt, CreateWindowAux, EventMask, PropMode, WindowClass};
use x11rb::wrapper::ConnectionExt as _;

fn error(e: impl std::fmt::Display) -> ClipboardError {
    ClipboardError(e.to_string())
}

pub(super) fn read() -> Result<ClipboardReadOutcome, ClipboardError> {
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        return read_wayland();
    }
    let (connection, screen) = x11rb::connect(None).map_err(error)?;
    let root = connection.setup().roots[screen].root;
    let window = connection.generate_id().map_err(error)?;
    connection
        .create_window(
            0,
            window,
            root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            0,
            &CreateWindowAux::new().event_mask(EventMask::PROPERTY_CHANGE),
        )
        .map_err(error)?;
    let atom = |name: &[u8]| -> Result<u32, ClipboardError> {
        Ok(connection
            .intern_atom(false, name)
            .map_err(error)?
            .reply()
            .map_err(error)?
            .atom)
    };
    let clipboard = atom(b"CLIPBOARD")?;
    let property = atom(b"DEVTOOLS_CLIPBOARD")?;
    let owner = connection
        .get_selection_owner(clipboard)
        .map_err(error)?
        .reply()
        .map_err(error)?
        .owner;
    if owner == 0 {
        return Ok(ClipboardReadOutcome::Empty);
    }
    let request = |target: u32, limit: usize| -> Result<Vec<u8>, ClipboardError> {
        connection
            .change_property8(PropMode::REPLACE, window, property, target, &[])
            .map_err(error)?;
        connection
            .convert_selection(window, clipboard, target, property, x11rb::CURRENT_TIME)
            .map_err(error)?;
        connection.flush().map_err(error)?;
        let deadline = Instant::now() + Duration::from_millis(250);
        loop {
            if let Some(Event::SelectionNotify(event)) =
                connection.poll_for_event().map_err(error)?
            {
                if event.property == 0 {
                    return Ok(vec![]);
                }
                let reply = connection
                    .get_property(
                        false,
                        window,
                        property,
                        x11rb::protocol::xproto::AtomEnum::ANY,
                        0,
                        (limit / 4 + 1) as u32,
                    )
                    .map_err(error)?
                    .reply()
                    .map_err(error)?;
                if reply.bytes_after != 0 || reply.value.len() > limit {
                    return Err(error("clipboard exceeds size limit"));
                }
                if reply.type_ == atom(b"INCR")? {
                    return Err(error("incremental clipboard deferred"));
                }
                return Ok(reply.value);
            }
            if Instant::now() >= deadline {
                return Err(error("clipboard conversion timed out"));
            }
            // Wait for this explicit selection request only; never subscribe to
            // selection changes or run a background clipboard watcher.
            std::thread::sleep(Duration::from_millis(2));
        }
    };
    let targets = request(atom(b"TARGETS")?, 64 * 1024)?;
    let targets: Vec<u32> = targets
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_ne_bytes(b.try_into().unwrap()))
        .collect();
    let hint = atom(b"x-kde-passwordManagerHint")?;
    if targets.contains(&hint) && request(hint, 64)?.as_slice() == b"secret" {
        return guarded_read(
            &ConcealmentHints {
                kde_password_hint: Some("secret".into()),
                ..Default::default()
            },
            || unreachable!("concealed payload"),
        );
    }
    let uri = atom(b"text/uri-list")?;
    let png = atom(b"image/png")?;
    let utf8 = atom(b"UTF8_STRING")?;
    let outcome = if targets.contains(&uri) {
        classify_payload(parse_files(&request(uri, FILE_LIST_LIMIT)?)?, None, None)
    } else if targets.contains(&png) {
        image(request(png, IMAGE_LIMIT)?)?
    } else if targets.contains(&utf8) {
        let bytes = request(utf8, TEXT_LIMIT)?;
        classify_payload(
            vec![],
            None,
            Some(String::from_utf8(bytes).map_err(error)?.into()),
        )
    } else {
        ClipboardReadOutcome::Empty
    };
    if connection
        .get_selection_owner(clipboard)
        .map_err(error)?
        .reply()
        .map_err(error)?
        .owner
        != owner
    {
        return Err(error("clipboard owner changed"));
    }
    Ok(outcome)
}

fn read_wayland() -> Result<ClipboardReadOutcome, ClipboardError> {
    // wl-paste uses the compositor's available data-control protocol. If absent
    // (e.g. GNOME), fail closed; never read through an unchecked XWayland owner.
    let types = command(&["--list-types"], 64 * 1024)?;
    let types = String::from_utf8(types).map_err(error)?;
    if types.lines().any(|s| s == "x-kde-passwordManagerHint") {
        let hint = command(&["--no-newline", "--type", "x-kde-passwordManagerHint"], 64)?;
        if hint == b"secret" {
            return guarded_read(
                &ConcealmentHints {
                    kde_password_hint: Some("secret".into()),
                    ..Default::default()
                },
                || unreachable!("concealed payload"),
            );
        }
    }
    if types.lines().any(|s| s == "text/uri-list") {
        return Ok(classify_payload(
            parse_files(&command(
                &["--no-newline", "--type", "text/uri-list"],
                FILE_LIST_LIMIT,
            )?)?,
            None,
            None,
        ));
    }
    if types.lines().any(|s| s == "image/png") {
        return image(command(
            &["--no-newline", "--type", "image/png"],
            IMAGE_LIMIT,
        )?);
    }
    if let Some(kind) = types.lines().find(|s| s.starts_with("text/plain")) {
        let bytes = command(&["--no-newline", "--type", kind], TEXT_LIMIT)?;
        return Ok(classify_payload(
            vec![],
            None,
            Some(String::from_utf8(bytes).map_err(error)?.into()),
        ));
    }
    Ok(ClipboardReadOutcome::Empty)
}
fn command(args: &[&str], limit: usize) -> Result<Vec<u8>, ClipboardError> {
    let mut child = Command::new("wl-paste")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(error)?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| error("no clipboard stream"))?;
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes);
        let _ = send.send(result);
    });
    let result = receive.recv_timeout(Duration::from_millis(300));
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error("Wayland clipboard timed out"));
    }
    let bytes = result.unwrap().map_err(error)?;
    if bytes.len() > limit {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error("clipboard exceeds size limit"));
    }
    if !child.wait().map_err(error)?.success() {
        return Err(error("Wayland clipboard unavailable"));
    }
    Ok(bytes)
}
fn parse_files(bytes: &[u8]) -> Result<Vec<PathBuf>, ClipboardError> {
    let value = std::str::from_utf8(bytes).map_err(error)?;
    value
        .lines()
        .filter(|s| !s.starts_with('#') && !s.is_empty())
        .take(FILE_LIMIT + 1)
        .map(|line| {
            let path = line
                .strip_prefix("file://localhost/")
                .map(|p| format!("/{p}"))
                .or_else(|| line.strip_prefix("file://").map(str::to_owned))
                .ok_or_else(|| error("unsupported file URI"))?;
            if !path.starts_with('/') {
                return Err(error("remote file URI unsupported"));
            }
            Ok(PathBuf::from(
                percent_encoding::percent_decode_str(&path)
                    .decode_utf8()
                    .map_err(error)?
                    .as_ref(),
            ))
        })
        .collect()
}
fn image(bytes: Vec<u8>) -> Result<ClipboardReadOutcome, ClipboardError> {
    let size = imagesize::blob_size(&bytes).map_err(error)?;
    Ok(classify_payload(
        vec![],
        Some(ImagePayload {
            width: size.width as u32,
            height: size.height as u32,
            bytes: bytes.into(),
        }),
        None,
    ))
}
