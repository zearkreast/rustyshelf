// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from the
// quit combo of Boxshelf's main/padwatch.js, as RustyShelf has it.
//! The pads while a game runs. On Windows a pad is only heard by the window in front, which is the game's then, so
//! XInput is read directly on a thread of its own; elsewhere the shelf's own pads keep answering.

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
const B: u16 = 0x2000;

/// How long the combo is held before it counts (ms).
pub const HOLD: u64 = 900;

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
        let Some(since) = self.0.get_mut(pad) else { return false };
        if buttons & mask != mask {
            *since = None;
            return false;
        }
        now - *since.get_or_insert(now) >= HOLD
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

/// Listens for the quit combo while a game runs.
pub struct Watch {
    mask: u16,
    held: Held,
    stop: Arc<AtomicBool>,
    quit: Option<Mutex<Receiver<()>>>,
}

impl Watch {
    /// `pid`: the game's process (0: not known). The combo only counts while it (or a program it started, or
    /// RustyShelf) is in front: held in another app the player switched to, it would close the game behind their
    /// back.
    pub fn start(combo: &str, pid: u32) -> Watch {
        let (mask, stop) = (mask(combo), Arc::new(AtomicBool::new(false)));
        let quit = xinput::watch(mask, pid, stop.clone()).map(Mutex::new);
        Watch { mask, held: Held::default(), stop, quit }
    }

    /// True once the combo was held.
    pub fn quit(&mut self, pads: &Query<&Gamepad>, now: u64) -> bool {
        match &self.quit {
            Some(rx) => rx.lock().unwrap().try_recv().is_ok(),
            None => held_here(&mut self.held, pads, self.mask, now),
        }
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

#[cfg(not(windows))]
mod xinput {
    use super::*;

    pub fn watch(_mask: u16, _pid: u32, _stop: Arc<AtomicBool>) -> Option<Receiver<()>> {
        None
    }
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
    }

    /// XInput's read of a pad, from the library this Windows has.
    fn reader() -> Option<Get> {
        for dll in [c"xinput1_4.dll", c"xinput9_1_0.dll"] {
            // SAFETY: a system library loaded by its name, and its exported XInputGetState, whose signature this is
            unsafe {
                let lib = LoadLibraryA(dll.as_ptr());
                if lib.is_null() {
                    continue;
                }
                let get = GetProcAddress(lib, c"XInputGetState".as_ptr());
                if !get.is_null() {
                    return Some(std::mem::transmute::<*mut c_void, Get>(get));
                }
            }
        }
        None
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

    pub fn watch(mask: u16, pid: u32, stop: Arc<AtomicBool>) -> Option<Receiver<()>> {
        let get = reader()?;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            // SAFETY: reads this process's own id
            let own = unsafe { GetCurrentProcessId() };
            let (start, mut held) = (Instant::now(), Held::default());
            // an empty slot is slow to ask (XInput looks for a device each time): again after a while
            let mut retry = [start; 4];
            let (mut fam, mut fam_at) = (family(pid), start);
            while !stop.load(Ordering::Relaxed) {
                let now = Instant::now();
                for (slot, retry) in retry.iter_mut().enumerate() {
                    if now < *retry {
                        continue;
                    }
                    let mut state = State::default();
                    // SAFETY: XInputGetState with a state of its own to fill
                    if unsafe { get(slot as u32, &mut state) } != 0 {
                        *retry = now + Duration::from_millis(1500);
                        held.seen(slot, 0, mask, 0);
                        continue;
                    }
                    if !held.seen(slot, state.buttons, mask, now.duration_since(start).as_millis() as u64) {
                        continue;
                    }
                    let front = front_pid();
                    // a launcher's child started since the last look
                    if front != own && !fam.contains(&front) && now.duration_since(fam_at) > Duration::from_secs(2) {
                        (fam, fam_at) = (family(pid), now);
                    }
                    if pid == 0 || front == own || fam.contains(&front) {
                        let _ = tx.send(());
                        return;
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
}
