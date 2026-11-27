use super::*;
use objc2_app_kit::NSPasteboard;
use objc2_foundation::{NSData, NSString};

pub(super) fn mark_transient() {
    let board = NSPasteboard::generalPasteboard();
    let empty = NSData::with_bytes(&[]);
    board.setData_forType(
        Some(&empty),
        &NSString::from_str("org.nspasteboard.TransientType"),
    );
}

pub(super) fn read() -> Result<ClipboardReadOutcome, ClipboardError> {
    let board = NSPasteboard::generalPasteboard();
    let sequence = board.changeCount() as u64;
    let types = board
        .types()
        .ok_or_else(|| ClipboardError("pasteboard types unavailable".into()))?;
    let formats: Vec<String> = types.iter().map(|t| t.to_string()).collect();
    let hints = ConcealmentHints {
        formats: formats.clone(),
        ..Default::default()
    };
    guarded_read(&hints, || read_payload(&board, sequence, &formats))
}

fn read_payload(
    board: &NSPasteboard,
    sequence: u64,
    formats: &[String],
) -> Result<ClipboardReadOutcome, ClipboardError> {
    // Inspect each file URL's byte length before copying it into Rust memory.
    let mut files = Vec::new();
    if formats.iter().any(|s| s == "public.file-url")
        && let Some(items) = board.pasteboardItems()
    {
        if items.len() > FILE_LIMIT {
            return Ok(ClipboardReadOutcome::TooLarge);
        }
        let mut total = 0;
        for item in items.iter() {
            let Some(data) = item.dataForType(&NSString::from_str("public.file-url")) else {
                continue;
            };
            total += data.length();
            if total > FILE_LIST_LIMIT {
                return Ok(ClipboardReadOutcome::TooLarge);
            }
            let value =
                String::from_utf8(data.to_vec()).map_err(|e| ClipboardError(e.to_string()))?;
            let url = objc2_foundation::NSURL::URLWithString(&NSString::from_str(&value))
                .ok_or_else(|| ClipboardError("invalid file URL".into()))?;
            if let Some(path) = url.path() {
                files.push(PathBuf::from(path.to_string()));
            }
        }
    }
    let outcome = if !files.is_empty() {
        classify_payload(files, None, None)
    } else if let Some(kind) = ["public.png", "public.tiff", "public.jpeg"]
        .iter()
        .find(|kind| formats.iter().any(|f| f == **kind))
    {
        let data = board
            .dataForType(&NSString::from_str(kind))
            .ok_or_else(|| ClipboardError("image unavailable".into()))?;
        if data.length() > IMAGE_LIMIT {
            return Ok(ClipboardReadOutcome::TooLarge);
        }
        let bytes = data.to_vec();
        let size = imagesize::blob_size(&bytes).map_err(|e| ClipboardError(e.to_string()))?;
        classify_payload(
            vec![],
            Some(ImagePayload {
                width: size.width as u32,
                height: size.height as u32,
                bytes: bytes.into(),
            }),
            None,
        )
    } else {
        let kind = NSString::from_str("public.utf8-plain-text");
        let Some(data) = board.dataForType(&kind) else {
            return Ok(ClipboardReadOutcome::Empty);
        };
        if data.length() > TEXT_LIMIT {
            return Ok(ClipboardReadOutcome::TooLarge);
        }
        let text = String::from_utf8(data.to_vec()).map_err(|e| ClipboardError(e.to_string()))?;
        classify_payload(vec![], None, Some(text.into()))
    };
    if board.changeCount() as u64 != sequence {
        return Err(ClipboardError("clipboard changed during read".into()));
    }
    Ok(match outcome {
        ClipboardReadOutcome::Payload { payload, .. } => {
            ClipboardReadOutcome::Payload { sequence, payload }
        }
        other => other,
    })
}
