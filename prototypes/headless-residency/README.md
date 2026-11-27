# DevTools Two-Stage Headless Residency Prototype (Issue #12)

> **Throwaway prototype** on branch `prototype/headless-residency` — validates ADR-0001 (`gpui-kit = 0.11.0` / `gpui-pre = 0.3.14`) and ADR-0002 two-stage headless background residency on Windows.

## Prerequisites

- Rust **1.95+** (pinned in `rust-toolchain.toml` — `gpui-pre 0.3.14` uses `std::hint::cold_path()`, stabilized in 1.95)
- Windows 10/11 SDK with `fxc.exe` on `PATH` for Release builds

## Quick Run

```powershell
# Add fxc.exe to PATH if needed (example):
$env:PATH = "D:\Windows Kits\10\bin\10.0.26100.0\x64;$env:PATH"

# Interactive mode (5s Warm Standby -> Deep Sleep grace period for quick manual check)
cargo run --release -- --grace 5

# Automated benchmark & 20-cycle (40 rebuilds) stress suite
cargo run --release -- --bench

# Silent Headless cold boot + IPC wake verification
.\target\release\devtools-residency-prototype.exe --silent
.\target\release\devtools-residency-prototype.exe --toggle
.\target\release\devtools-residency-prototype.exe --quit-running
```
