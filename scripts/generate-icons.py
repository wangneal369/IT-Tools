#!/usr/bin/env python3
"""Render assets/icons/devtools.svg into Windows .ico, macOS .icns, PNG, and tray RGBA.

Uses headless Edge/Chrome for full SVG 1.1 filter/gradient fidelity and Pillow for
high-quality Lanczos downsampling and container packing.
"""

from __future__ import annotations

import io
import shutil
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image

REPO_ROOT = Path(__file__).resolve().parent.parent
ICONS_DIR = REPO_ROOT / "assets" / "icons"
SVG_PATH = ICONS_DIR / "devtools.svg"

ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]
ICNS_CHUNKS = [
    (b"icp4", 16),
    (b"icp5", 32),
    (b"icp6", 64),
    (b"ic07", 128),
    (b"ic08", 256),
    (b"ic09", 512),
    (b"ic10", 1024),
]
TRAY_SIZE = 64
UI_BRAND_SIZE = 64


def find_browser() -> str:
    candidates = [
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        "msedge",
        "google-chrome",
        "chromium",
    ]
    for candidate in candidates:
        path = Path(candidate)
        if path.is_file():
            return str(path)
        found = shutil.which(candidate)
        if found:
            return found
    raise RuntimeError("Could not find Edge/Chrome/Chromium to rasterize SVG.")


def rasterize_svg(svg_path: Path, size: int = 1024) -> Image.Image:
    browser = find_browser()
    with tempfile.TemporaryDirectory(prefix="devtools-icon-") as tmp:
        tmp_dir = Path(tmp)
        html_path = tmp_dir / "render.html"
        out_png = tmp_dir / "master.png"
        svg_text = svg_path.read_text(encoding="utf-8")
        html_path.write_text(
            "<!doctype html><html><head><meta charset='utf-8'><style>"
            f"html,body{{margin:0;padding:0;width:{size}px;height:{size}px;"
            "background:transparent;overflow:hidden;}"
            f"svg{{display:block;width:{size}px;height:{size}px;}}"
            "</style></head><body>"
            f"{svg_text}</body></html>",
            encoding="utf-8",
        )
        cmd = [
            browser,
            "--headless=new",
            "--disable-gpu",
            "--hide-scrollbars",
            "--default-background-color=00000000",
            "--force-device-scale-factor=1",
            f"--window-size={size},{size}",
            f"--screenshot={out_png}",
            html_path.as_uri(),
        ]
        proc = subprocess.run(cmd, capture_output=True, text=True, check=False)
        if proc.returncode != 0 or not out_png.is_file():
            raise RuntimeError(
                f"Browser rasterization failed (code={proc.returncode}): {proc.stderr}"
            )
        img = Image.open(out_png).convert("RGBA")
        if img.size != (size, size):
            img = img.resize((size, size), Image.Resampling.LANCZOS)
        # Verify transparent corners
        corner_alpha = img.getpixel((0, 0))[3]
        if corner_alpha != 0:
            raise RuntimeError(
                f"Expected transparent corner (alpha=0), got alpha={corner_alpha}"
            )
        return img.copy()


def encode_png(img: Image.Image) -> bytes:
    buf = io.BytesIO()
    img.save(buf, format="PNG", optimize=True)
    return buf.getvalue()


def encode_bmp_ico_entry(img: Image.Image) -> bytes:
    """Encode a 32-bit BGRA DIB + AND mask for maximum Windows compatibility."""
    w, h = img.size
    rgba = img.convert("RGBA").tobytes()
    # BITMAPINFOHEADER (40 bytes): height is 2*h (XOR + AND mask)
    header = struct.pack(
        "<IIIHHIIIIII",
        40,  # biSize
        w,  # biWidth
        h * 2,  # biHeight (XOR + AND)
        1,  # biPlanes
        32,  # biBitCount
        0,  # biCompression = BI_RGB
        w * h * 4,  # biSizeImage
        0,
        0,
        0,
        0,
    )
    # Bottom-up BGRA rows
    xor_rows = bytearray()
    for y in range(h - 1, -1, -1):
        row_start = y * w * 4
        for x in range(w):
            p = row_start + x * 4
            r, g, b, a = rgba[p], rgba[p + 1], rgba[p + 2], rgba[p + 3]
            xor_rows.extend((b, g, r, a))
    # Bottom-up 1-bit AND mask, padded to 4-byte boundary per row
    mask_stride = ((w + 31) // 32) * 4
    and_rows = bytearray(mask_stride * h)
    for y in range(h):
        src_y = h - 1 - y
        for x in range(w):
            alpha = rgba[(src_y * w + x) * 4 + 3]
            if alpha == 0:
                and_rows[y * mask_stride + (x // 8)] |= 0x80 >> (x % 8)
    return header + bytes(xor_rows) + bytes(and_rows)


def build_ico(master: Image.Image) -> bytes:
    entries: list[tuple[int, bytes]] = []
    for size in ICO_SIZES:
        resized = master.resize((size, size), Image.Resampling.LANCZOS)
        # Use PNG for 256px (standard Vista+ ICO) and 32-bit DIB for smaller sizes.
        payload = encode_png(resized) if size >= 256 else encode_bmp_ico_entry(resized)
        entries.append((size, payload))

    header = struct.pack("<HHH", 0, 1, len(entries))
    offset = 6 + 16 * len(entries)
    dir_entries = bytearray()
    blobs = bytearray()
    for size, payload in entries:
        dim = 0 if size >= 256 else size
        dir_entries.extend(
            struct.pack(
                "<BBBBHHII",
                dim,
                dim,
                0,  # color count
                0,  # reserved
                1,  # planes
                32,  # bpp
                len(payload),
                offset,
            )
        )
        blobs.extend(payload)
        offset += len(payload)
    return header + bytes(dir_entries) + bytes(blobs)


def build_icns(master: Image.Image) -> bytes:
    chunks = bytearray()
    for tag, size in ICNS_CHUNKS:
        resized = (
            master
            if size == master.width
            else master.resize((size, size), Image.Resampling.LANCZOS)
        )
        png_bytes = encode_png(resized)
        chunks.extend(tag)
        chunks.extend(struct.pack(">I", 8 + len(png_bytes)))
        chunks.extend(png_bytes)
    total_len = 8 + len(chunks)
    return b"icns" + struct.pack(">I", total_len) + bytes(chunks)


def main() -> int:
    if not SVG_PATH.is_file():
        print(f"Missing SVG source: {SVG_PATH}", file=sys.stderr)
        return 1

    master = rasterize_svg(SVG_PATH, 1024)

    ico_path = ICONS_DIR / "devtools.ico"
    ico_bytes = build_ico(master)
    ico_path.write_bytes(ico_bytes)

    icns_path = ICONS_DIR / "devtools.icns"
    icns_bytes = build_icns(master)
    icns_path.write_bytes(icns_bytes)

    brand_png = master.resize((UI_BRAND_SIZE, UI_BRAND_SIZE), Image.Resampling.LANCZOS)
    brand_png_path = ICONS_DIR / "devtools-64.png"
    brand_png_path.write_bytes(encode_png(brand_png))

    tray_img = master.resize((TRAY_SIZE, TRAY_SIZE), Image.Resampling.LANCZOS).convert(
        "RGBA"
    )
    tray_rgba_path = ICONS_DIR / "tray-64.rgba"
    tray_rgba_path.write_bytes(tray_img.tobytes())

    for path in (ico_path, icns_path, brand_png_path, tray_rgba_path):
        print(f"Generated {path.relative_to(REPO_ROOT)} ({path.stat().st_size:,} bytes)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
