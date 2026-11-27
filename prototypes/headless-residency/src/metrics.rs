//! Live Windows process resource telemetry (CPU%, RSS, Private Commit, DXGI VRAM, thread count).
//! Throwaway prototype for Issue #12.

#[derive(Debug, Clone, Copy, Default)]
pub struct ResourceSnapshot {
    pub working_set_mb: f64,
    pub private_mb: f64,
    pub gpu_dedicated_mb: f64,
    pub gpu_shared_mb: f64,
    pub thread_count: u32,
    pub cpu_time_100ns: u64,
}

#[cfg(windows)]
mod win {
    use super::ResourceSnapshot;
    use std::mem::size_of;
    use windows::Win32::Foundation::{CloseHandle, FILETIME};
    use windows::Win32::Graphics::Dxgi::{
        CreateDXGIFactory1, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL,
        DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter3, IDXGIFactory4,
    };
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
    };
    use windows::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows::Win32::System::Threading::{
        GetCurrentProcess, GetCurrentProcessId, GetProcessTimes,
    };
    use windows::core::Interface;

    fn filetime_to_u64(ft: FILETIME) -> u64 {
        ((ft.dwHighDateTime as u64) << 32) | (ft.dwLowDateTime as u64)
    }

    fn count_process_threads(pid: u32) -> u32 {
        unsafe {
            let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else {
                return 0;
            };
            let mut entry = THREADENTRY32 {
                dwSize: size_of::<THREADENTRY32>() as u32,
                ..Default::default()
            };
            let mut count = 0u32;
            if Thread32First(snap, &mut entry).is_ok() {
                loop {
                    if entry.th32OwnerProcessID == pid {
                        count += 1;
                    }
                    if Thread32Next(snap, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
            count
        }
    }

    fn query_dxgi_vram_mb() -> (f64, f64) {
        unsafe {
            let Ok(factory) = CreateDXGIFactory1::<IDXGIFactory4>() else {
                return (0.0, 0.0);
            };
            let mut dedicated = 0u64;
            let mut shared = 0u64;
            let mut idx = 0u32;
            while let Ok(adapter) = factory.EnumAdapters1(idx) {
                idx += 1;
                if let Ok(adapter3) = adapter.cast::<IDXGIAdapter3>() {
                    let mut local = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                    if adapter3
                        .QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut local)
                        .is_ok()
                    {
                        dedicated = dedicated.saturating_add(local.CurrentUsage);
                    }
                    let mut non_local = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
                    if adapter3
                        .QueryVideoMemoryInfo(
                            0,
                            DXGI_MEMORY_SEGMENT_GROUP_NON_LOCAL,
                            &mut non_local,
                        )
                        .is_ok()
                    {
                        shared = shared.saturating_add(non_local.CurrentUsage);
                    }
                }
            }
            let mb = 1024.0 * 1024.0;
            (dedicated as f64 / mb, shared as f64 / mb)
        }
    }

    pub fn snapshot_now() -> ResourceSnapshot {
        unsafe {
            let proc_handle = GetCurrentProcess();
            let pid = GetCurrentProcessId();

            let mut mem = PROCESS_MEMORY_COUNTERS_EX::default();
            let mem_ok = GetProcessMemoryInfo(
                proc_handle,
                &mut mem as *mut PROCESS_MEMORY_COUNTERS_EX as *mut PROCESS_MEMORY_COUNTERS,
                size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32,
            )
            .is_ok();

            let mut creation = FILETIME::default();
            let mut exit = FILETIME::default();
            let mut kernel = FILETIME::default();
            let mut user = FILETIME::default();
            let _ = GetProcessTimes(
                proc_handle,
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            );
            let cpu_time_100ns = filetime_to_u64(kernel).saturating_add(filetime_to_u64(user));

            let (gpu_dedicated_mb, gpu_shared_mb) = query_dxgi_vram_mb();
            let thread_count = count_process_threads(pid);
            let mb = 1024.0 * 1024.0;

            ResourceSnapshot {
                working_set_mb: if mem_ok {
                    mem.WorkingSetSize as f64 / mb
                } else {
                    0.0
                },
                private_mb: if mem_ok {
                    mem.PrivateUsage as f64 / mb
                } else {
                    0.0
                },
                gpu_dedicated_mb,
                gpu_shared_mb,
                thread_count,
                cpu_time_100ns,
            }
        }
    }
}

#[cfg(windows)]
pub use win::snapshot_now;

pub fn compute_cpu_pct(
    prev_cpu_100ns: u64,
    curr_cpu_100ns: u64,
    elapsed_secs: f64,
) -> (f64, f64) {
    if elapsed_secs <= 0.0 {
        return (0.0, 0.0);
    }
    let delta_secs = curr_cpu_100ns.saturating_sub(prev_cpu_100ns) as f64 / 10_000_000.0;
    let single_core_pct = (delta_secs / elapsed_secs) * 100.0;
    let cores = std::thread::available_parallelism()
        .map(|n| n.get() as f64)
        .unwrap_or(1.0);
    (single_core_pct, single_core_pct / cores)
}
