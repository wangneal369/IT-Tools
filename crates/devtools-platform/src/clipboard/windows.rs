use super::*;
use std::{ffi::c_void, ptr};

#[link(name = "user32")]
unsafe extern "system" {
    fn OpenClipboard(owner: *mut c_void) -> i32;
    fn CloseClipboard() -> i32;
    fn IsClipboardFormatAvailable(format: u32) -> i32;
    fn RegisterClipboardFormatW(name: *const u16) -> u32;
    fn GetClipboardData(format: u32) -> *mut c_void;
    fn GetClipboardSequenceNumber() -> u32;
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalSize(handle: *mut c_void) -> usize;
    fn GlobalLock(handle: *mut c_void) -> *const u8;
    fn GlobalUnlock(handle: *mut c_void) -> i32;
}
#[link(name = "shell32")]
unsafe extern "system" {
    fn DragQueryFileW(drop: *mut c_void, index: u32, path: *mut u16, count: u32) -> u32;
}
struct Open;
impl Drop for Open {
    fn drop(&mut self) {
        unsafe {
            CloseClipboard();
        }
    }
}
fn format(name: &str) -> u32 {
    let name: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
    unsafe { RegisterClipboardFormatW(name.as_ptr()) }
}
fn available(format: u32) -> bool {
    format != 0 && unsafe { IsClipboardFormatAvailable(format) != 0 }
}
fn data(format: u32, limit: usize) -> Result<Vec<u8>, ClipboardError> {
    // The clipboard stays open until the owned copy has been made; GlobalSize
    // is checked before locking or allocating any payload bytes.
    unsafe {
        let handle = GetClipboardData(format);
        if handle.is_null() {
            return Err(ClipboardError("format unavailable".into()));
        }
        let length = GlobalSize(handle);
        if length > limit {
            return Err(ClipboardError("payload exceeds size limit".into()));
        }
        if length == 0 {
            return Ok(Vec::new());
        }
        let pointer = GlobalLock(handle);
        if pointer.is_null() {
            return Err(ClipboardError("cannot lock clipboard data".into()));
        }
        let bytes = std::slice::from_raw_parts(pointer, length).to_vec();
        GlobalUnlock(handle);
        Ok(bytes)
    }
}

pub(super) fn read() -> Result<ClipboardReadOutcome, ClipboardError> {
    let mut opened = false;
    for _ in 0..5 {
        if unsafe { OpenClipboard(ptr::null_mut()) } != 0 {
            opened = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    if !opened {
        return Err(ClipboardError("clipboard occupied".into()));
    }
    let _open = Open;
    let excluded = format("ExcludeClipboardContentFromMonitorProcessing");
    let history = format("CanIncludeInClipboardHistory");
    let hints = ConcealmentHints {
        formats: if available(excluded) {
            vec!["ExcludeClipboardContentFromMonitorProcessing".into()]
        } else {
            Vec::new()
        },
        can_include_in_history: if available(history) {
            Some(data(history, 4)?.as_slice() != [0, 0, 0, 0])
        } else {
            None
        },
        ..Default::default()
    };
    guarded_read(&hints, read_payload)
}

fn read_payload() -> Result<ClipboardReadOutcome, ClipboardError> {
    let sequence = u64::from(unsafe { GetClipboardSequenceNumber() });
    let payload = if available(15) {
        let drop = unsafe { GetClipboardData(15) };
        if drop.is_null() {
            return Err(ClipboardError("file list unavailable".into()));
        }
        let count = unsafe { DragQueryFileW(drop, u32::MAX, ptr::null_mut(), 0) };
        if count as usize > FILE_LIMIT {
            return Ok(ClipboardReadOutcome::TooLarge);
        }
        let mut paths = Vec::new();
        let mut total = 0;
        for index in 0..count {
            let length = unsafe { DragQueryFileW(drop, index, ptr::null_mut(), 0) };
            total += length as usize;
            if total > FILE_LIST_LIMIT {
                return Ok(ClipboardReadOutcome::TooLarge);
            }
            let mut path = vec![0u16; length as usize + 1];
            unsafe {
                DragQueryFileW(drop, index, path.as_mut_ptr(), length + 1);
            }
            use std::os::windows::ffi::OsStringExt;
            paths.push(PathBuf::from(std::ffi::OsString::from_wide(
                &path[..length as usize],
            )));
        }
        if paths.is_empty() {
            return Ok(ClipboardReadOutcome::Empty);
        }
        RawPayload::Files(paths)
    } else if available(format("PNG")) || available(8) {
        let png = format("PNG");
        let bytes = if available(png) {
            data(png, IMAGE_LIMIT)?
        } else {
            let dib = data(8, IMAGE_LIMIT - 14)?;
            if dib.len() < 40 {
                return Ok(ClipboardReadOutcome::Empty);
            }
            let header = u32::from_le_bytes(dib[0..4].try_into().unwrap()) as usize;
            let bits = u16::from_le_bytes(dib[14..16].try_into().unwrap());
            let compression = u32::from_le_bytes(dib[16..20].try_into().unwrap());
            let colors = u32::from_le_bytes(dib[32..36].try_into().unwrap()) as usize;
            let palette = if colors != 0 {
                colors
            } else if bits <= 8 {
                1 << bits
            } else {
                0
            };
            let masks = if header == 40 && compression == 3 {
                12
            } else {
                0
            };
            let offset = 14usize
                .saturating_add(header)
                .saturating_add(palette.saturating_mul(4))
                .saturating_add(masks);
            if offset > dib.len() + 14 {
                return Ok(ClipboardReadOutcome::Empty);
            }
            let mut bmp = Vec::with_capacity(dib.len() + 14);
            bmp.extend_from_slice(b"BM");
            bmp.extend_from_slice(&((dib.len() + 14) as u32).to_le_bytes());
            bmp.extend_from_slice(&[0; 4]);
            bmp.extend_from_slice(&(offset as u32).to_le_bytes());
            bmp.extend_from_slice(&dib);
            bmp
        };
        let size = imagesize::blob_size(&bytes).map_err(|e| ClipboardError(e.to_string()))?;
        RawPayload::Image(ImagePayload {
            width: size.width as u32,
            height: size.height as u32,
            bytes: bytes.into(),
        })
    } else if available(13) {
        let bytes = data(13, TEXT_LIMIT * 2 + 2)?;
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| u16::from_le_bytes([b[0], b[1]]))
            .take_while(|u| *u != 0)
            .collect();
        let text = String::from_utf16_lossy(&units);
        if text.len() > TEXT_LIMIT {
            return Ok(ClipboardReadOutcome::TooLarge);
        }
        if text.is_empty() {
            return Ok(ClipboardReadOutcome::Empty);
        }
        RawPayload::Text(text.into())
    } else {
        return Ok(ClipboardReadOutcome::Empty);
    };
    Ok(ClipboardReadOutcome::Payload { sequence, payload })
}
