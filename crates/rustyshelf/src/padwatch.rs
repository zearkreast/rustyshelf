// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/padwatch.js (the quit combo, Home, the hotkeys sent to a game and the window brought to the
// front), as RustyShelf has it.
//! The pads while a game runs. On Windows a pad is only heard by the window in front, which is the game's then, so
//! XInput is read directly on a thread of its own; elsewhere the shelf's own pads keep answering. Also what the
//! Home menu does to the game from outside: its emulator's hotkeys, and which window is in front.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use bevy::prelude::*;

// XInput's button bits; every pad is read as one by position
const START: u16 = 0x0010;
const BACK: u16 = 0x0020;
const L3: u16 = 0x0040;
const R3: u16 = 0x0080;
const LB: u16 = 0x0100;
const RB: u16 = 0x0200;
#[cfg(windows)]
const GUIDE: u16 = 0x0400;
const B: u16 = 0x2000;

/// How long the combo is held before it counts (ms).
pub const HOLD: u64 = 900;
/// (Linux) Select + Start held this long closes the game whatever the quit combo is (in Steam's Game Mode the Steam
/// button belongs to Steam). Keep in sync with main/linuxpad.js HOLD_MS.
#[cfg(target_os = "linux")]
const SELECT_START_HOLD: u64 = 1500;

/// The buttons of a quit combo as the settings name it (`ui.quitComboKeys`); L3 + R3 for a name that isn't one.
pub fn mask(combo: &str) -> u16 {
    match combo {
        "start+back" => START | BACK,
        "lb+rb+start" => LB | RB | START,
        "back+b" => BACK | B,
        _ => L3 | R3,
    }
}

/// Since when each pad has held the combo.
#[derive(Default)]
pub struct Held([Option<u64>; 8]);

impl Held {
    /// Takes a pad's buttons at `now` (ms); true once it has held all of `mask` for `HOLD`.
    pub fn seen(&mut self, pad: usize, buttons: u16, mask: u16, now: u64) -> bool {
        self.seen_for(pad, buttons, mask, now, HOLD)
    }

    fn seen_for(&mut self, pad: usize, buttons: u16, mask: u16, now: u64, hold: u64) -> bool {
        let Some(since) = self.0.get_mut(pad) else { return false };
        if buttons & mask != mask {
            *since = None;
            return false;
        }
        now - *since.get_or_insert(now) >= hold
    }
}

/// A pad's buttons as XInput's bits, the ones a quit combo can name.
pub fn buttons(pad: &Gamepad) -> u16 {
    [
        (GamepadButton::Start, START),
        (GamepadButton::Select, BACK),
        (GamepadButton::LeftThumb, L3),
        (GamepadButton::RightThumb, R3),
        (GamepadButton::LeftTrigger, LB),
        (GamepadButton::RightTrigger, RB),
        (GamepadButton::East, B),
    ]
    .into_iter()
    .filter(|(b, _)| pad.pressed(*b))
    .fold(0, |all, (_, bit)| all | bit)
}

/// True once any of the shelf's pads has held the combo.
pub fn held_here(held: &mut Held, pads: &Query<&Gamepad>, mask: u16, now: u64) -> bool {
    let mut any = false;
    for (i, pad) in pads.iter().enumerate() {
        any |= held.seen(i, buttons(pad), mask, now);
    }
    any
}

/// What a pad asked for while a game runs.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(not(windows), allow(dead_code))]
pub enum Signal {
    /// the quit combo was held
    Quit,
    /// Guide was tapped, or View held on its own: the Home menu
    Home,
}

/// Since when each pad has held View and nothing else; None once it has counted, until it's let go.
#[derive(Default)]
pub struct ViewHeld([Option<Option<u64>>; 8]);

impl ViewHeld {
    /// Takes a pad's buttons at `now` (ms); true the moment View alone has been held for `HOLD`.
    pub fn seen(&mut self, pad: usize, buttons: u16, now: u64) -> bool {
        let Some(state) = self.0.get_mut(pad) else { return false };
        if buttons != BACK {
            *state = None;
            return false;
        }
        match state.get_or_insert(Some(now)) {
            Some(since) if now - *since >= HOLD => {
                *state = Some(None);
                true
            }
            _ => false,
        }
    }
}

/// Listens for the quit combo and for Home while a game runs.
pub struct Watch {
    mask: u16,
    held: Held,
    view: ViewHeld,
    #[cfg(target_os = "linux")]
    select_start: Held,
    stop: Arc<AtomicBool>,
    signals: Option<Mutex<Receiver<Signal>>>,
    quit: bool,
    home: bool,
}

impl Watch {
    /// `combo`: the quit combo, when it's on. `pid`: the game's process (0: not known). A signal only counts
    /// while the game (or a program it started, or RustyShelf) is in front: given in another app the player
    /// switched to, it would close or interrupt the game behind their back.
    pub fn start(combo: Option<&str>, pid: u32) -> Watch {
        let (mask, stop) = (combo.map_or(0, mask), Arc::new(AtomicBool::new(false)));
        let signals = xinput::watch(mask, pid, stop.clone()).map(Mutex::new);
        Watch {
            mask,
            held: Held::default(),
            view: ViewHeld::default(),
            #[cfg(target_os = "linux")]
            select_start: Held::default(),
            stop,
            signals,
            quit: false,
            home: false,
        }
    }

    fn hear(&mut self, pads: &Query<&Gamepad>, now: u64) {
        match &self.signals {
            Some(rx) => {
                for signal in rx.lock().unwrap().try_iter() {
                    match signal {
                        Signal::Quit => self.quit = true,
                        Signal::Home => self.home = true,
                    }
                }
            }
            None => {
                self.quit |= self.mask != 0 && held_here(&mut self.held, pads, self.mask, now);
                #[cfg(target_os = "linux")]
                for (i, pad) in pads.iter().enumerate() {
                    self.quit |= self.select_start.seen_for(i, buttons(pad), BACK | START, now, SELECT_START_HOLD);
                }
                for (i, pad) in pads.iter().enumerate() {
                    self.home |= pad.just_pressed(GamepadButton::Mode) | self.view.seen(i, buttons(pad), now);
                }
            }
        }
    }

    /// True once the combo was held.
    pub fn quit(&mut self, pads: &Query<&Gamepad>, now: u64) -> bool {
        self.hear(pads, now);
        self.quit
    }

    /// True once for each time Home was asked for.
    pub fn home(&mut self, pads: &Query<&Gamepad>, now: u64) -> bool {
        self.hear(pads, now);
        std::mem::take(&mut self.home)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Sends a game its emulator's hotkeys ("f10", "shift+f1"; "wait150" waits that many milliseconds), with the
/// game's own window in front first. False when that window couldn't be brought to the front: keys sent then
/// would land somewhere else. Takes a moment: not for the frame's thread.
pub fn send_keys(pid: u32, keys: &[String]) -> bool {
    xinput::send_keys(pid, keys)
}

/// Brings RustyShelf's own window to the front.
pub fn front_self() {
    xinput::front_self();
}

/// Puts the PC to sleep ("sleep"), restarts it ("restart") or shuts it down ("shutdown").
pub fn power(kind: &str) {
    rustyshelf_core::settings::flush();
    info!("Power: {kind}");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const NO_WINDOW: u32 = 0x0800_0000;
        let mut command = match kind {
            "sleep" => {
                let mut c = std::process::Command::new("powershell.exe");
                c.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", "Add-Type -AssemblyName System.Windows.Forms; [void][System.Windows.Forms.Application]::SetSuspendState('Suspend', $false, $false)"]);
                c
            }
            "restart" | "shutdown" => {
                let mut c = std::process::Command::new("shutdown.exe");
                c.args([if kind == "restart" { "/r" } else { "/s" }, "/t", "0"]);
                c
            }
            _ => return,
        };
        if let Err(e) = command.creation_flags(NO_WINDOW).spawn() {
            warn!("Power failed: {e}");
        }
    }
}

#[cfg(not(windows))]
mod xinput {
    use super::*;

    pub fn watch(_mask: u16, _pid: u32, _stop: Arc<AtomicBool>) -> Option<Receiver<Signal>> {
        None
    }

    pub fn send_keys(_pid: u32, _keys: &[String]) -> bool {
        true
    }

    pub fn front_self() {}
}

#[cfg(windows)]
mod xinput {
    use std::ffi::{c_char, c_void};
    use std::sync::mpsc::channel;
    use std::time::{Duration, Instant};

    use super::*;

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct State {
        packet: u32,
        buttons: u16,
        left_trigger: u8,
        right_trigger: u8,
        thumbs: [i16; 4],
        // the read that also tells Guide fills four bytes more
        reserved: u32,
    }

    #[repr(C)]
    #[derive(Default, Clone, Copy)]
    struct Rect {
        left: i32,
        top: i32,
        right: i32,
        bottom: i32,
    }

    #[repr(C)]
    #[derive(Default)]
    struct MonitorInfo {
        size: u32,
        monitor: Rect,
        work: Rect,
        flags: u32,
    }

    #[repr(C)]
    struct Entry {
        size: u32,
        usage: u32,
        pid: u32,
        heap: usize,
        module: u32,
        threads: u32,
        parent: u32,
        priority: i32,
        flags: u32,
        exe: [u16; 260],
    }

    type Get = unsafe extern "system" fn(u32, *mut State) -> u32;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryA(name: *const c_char) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
        fn GetCurrentProcessId() -> u32;
        fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
        fn Process32FirstW(snapshot: *mut c_void, entry: *mut Entry) -> i32;
        fn Process32NextW(snapshot: *mut c_void, entry: *mut Entry) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }

    #[link(name = "user32")]
    extern "system" {
        fn GetForegroundWindow() -> *mut c_void;
        fn GetWindowThreadProcessId(window: *mut c_void, pid: *mut u32) -> u32;
        fn SetForegroundWindow(window: *mut c_void) -> i32;
        fn EnumWindows(each: unsafe extern "system" fn(*mut c_void, isize) -> i32, with: isize) -> i32;
        fn IsWindowVisible(window: *mut c_void) -> i32;
        fn IsIconic(window: *mut c_void) -> i32;
        fn GetWindowRect(window: *mut c_void, rect: *mut Rect) -> i32;
        fn MonitorFromWindow(window: *mut c_void, flags: u32) -> *mut c_void;
        fn GetMonitorInfoW(monitor: *mut c_void, info: *mut MonitorInfo) -> i32;
        fn keybd_event(key: u8, scan: u8, flags: u32, extra: usize);
    }

    /// XInput's read of a pad, from the library this Windows has: the one that also tells Guide where there is
    /// one (xinput1_4's export number 100, which has no name).
    fn reader() -> Option<Get> {
        for (dll, named) in [(c"xinput1_4.dll", false), (c"xinput1_4.dll", true), (c"xinput9_1_0.dll", true)] {
            // SAFETY: a system library loaded by its name, and its XInputGetState (or the same read with Guide),
            // whose signature this is
            unsafe {
                let lib = LoadLibraryA(dll.as_ptr());
                if lib.is_null() {
                    continue;
                }
                let get = GetProcAddress(lib, if named { c"XInputGetState".as_ptr() } else { 100usize as *const c_char });
                if !get.is_null() {
                    return Some(std::mem::transmute::<*mut c_void, Get>(get));
                }
            }
        }
        None
    }

    /// The window to send keys to: one of these processes' fullscreen window, else their largest visible one (an
    /// emulator's game window, not its game list).
    fn game_window(pids: &[u32]) -> *mut c_void {
        struct Best<'a> {
            pids: &'a [u32],
            window: *mut c_void,
            area: i64,
            full: bool,
        }
        unsafe extern "system" fn each(window: *mut c_void, with: isize) -> i32 {
            // SAFETY: `with` is the Best that EnumWindows was given below, alive for the whole call
            let best = &mut *(with as *mut Best);
            if IsWindowVisible(window) == 0 || IsIconic(window) != 0 {
                return 1;
            }
            let (mut pid, mut r) = (0, Rect::default());
            GetWindowThreadProcessId(window, &mut pid);
            if !best.pids.contains(&pid) || GetWindowRect(window, &mut r) == 0 {
                return 1;
            }
            let area = (r.right - r.left) as i64 * (r.bottom - r.top) as i64;
            if area < 64 * 64 {
                return 1;
            }
            let mut info = MonitorInfo { size: std::mem::size_of::<MonitorInfo>() as u32, ..MonitorInfo::default() };
            const NEAREST: u32 = 2;
            let full = GetMonitorInfoW(MonitorFromWindow(window, NEAREST), &mut info) != 0 && { let m = info.monitor; r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom };
            if (full && !best.full) || (full == best.full && area > best.area) {
                (best.window, best.area, best.full) = (window, area, full);
            }
            1
        }
        let mut best = Best { pids, window: std::ptr::null_mut(), area: 0, full: false };
        // SAFETY: every top-level window is shown to `each`, with `best` to fill
        unsafe {
            EnumWindows(each, &mut best as *mut Best as isize);
        }
        best.window
    }

    fn key(vk: u8, down: bool) {
        const UP: u32 = 2;
        // SAFETY: a key press or release, as from the keyboard
        unsafe { keybd_event(vk, 0, if down { 0 } else { UP }, 0) }
    }

    /// Brings a window to the front; true once it is.
    fn front(window: *mut c_void) -> bool {
        // SAFETY: user32 calls on a window handle that EnumWindows gave
        unsafe {
            if GetForegroundWindow() == window {
                return true;
            }
            for _ in 0..3 {
                // F24, a key nothing uses: the last process to send input may move the foreground
                const F24: u8 = 0x87;
                key(F24, true);
                key(F24, false);
                SetForegroundWindow(window);
                std::thread::sleep(Duration::from_millis(80));
                if GetForegroundWindow() == window {
                    return true;
                }
            }
        }
        false
    }

    pub fn send_keys(pid: u32, keys: &[String]) -> bool {
        if pid != 0 {
            let fam = family(pid);
            let window = game_window(&fam);
            if window.is_null() {
                if !fam.contains(&front_pid()) {
                    return false;
                }
            } else if !front(window) {
                return false;
            }
        }
        for spec in keys {
            if let Some(ms) = spec.strip_prefix("wait").and_then(|n| n.parse::<u64>().ok()) {
                std::thread::sleep(Duration::from_millis(ms));
                continue;
            }
            let Some((mods, vk)) = rustyshelf_core::homemenu::virtual_keys(spec) else { continue };
            mods.iter().for_each(|m| key(*m, true));
            key(vk, true);
            key(vk, false);
            std::thread::sleep(Duration::from_millis(30));
            mods.iter().for_each(|m| key(*m, false));
            std::thread::sleep(Duration::from_millis(90));
        }
        true
    }

    pub fn front_self() {
        // SAFETY: reads this process's own id
        let own = unsafe { GetCurrentProcessId() };
        let window = game_window(&[own]);
        if !window.is_null() {
            front(window);
        }
    }

    fn front_pid() -> u32 {
        let mut pid = 0;
        // SAFETY: user32 calls that read which window is in front and whose it is
        unsafe {
            GetWindowThreadProcessId(GetForegroundWindow(), &mut pid);
        }
        pid
    }

    /// A process and every process started under it.
    fn family(root: u32) -> Vec<u32> {
        const PROCESSES: u32 = 2;
        let mut all: Vec<(u32, u32)> = Vec::new();
        // SAFETY: a snapshot of the process list, walked with an entry of the size the calls are told, then closed
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(PROCESSES, 0);
            if snapshot as isize != -1 {
                let mut e: Entry = std::mem::zeroed();
                e.size = std::mem::size_of::<Entry>() as u32;
                let mut more = Process32FirstW(snapshot, &mut e);
                while more != 0 {
                    all.push((e.pid, e.parent));
                    more = Process32NextW(snapshot, &mut e);
                }
                CloseHandle(snapshot);
            }
        }
        let mut set = vec![root];
        let mut grew = true;
        while grew {
            grew = false;
            for (pid, parent) in &all {
                if set.contains(parent) && !set.contains(pid) {
                    set.push(*pid);
                    grew = true;
                }
            }
        }
        set
    }

    /// Nintendo, PlayStation and PowerA pads over HID: looked for every 2 s once a game runs, each one found read on a
    /// thread of its own, as the Electron app's helper reads them.
    mod hid {
        use std::ffi::c_void;
        use std::sync::{Arc, Mutex, OnceLock};
        use std::time::{Duration, Instant};

        use rustyshelf_core::hidpads::{self, Kind, Report};

        #[repr(C)]
        #[derive(Default, Clone, Copy)]
        struct Guid(u32, u16, u16, [u8; 8]);

        #[repr(C)]
        struct InterfaceData {
            size: u32,
            class: Guid,
            flags: u32,
            reserved: usize,
        }

        #[repr(C)]
        #[derive(Default)]
        struct Attributes {
            size: u32,
            vid: u16,
            pid: u16,
            version: u16,
        }

        #[link(name = "hid")]
        extern "system" {
            fn HidD_GetHidGuid(guid: *mut Guid);
            fn HidD_GetAttributes(h: *mut c_void, a: *mut Attributes) -> u8;
        }
        #[link(name = "setupapi")]
        extern "system" {
            fn SetupDiGetClassDevsW(guid: *const Guid, enumerator: *const u16, parent: *mut c_void, flags: u32) -> *mut c_void;
            fn SetupDiEnumDeviceInterfaces(set: *mut c_void, info: *mut c_void, guid: *const Guid, index: u32, data: *mut InterfaceData) -> i32;
            fn SetupDiGetDeviceInterfaceDetailW(set: *mut c_void, data: *mut InterfaceData, detail: *mut u8, size: u32, required: *mut u32, info: *mut c_void) -> i32;
            fn SetupDiDestroyDeviceInfoList(set: *mut c_void) -> i32;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn CreateFileW(name: *const u16, access: u32, share: u32, security: *mut c_void, disposition: u32, flags: u32, template: *mut c_void) -> *mut c_void;
            fn ReadFile(h: *mut c_void, buf: *mut u8, n: u32, got: *mut u32, overlapped: *mut c_void) -> i32;
            fn CloseHandle(h: *mut c_void) -> i32;
        }

        struct Pad {
            path: String,
            kind: Kind,
            last: Option<(Report, Instant)>,
        }

        static PADS: Mutex<Vec<Arc<Mutex<Pad>>>> = Mutex::new(Vec::new());
        static SCAN: OnceLock<()> = OnceLock::new();

        /// The paths of the HID devices plugged in now.
        fn paths() -> Vec<String> {
            let mut out = Vec::new();
            let mut guid = Guid::default();
            // SAFETY: SetupAPI's walk over the HID interfaces present, each detail read into a buffer of the size it
            // asks for (its first four bytes the structure's size), the list destroyed at the end
            unsafe {
                HidD_GetHidGuid(&mut guid);
                let set = SetupDiGetClassDevsW(&guid, std::ptr::null(), std::ptr::null_mut(), 0x12);
                if set.is_null() || set as isize == -1 {
                    return out;
                }
                for i in 0..256 {
                    let mut d = InterfaceData { size: std::mem::size_of::<InterfaceData>() as u32, class: Guid::default(), flags: 0, reserved: 0 };
                    if SetupDiEnumDeviceInterfaces(set, std::ptr::null_mut(), &guid, i, &mut d) == 0 {
                        break;
                    }
                    let mut need = 0u32;
                    SetupDiGetDeviceInterfaceDetailW(set, &mut d, std::ptr::null_mut(), 0, &mut need, std::ptr::null_mut());
                    if need < 8 {
                        continue;
                    }
                    let mut buf = vec![0u8; need as usize];
                    buf[..4].copy_from_slice(&(if cfg!(target_pointer_width = "64") { 8u32 } else { 6u32 }).to_le_bytes());
                    if SetupDiGetDeviceInterfaceDetailW(set, &mut d, buf.as_mut_ptr(), need, &mut need, std::ptr::null_mut()) == 0 {
                        continue;
                    }
                    let wide: Vec<u16> = buf[4..].chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).take_while(|&c| c != 0).collect();
                    out.push(String::from_utf16_lossy(&wide));
                }
                SetupDiDestroyDeviceInfoList(set);
            }
            out
        }

        fn scan() {
            for path in paths() {
                let lp = path.to_lowercase();
                let maker = ["vid_057e", "vid&0002057e", "vid_054c", "vid&0002054c", "vid_20d6&pid_a713"].iter().any(|v| lp.contains(v));
                if !maker || PADS.lock().unwrap().iter().any(|p| p.lock().unwrap().path == lp) {
                    continue;
                }
                let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
                // SAFETY: a device path from SetupAPI, opened to read (shared, so the game's emulator can open it too)
                let h = unsafe { CreateFileW(wide.as_ptr(), 0x8000_0000, 3, std::ptr::null_mut(), 3, 0, std::ptr::null_mut()) };
                if h.is_null() || h as isize == -1 {
                    continue;
                }
                let mut a = Attributes { size: std::mem::size_of::<Attributes>() as u32, ..Attributes::default() };
                // SAFETY: the handle just opened, and attributes of the size they're told
                let kind = (unsafe { HidD_GetAttributes(h, &mut a) } != 0).then(|| hidpads::kind(a.vid, a.pid, lp.contains("00001124-0000-1000-8000-00805f9b34fb"))).flatten();
                let Some(kind) = kind else {
                    // SAFETY: the handle just opened
                    unsafe { CloseHandle(h) };
                    continue;
                };
                let pad = Arc::new(Mutex::new(Pad { path: lp, kind, last: None }));
                PADS.lock().unwrap().push(pad.clone());
                let h = h as usize;
                std::thread::spawn(move || {
                    let mut buf = [0u8; 1024];
                    let mut got = 0u32;
                    // SAFETY: blocking reads into a buffer of the size given, on the handle this thread owns
                    while unsafe { ReadFile(h as *mut c_void, buf.as_mut_ptr(), buf.len() as u32, &mut got, std::ptr::null_mut()) } != 0 {
                        if let Some(r) = hidpads::parse(kind, &buf[..got as usize]) {
                            pad.lock().unwrap().last = Some((r, Instant::now()));
                        }
                    }
                    // unplugged or out of range: the scan finds it again when it's back
                    PADS.lock().unwrap().retain(|p| !Arc::ptr_eq(p, &pad));
                    // SAFETY: the handle this thread owns, closed once
                    unsafe { CloseHandle(h as *mut c_void) };
                });
            }
        }

        /// What the Nintendo pads (and PowerA's), then the PlayStation ones, hold now, each as one XInput slot.
        pub fn slots() -> [Option<u16>; 2] {
            SCAN.get_or_init(|| {
                std::thread::spawn(|| loop {
                    scan();
                    std::thread::sleep(Duration::from_secs(2));
                });
            });
            let pads = PADS.lock().unwrap();
            let of = |ps: bool| {
                let list: Vec<(Report, u64)> = pads
                    .iter()
                    .filter_map(|p| {
                        let p = p.lock().unwrap();
                        let is_ps = matches!(p.kind, Kind::PlayStation { .. });
                        p.last.filter(|_| is_ps == ps).map(|(r, at)| (r, at.elapsed().as_millis() as u64))
                    })
                    .collect();
                hidpads::combined(&list)
            };
            [of(false), of(true)]
        }
    }

    pub fn watch(mask: u16, pid: u32, stop: Arc<AtomicBool>) -> Option<Receiver<Signal>> {
        let get = reader()?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            // SAFETY: reads this process's own id
            let own = unsafe { GetCurrentProcessId() };
            let (start, mut held, mut view) = (Instant::now(), Held::default(), ViewHeld::default());
            // an empty slot is slow to ask (XInput looks for a device each time): again after a while
            let mut retry = [start; 4];
            let mut before = [0u16; 6];
            let (mut fam, mut fam_at) = (family(pid), start);
            while !stop.load(Ordering::Relaxed) {
                let now = Instant::now();
                let hid = hid::slots();
                for slot in 0..6 {
                    let mut state = State::default();
                    if slot >= 4 {
                        let Some(b) = hid[slot - 4] else {
                            held.seen(slot, 0, mask, 0);
                            before[slot] = 0;
                            continue;
                        };
                        state.buttons = b;
                    } else {
                        if now < retry[slot] {
                            continue;
                        }
                        // SAFETY: XInputGetState with a state of its own to fill
                        if unsafe { get(slot as u32, &mut state) } != 0 {
                            retry[slot] = now + Duration::from_millis(1500);
                            held.seen(slot, 0, mask, 0);
                            before[slot] = 0;
                            continue;
                        }
                    }
                    let ms = now.duration_since(start).as_millis() as u64;
                    let quit = mask != 0 && held.seen(slot, state.buttons, mask, ms);
                    let home = (state.buttons & GUIDE != 0 && before[slot] & GUIDE == 0) | view.seen(slot, state.buttons, ms);
                    before[slot] = state.buttons;
                    if !quit && !home {
                        continue;
                    }
                    let front = front_pid();
                    // a launcher's child started since the last look
                    if front != own && !fam.contains(&front) && now.duration_since(fam_at) > Duration::from_secs(2) {
                        (fam, fam_at) = (family(pid), now);
                    }
                    if pid == 0 || front == own || fam.contains(&front) {
                        if quit {
                            let _ = tx.send(Signal::Quit);
                            return;
                        }
                        if tx.send(Signal::Home).is_err() {
                            return;
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(8));
            }
        });
        Some(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_longer_hold() {
        let mut h = Held::default();
        assert!(!h.seen_for(0, BACK | START, BACK | START, 0, 1500));
        assert!(!h.seen_for(0, BACK | START, BACK | START, 1400, 1500));
        assert!(h.seen_for(0, BACK | START | B, BACK | START, 1500, 1500));
    }

    #[test]
    fn combo_counts_once_held() {
        let m = mask("l3+r3");
        assert_eq!((m, mask("nonsense"), mask("start+back"), mask("lb+rb+start"), mask("back+b")), (0xc0, 0xc0, 0x30, 0x310, 0x2020));
        let mut held = Held::default();
        assert!(!held.seen(0, 0xc0, m, 1000));
        assert!(!held.seen(0, 0xc0 | 0x1000, m, 1899));
        assert!(held.seen(0, 0xc0, m, 1900));
        // letting go starts the count again, and each pad has its own
        assert!(!held.seen(0, 0x40, m, 1950));
        assert!(!held.seen(0, 0xc0, m, 2000));
        assert!(!held.seen(1, 0xc0, m, 2800));
        assert!(held.seen(0, 0xc0, m, 2900));
        assert!(!held.seen(1, 0xc0, m, 3699));
        assert!(!held.seen(99, 0xc0, m, 9999));
    }

    #[test]
    fn view_counts_once_when_held_alone() {
        let mut view = ViewHeld::default();
        assert!(!view.seen(0, BACK, 1000));
        assert!(!view.seen(0, BACK, 1899));
        assert!(view.seen(0, BACK, 1900));
        // not again until it's let go
        assert!(!view.seen(0, BACK, 5000));
        assert!(!view.seen(0, 0, 5100));
        assert!(!view.seen(0, BACK, 5200));
        // with another button it's a combo, not Home
        assert!(!view.seen(0, BACK | B, 6000));
        assert!(!view.seen(0, BACK, 6100));
        assert!(!view.seen(0, BACK, 6999));
        assert!(view.seen(0, BACK, 7000));
    }
}
