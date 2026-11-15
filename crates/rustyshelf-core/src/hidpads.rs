// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from the
// HID pads of Boxshelf's main/padwatch.js (its NParse, DParse and PParse), as RustyShelf has it.
//! Nintendo, PlayStation and PowerA pads read over HID while a game runs (Windows: the window in front is the game's,
//! and those pads aren't XInput's): their input reports as XInput's button bits, the pads read by position (the
//! bottom face button is A's 0x1000, the right one B's 0x2000).

/// The pads read this way, by their maker and model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Joy-Con L / R, Pro Controller, the charging grip
    Nintendo,
    /// PowerA's wired Switch pad, a plain HID pad
    PowerA,
    /// DualShock 4 (`dualsense`: false) or DualSense / DualSense Edge; `bluetooth`: paired over Bluetooth
    PlayStation { dualsense: bool, bluetooth: bool },
}

/// The pad a HID device is, from its vendor and product ids and whether its path is Bluetooth's HID service.
pub fn kind(vid: u16, pid: u16, bluetooth: bool) -> Option<Kind> {
    match (vid, pid) {
        (0x057e, 0x2006 | 0x2007 | 0x2009 | 0x200e) => Some(Kind::Nintendo),
        (0x20d6, 0xa713) => Some(Kind::PowerA),
        (0x054c, 0x05c4 | 0x09cc) => Some(Kind::PlayStation { dualsense: false, bluetooth }),
        (0x054c, 0x0ce6 | 0x0df2) => Some(Kind::PlayStation { dualsense: true, bluetooth }),
        _ => None,
    }
}

/// A pad's buttons from one report. `full`: a report that streams many times a second (its pad is gone once they
/// stop); the others only come on a change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Report {
    pub buttons: u16,
    pub full: bool,
}

fn bit(byte: u8, mask: u8, xinput: u16) -> u16 {
    if byte & mask != 0 { xinput } else { 0 }
}

/// A hat switch (0 up, clockwise, 8 none) as the D-pad's bits.
fn hat(h: u8) -> u16 {
    let mut m = 0;
    if h == 7 || h == 0 || h == 1 {
        m |= 0x0001;
    }
    if (1..=3).contains(&h) {
        m |= 0x0008;
    }
    if (3..=5).contains(&h) {
        m |= 0x0002;
    }
    if (5..=7).contains(&h) {
        m |= 0x0004;
    }
    m
}

/// One input report of the pad; None for one that isn't input (a reply to an emulator's own commands).
pub fn parse(kind: Kind, r: &[u8]) -> Option<Report> {
    match kind {
        Kind::Nintendo => nintendo(r),
        Kind::PowerA => powera(r),
        Kind::PlayStation { dualsense, bluetooth } => playstation(r, dualsense, bluetooth),
    }
}

fn nintendo(r: &[u8]) -> Option<Report> {
    let n = r.len();
    // full reports: what emulators / Steam / SDL switch the pad to
    if n >= 12 && matches!(r[0], 0x30 | 0x21 | 0x31) {
        let (b3, b4, b5) = (r[3], r[4], r[5]);
        let m = bit(b3, 0x01, 0x4000) | bit(b3, 0x02, 0x8000) | bit(b3, 0x04, 0x1000) | bit(b3, 0x08, 0x2000) | bit(b3, 0x40, 0x0200)
            | bit(b4, 0x01, 0x0020) | bit(b4, 0x02, 0x0010) | bit(b4, 0x04, 0x0080) | bit(b4, 0x08, 0x0040) | bit(b4, 0x10, 0x0400)
            | bit(b5, 0x01, 0x0002) | bit(b5, 0x02, 0x0001) | bit(b5, 0x04, 0x0008) | bit(b5, 0x08, 0x0004) | bit(b5, 0x40, 0x0100);
        return Some(Report { buttons: m, full: true });
    }
    // simple report: the pad's own mode over Bluetooth before anything set it up
    if n >= 4 && r[0] == 0x3f {
        let (b1, b2) = (r[1], r[2]);
        let m = bit(b1, 0x01, 0x1000) | bit(b1, 0x02, 0x2000) | bit(b1, 0x04, 0x4000) | bit(b1, 0x08, 0x8000) | bit(b1, 0x10, 0x0100) | bit(b1, 0x20, 0x0200)
            | bit(b2, 0x01, 0x0020) | bit(b2, 0x02, 0x0010) | bit(b2, 0x04, 0x0040) | bit(b2, 0x08, 0x0080) | bit(b2, 0x10, 0x0400)
            | hat(r[3]);
        return Some(Report { buttons: m, full: false });
    }
    None
}

/// No report id, so Windows puts a 0 in front: buttons low, buttons high, hat, then the sticks.
fn powera(r: &[u8]) -> Option<Report> {
    let o = usize::from(r.len() >= 8 && r[0] == 0);
    if r.len() < o + 7 {
        return None;
    }
    let bt = u16::from(r[o]) | (u16::from(r[o + 1]) << 8);
    let b = |mask: u16, x: u16| if bt & mask != 0 { x } else { 0 };
    let m = b(0x0001, 0x4000) | b(0x0002, 0x1000) | b(0x0004, 0x2000) | b(0x0008, 0x8000) | b(0x0010, 0x0100) | b(0x0020, 0x0200)
        | b(0x0100, 0x0020) | b(0x0200, 0x0010) | b(0x0400, 0x0040) | b(0x0800, 0x0080) | b(0x1000, 0x0400)
        | hat(r[o + 2]);
    Some(Report { buttons: m, full: false })
}

///   USB 0x01: DualSense buttons 8-10; DualShock 4 buttons 5-7
///   Bluetooth 0x01 (the pad's own mode before anything set it up): buttons 5-7 (both pads)
///   Bluetooth 0x31 (DualSense, set up by the emulator / Steam / SDL): buttons 9-11
///   Bluetooth 0x11 (DualShock 4, set up): buttons 7-9
fn playstation(r: &[u8], dualsense: bool, bluetooth: bool) -> Option<Report> {
    let n = r.len();
    let (at, full) = match r.first()? {
        0x01 if !bluetooth && n >= 11 => (if dualsense { 8 } else { 5 }, true),
        0x01 if bluetooth && n >= 10 => (5, false),
        0x31 if dualsense && n >= 12 => (9, true),
        0x11 if !dualsense && n >= 12 => (7, true),
        _ => return None,
    };
    let (b0, b1, b2) = (r[at], r[at + 1], r[at + 2]);
    let m = hat(b0 & 0x0f)
        | bit(b0, 0x10, 0x4000) | bit(b0, 0x20, 0x1000) | bit(b0, 0x40, 0x2000) | bit(b0, 0x80, 0x8000)
        | bit(b1, 0x01, 0x0100) | bit(b1, 0x02, 0x0200) | bit(b1, 0x10, 0x0020) | bit(b1, 0x20, 0x0010) | bit(b1, 0x40, 0x0040) | bit(b1, 0x80, 0x0080)
        | bit(b2, 0x01, 0x0400);
    Some(Report { buttons: m, full })
}

/// The pads of one kind as one XInput slot: the buttons any of them holds. `pads`: each pad's last report and how
/// long ago it came (ms). A full report not repeated for 300 ms means the pad is gone (low battery, out of range),
/// a simple one stands for 3 s: nothing stays held down for ever. None when no pad is there.
pub fn combined(pads: &[(Report, u64)]) -> Option<u16> {
    let live: Vec<&(Report, u64)> = pads.iter().filter(|(r, age)| !(r.full && *age > 300)).collect();
    if live.is_empty() {
        return None;
    }
    Some(live.iter().filter(|(r, age)| r.full || *age <= 3000).fold(0, |m, (r, _)| m | r.buttons))
}

#[cfg(test)]
mod tests {
    use super::*;

    const START: u16 = 0x0010;
    const BACK: u16 = 0x0020;
    const L3: u16 = 0x0040;
    const R3: u16 = 0x0080;
    const GUIDE: u16 = 0x0400;

    #[test]
    fn pads() {
        assert_eq!(kind(0x057e, 0x2009, true), Some(Kind::Nintendo));
        assert_eq!(kind(0x054c, 0x0ce6, false), Some(Kind::PlayStation { dualsense: true, bluetooth: false }));
        assert_eq!(kind(0x057e, 0x0306, false), None);
    }

    #[test]
    fn nintendo_reports() {
        // a Pro Controller in full mode: Home and +
        let mut r = vec![0x30, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        r[4] = 0x10 | 0x02;
        assert_eq!(parse(Kind::Nintendo, &r), Some(Report { buttons: GUIDE | START, full: true }));
        // L3 + R3 in its own Bluetooth mode, the hat at rest
        assert_eq!(parse(Kind::Nintendo, &[0x3f, 0, 0x04 | 0x08, 8]), Some(Report { buttons: L3 | R3, full: false }));
        assert_eq!(parse(Kind::Nintendo, &[0x81, 1, 2, 3]), None);
    }

    #[test]
    fn playstation_reports() {
        // a DualSense over USB: the PS button and Create (Back)
        let mut r = vec![0x01; 11];
        r[8] = 0x08;
        (r[9], r[10]) = (0x10, 0x01);
        assert_eq!(parse(Kind::PlayStation { dualsense: true, bluetooth: false }, &r), Some(Report { buttons: BACK | GUIDE, full: true }));
        // a DualShock 4 set up over Bluetooth: Options and L3
        let mut r = vec![0u8; 12];
        r[0] = 0x11;
        (r[7], r[8]) = (0x08, 0x20 | 0x40);
        assert_eq!(parse(Kind::PlayStation { dualsense: false, bluetooth: true }, &r), Some(Report { buttons: START | L3, full: true }));
    }

    #[test]
    fn powera_and_together() {
        assert_eq!(parse(Kind::PowerA, &[0, 0x00, 0x10, 8, 128, 128, 128, 128]), Some(Report { buttons: GUIDE, full: false }));
        let (home, l3) = (Report { buttons: GUIDE, full: true }, Report { buttons: L3, full: false });
        assert_eq!(combined(&[(home, 10), (l3, 100)]), Some(GUIDE | L3));
        assert_eq!(combined(&[(home, 400)]), None);
        assert_eq!(combined(&[(l3, 4000)]), Some(0));
        assert_eq!(combined(&[]), None);
    }
}
