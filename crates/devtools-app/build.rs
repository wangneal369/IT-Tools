//! Embed the Windows application icon (`RT_GROUP_ICON` ID = 1) into `devtools.exe`.
//!
//! `gpui-pre-windows` loads `LoadImageW(module, PCWSTR(1), IMAGE_ICON, ...)` for
//! window titlebars and taskbar icons, and Windows Explorer uses the first
//! `RT_GROUP_ICON` in the PE resource directory for file/shortcut icons.
//!
//! Emitting the Win32 binary `.res` file directly in pure Rust keeps the
//! workspace free of extra build-dependencies and avoids depending on `rc.exe`
//! being on `PATH`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let ico_path = Path::new("../../assets/icons/devtools.ico");
    println!("cargo:rerun-if-changed={}", ico_path.display());
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os != "windows" || target_env != "msvc" {
        return;
    }

    let ico = fs::read(ico_path).unwrap_or_else(|err| {
        panic!("failed to read {}: {err}", ico_path.display());
    });
    let res_bytes = compile_ico_to_res(&ico).expect("invalid Windows .ico file");

    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR not set"));
    let res_path = out_dir.join("devtools-icon.res");
    fs::write(&res_path, res_bytes).expect("failed to write devtools-icon.res");

    println!("cargo:rustc-link-arg-bin=devtools={}", res_path.display());
}

fn compile_ico_to_res(ico: &[u8]) -> Result<Vec<u8>, &'static str> {
    if ico.len() < 6 {
        return Err("ico shorter than ICONDIR header");
    }
    let reserved = u16::from_le_bytes([ico[0], ico[1]]);
    let ico_type = u16::from_le_bytes([ico[2], ico[3]]);
    let count = u16::from_le_bytes([ico[4], ico[5]]) as usize;
    if reserved != 0 || ico_type != 1 || count == 0 {
        return Err("unexpected ICONDIR header");
    }
    let dir_end = 6 + count * 16;
    if ico.len() < dir_end {
        return Err("ico truncated in ICONDIRENTRY table");
    }

    let mut res = Vec::with_capacity(ico.len() + 512);
    // Standard 32-byte empty Win32 .res file header.
    append_resource_record(&mut res, 0, 0, 0, 0, &[]);

    let mut group_payload = Vec::with_capacity(6 + count * 14);
    group_payload.extend_from_slice(&0u16.to_le_bytes());
    group_payload.extend_from_slice(&1u16.to_le_bytes());
    group_payload.extend_from_slice(&(count as u16).to_le_bytes());

    for index in 0..count {
        let entry_offset = 6 + index * 16;
        let entry = &ico[entry_offset..entry_offset + 16];
        let bytes_in_res = u32::from_le_bytes([entry[8], entry[9], entry[10], entry[11]]) as usize;
        let image_offset =
            u32::from_le_bytes([entry[12], entry[13], entry[14], entry[15]]) as usize;
        let image_end = image_offset
            .checked_add(bytes_in_res)
            .ok_or("ico entry offset overflow")?;
        if image_end > ico.len() {
            return Err("ico entry points past end of file");
        }

        let icon_id = u16::try_from(index + 1).map_err(|_| "too many icon frames")?;
        // RT_ICON = 3, memory flags = MOVEABLE | PURE | DISCARDABLE (0x1030), lang = en-US (0x0409)
        append_resource_record(
            &mut res,
            3,
            icon_id,
            0x1030,
            0x0409,
            &ico[image_offset..image_end],
        );

        // GRPICONDIRENTRY: first 12 bytes match ICONDIRENTRY, followed by u16 resource ordinal ID.
        group_payload.extend_from_slice(&entry[0..12]);
        group_payload.extend_from_slice(&icon_id.to_le_bytes());
    }

    // RT_GROUP_ICON = 14, resource ID = 1 (required by gpui-pre-windows `LoadImageW(_, 1, ...)`).
    append_resource_record(&mut res, 14, 1, 0x1030, 0x0409, &group_payload);
    Ok(res)
}

fn append_resource_record(
    out: &mut Vec<u8>,
    type_ordinal: u16,
    name_ordinal: u16,
    memory_flags: u16,
    language_id: u16,
    payload: &[u8],
) {
    let data_size = u32::try_from(payload.len()).expect("resource payload exceeds u32");
    let header_size: u32 = 32;

    out.extend_from_slice(&data_size.to_le_bytes());
    out.extend_from_slice(&header_size.to_le_bytes());
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&type_ordinal.to_le_bytes());
    out.extend_from_slice(&0xFFFFu16.to_le_bytes());
    out.extend_from_slice(&name_ordinal.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // DataVersion
    out.extend_from_slice(&memory_flags.to_le_bytes());
    out.extend_from_slice(&language_id.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // Version
    out.extend_from_slice(&0u32.to_le_bytes()); // Characteristics
    out.extend_from_slice(payload);

    while !out.len().is_multiple_of(4) {
        out.push(0);
    }
}
