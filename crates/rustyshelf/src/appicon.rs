//! RustyShelf's icon on its own window (title bar, taskbar, Alt+Tab): the exe's icon resource, which build.rs
//! embeds, given to the window once it exists. Windows only; elsewhere nothing.

use bevy::prelude::*;

/// Tries every frame until the window has the icon, then stops.
pub fn set(mut done: Local<bool>) {
    if !*done {
        *done = win::apply();
    }
}

#[cfg(windows)]
mod win {
    use std::ffi::c_void;

    const WM_SETICON: u32 = 0x0080;
    const ICON_SMALL: usize = 0;
    const ICON_BIG: usize = 1;
    const IMAGE_ICON: u32 = 1;
    const LR_SHARED: u32 = 0x8000;
    const SM_CXICON: i32 = 11;
    const SM_CXSMICON: i32 = 49;

    #[link(name = "kernel32")]
    extern "system" {
        fn GetModuleHandleW(name: *const u16) -> *mut c_void;
        fn GetCurrentProcessId() -> u32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn EnumWindows(each: unsafe extern "system" fn(*mut c_void, isize) -> i32, with: isize) -> i32;
        fn GetWindowThreadProcessId(window: *mut c_void, pid: *mut u32) -> u32;
        fn IsWindowVisible(window: *mut c_void) -> i32;
        fn LoadImageW(module: *mut c_void, name: *const u16, kind: u32, cx: i32, cy: i32, flags: u32) -> *mut c_void;
        fn GetSystemMetrics(index: i32) -> i32;
        fn SendMessageW(window: *mut c_void, msg: u32, wparam: usize, lparam: isize) -> isize;
    }

    /// `true` once one of this process's visible windows has been given the icon.
    pub fn apply() -> bool {
        struct Found {
            pid: u32,
            windows: Vec<*mut c_void>,
        }
        unsafe extern "system" fn each(window: *mut c_void, with: isize) -> i32 {
            // SAFETY: `with` is the Found that EnumWindows was given below, alive for the whole call
            let found = &mut *(with as *mut Found);
            let mut pid = 0;
            GetWindowThreadProcessId(window, &mut pid);
            if pid == found.pid && IsWindowVisible(window) != 0 {
                found.windows.push(window);
            }
            1
        }
        // SAFETY: plain Win32 calls; the icon is resource 1 of this exe (MAKEINTRESOURCE(1)), loaded shared so it is
        // never freed while the window shows it
        unsafe {
            let mut found = Found { pid: GetCurrentProcessId(), windows: Vec::new() };
            EnumWindows(each, &mut found as *mut Found as isize);
            if found.windows.is_empty() {
                return false;
            }
            let module = GetModuleHandleW(std::ptr::null());
            // MAKEINTRESOURCE(1): the icon's resource id, not a pointer
            #[allow(clippy::manual_dangling_ptr)]
            let load = |metric| {
                let size = GetSystemMetrics(metric);
                LoadImageW(module, 1usize as *const u16, IMAGE_ICON, size, size, LR_SHARED)
            };
            let (big, small) = (load(SM_CXICON), load(SM_CXSMICON));
            if big.is_null() {
                // (a build without the resource) nothing to give; stop trying
                return true;
            }
            let small = if small.is_null() { big } else { small };
            for window in found.windows {
                SendMessageW(window, WM_SETICON, ICON_BIG, big as isize);
                SendMessageW(window, WM_SETICON, ICON_SMALL, small as isize);
            }
            true
        }
    }
}

#[cfg(not(windows))]
mod win {
    pub fn apply() -> bool {
        true
    }
}
