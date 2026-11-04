//! Helpers for reporting peak process memory.


pub fn peak_rss_bytes() -> Option<u64> {
    #[cfg(windows)]
    {
        windows_peak_rss_bytes()
    }

    #[cfg(target_os = "linux")]
    {
        linux_peak_rss_bytes()
    }

    #[cfg(target_os = "macos")]
    {
        macos_peak_rss_bytes()
    }

    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

#[cfg(windows)]
fn windows_peak_rss_bytes() -> Option<u64> {
    use std::ffi::c_void;

    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
        fn K32GetProcessMemoryInfo(
            process: *mut c_void,
            counters: *mut ProcessMemoryCounters,
            size: u32,
        ) -> i32;
    }

    let mut counters: ProcessMemoryCounters = unsafe { std::mem::zeroed() };
    counters.cb = std::mem::size_of::<ProcessMemoryCounters>() as u32;

    let ok = unsafe {
        K32GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb)
    };

    if ok != 0 {
        Some(counters.peak_working_set_size as u64)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn linux_peak_rss_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;

    status.lines().find_map(|line| {
        let rest = line.strip_prefix("VmHWM:")?;
        let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;

        Some(kb * 1024)
    })
}

#[cfg(target_os = "macos")]
fn macos_peak_rss_bytes() -> Option<u64> {
    #[repr(C)]
    struct TimeVal {
        tv_sec: i64,
        tv_usec: i32,
        _pad: i32,
    }

    #[repr(C)]
    struct Rusage {
        utime: TimeVal,
        stime: TimeVal,
        maxrss: i64,
        ixrss: i64,
        idrss: i64,
        isrss: i64,
        minflt: i64,
        majflt: i64,
        nswap: i64,
        inblock: i64,
        oublock: i64,
        msgsnd: i64,
        msgrcv: i64,
        nsignals: i64,
        nvcsw: i64,
        nivcsw: i64,
    }

    const RUSAGE_SELF: i32 = 0;

    unsafe extern "C" {
        fn getrusage(who: i32, usage: *mut Rusage) -> i32;
    }

    let mut usage: Rusage = unsafe { std::mem::zeroed() };

    if unsafe { getrusage(RUSAGE_SELF, &mut usage) } == 0 {
        // On macOS 'ru_maxrss' is already in bytes.
        Some(usage.maxrss.max(0) as u64)
    } else {
        None
    }
}
