// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's main/padsense.js and the controller parts of its renderer/app.js, as RustyShelf has them.
//! Controllers: what a pad is, whose letters its buttons carry on screen, and the real controllers plugged in
//! behind the pads Windows shows as XInput ones (Steam Input and DS4Windows wrap PlayStation and Switch pads
//! as those).

use std::sync::LazyLock;

use regex::Regex;

pub use crate::homemenu::Glyphs;

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap()
}

/// A maker's pads by product id; "*" is any other of its pads.
const MODELS: [(&str, &[(&str, &str)]); 5] = [
    ("054c", &[("05c4", "DualShock 4"), ("09cc", "DualShock 4"), ("0ba0", "DualShock 4 (wireless adapter)"), ("0ce6", "DualSense"), ("0df2", "DualSense Edge"), ("0e5f", "Access controller"), ("*", "PlayStation controller")]),
    (
        "057e",
        &[
            ("2009", "Switch Pro Controller"),
            ("2069", "Switch 2 Pro Controller"),
            ("2006", "Joy-Con (L)"),
            ("2007", "Joy-Con (R)"),
            ("200e", "Joy-Con pair"),
            ("2066", "Joy-Con 2 (R)"),
            ("2067", "Joy-Con 2 (L)"),
            ("2017", "SNES controller"),
            ("2073", "GameCube controller"),
            ("0337", "GameCube adapter"),
            ("0306", "Wii remote"),
            ("0330", "Wii remote"),
            ("*", "Nintendo controller"),
        ],
    ),
    ("045e", &[("*", "Xbox controller")]),
    ("28de", &[("1102", "Steam Controller"), ("1142", "Steam Controller"), ("1205", "Steam Deck"), ("*", "Steam controller")]),
    ("2dc8", &[("*", "8BitDo controller")]),
];

/// The prompts a maker's pads get. 8BitDo's go by their name, Xbox otherwise: most run in X-input mode, and in
/// Switch mode they show up as Nintendo's own pad.
fn maker_style(vendor: &str) -> Option<Glyphs> {
    match vendor {
        "054c" => Some(Glyphs::Ps),
        "057e" => Some(Glyphs::Nintendo),
        "045e" | "28de" => Some(Glyphs::Xbox),
        _ => None,
    }
}

fn model(vendor: &str, product: &str) -> Option<&'static str> {
    let models = MODELS.iter().find(|m| m.0 == vendor)?.1;
    models.iter().find(|m| m.0 == product).or(models.iter().find(|m| m.0 == "*")).map(|m| m.1)
}

/// What a pad is.
#[derive(Debug, Clone, PartialEq)]
pub struct PadInfo {
    pub style: Glyphs,
    pub name: String,
    /// four hex digits each, "" when the id doesn't say
    pub vendor: String,
    pub product: String,
    /// an XInput pad: Windows shows it without a maker, so anything can be behind it
    pub wrapped: bool,
}

/// What a pad is, from its id as a browser writes it ("DualSense Wireless Controller (STANDARD GAMEPAD Vendor:
/// 054c Product: 0ce6)"): its maker and model first, then the words in its name.
pub fn pad_info(id: &str) -> PadInfo {
    static IDS: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)vendor:?\s*([0-9a-f]{4})[^0-9a-f]+product:?\s*([0-9a-f]{4})"));
    static NINTENDO: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)switch|nintendo|joy-?con|pro controller|pokken"));
    static PS: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)dualsense|dualshock|playstation|ps[345]\b|^wireless controller"));
    let ids = IDS.captures(id).map(|m| (m[1].to_lowercase(), m[2].to_lowercase()));
    // makers of pads for every console (Hori, PDP, PowerA, 8BitDo…): what the name says
    let style = if NINTENDO.is_match(id) {
        Glyphs::Nintendo
    } else if PS.is_match(id) {
        Glyphs::Ps
    } else {
        Glyphs::Xbox
    };
    if let Some((name, (v, p))) = ids.as_ref().and_then(|(v, p)| Some((model(v, p)?, (v, p)))) {
        return PadInfo { style: maker_style(v).unwrap_or(style), name: name.into(), vendor: v.clone(), product: p.clone(), wrapped: false };
    }
    let wrapped = id.to_lowercase().contains("xinput") && ids.is_none();
    let name = if wrapped {
        "Xbox-style controller (XInput)"
    } else if style == Glyphs::Ps {
        "PlayStation controller"
    } else if style == Glyphs::Nintendo {
        "Nintendo controller"
    } else if id.to_lowercase().contains("xbox") {
        "Xbox controller"
    } else {
        "Controller"
    };
    let (vendor, product) = ids.unwrap_or_default();
    PadInfo { style, name: name.into(), vendor, product, wrapped }
}

/// A real controller plugged in (USB or Bluetooth).
#[derive(Debug, Clone, PartialEq)]
pub struct Real {
    pub vendor: String,
    pub product: String,
    pub style: Glyphs,
}

/// What's behind an XInput pad, when the controllers plugged in say: with a real Xbox pad among them, nothing;
/// otherwise the one kind there is (a DualSense behind Steam Input or DS4Windows).
pub fn see_real(real: &[Real]) -> Option<&Real> {
    let first = real.first()?;
    (real.iter().all(|r| r.style != Glyphs::Xbox && r.style == first.style)).then_some(first)
}

/// The prompts a pad gets: its own, or for an XInput pad those of the real controller behind it.
pub fn style_of(id: &str, real: &[Real]) -> Glyphs {
    let info = pad_info(id);
    if !info.wrapped {
        return info.style;
    }
    see_real(real).map_or(Glyphs::Xbox, |r| r.style)
}

/// A pad's name for the player.
pub fn pad_name(id: &str, real: &[Real]) -> String {
    let info = pad_info(id);
    match see_real(real).filter(|_| info.wrapped) {
        Some(r) => format!("{} (through Steam / DS4Windows)", model(&r.vendor, &r.product).unwrap_or("Controller")),
        None => info.name,
    }
}

/// The real controllers in what Windows lists of its devices (lines of `instance id|class|name`). Left out:
/// other Microsoft devices (mice, keyboards), Sony and Nintendo devices that aren't pads (a headset has no
/// game-controller part), Steam's and ViGEm's virtual pads, Wii remotes.
pub fn parse(out: &str) -> Vec<Real> {
    static USB: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)VID_([0-9A-F]{4})&PID_([0-9A-F]{4})"));
    static BLUETOOTH: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)VID&0002([0-9A-F]{4})_PID&([0-9A-F]{4})"));
    static PAD: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)game controller|gamepad|joystick|controller"));
    static XBOX_CLASS: LazyLock<Regex> = LazyLock::new(|| re(r"(?i)^(XnaComposite|XboxComposite)$"));
    static XBOX_PAD: LazyLock<Regex> = LazyLock::new(|| re(r"^0[2b]"));
    let lines: Vec<&str> = out.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l)).filter(|l| !l.is_empty()).collect();
    let vigem = lines.iter().any(|l| l.to_lowercase().contains("virtual gamepad emulation bus"));
    // by vendor and product, as first seen: whether a game-controller part was seen, and an Xbox device class
    let mut seen: Vec<((String, String), bool, bool)> = Vec::new();
    for l in lines {
        let mut parts = l.split('|');
        let (id, class, name) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""), parts.next().unwrap_or(""));
        let Some(m) = USB.captures(id).or_else(|| BLUETOOTH.captures(id)) else { continue };
        let key = (m[1].to_lowercase(), m[2].to_lowercase());
        let at = seen.iter().position(|s| s.0 == key).unwrap_or_else(|| {
            seen.push((key, false, false));
            seen.len() - 1
        });
        seen[at].1 |= PAD.is_match(name);
        seen[at].2 |= XBOX_CLASS.is_match(class);
    }
    seen.into_iter()
        .filter(|((v, p), pad, xbox)| match (v.as_str(), p.as_str()) {
            // Wii remotes; Steam's own virtual pad
            ("057e", "0306" | "0330") | ("28de", "11ff") => false,
            // an Xbox pad, not a mouse; not DS4Windows' virtual 360 pad
            ("045e", p) => (*xbox || XBOX_PAD.is_match(p)) && !(vigem && p == "028e"),
            _ => *pad,
        })
        .map(|((vendor, product), _, _)| Real { style: maker_style(&vendor).unwrap_or(Glyphs::Xbox), vendor, product })
        .collect()
}

/// The real controllers plugged in now; none anywhere but on Windows, where it's asked of PowerShell (a second
/// or so).
pub fn list() -> Vec<Real> {
    const ASK: &str = r#"(Get-PnpDevice -PresentOnly -ErrorAction SilentlyContinue | Where-Object { $_.InstanceId -match "VID_(054C|057E|045E|28DE|2DC8)|VID&0002(054C|057E|045E|28DE|2DC8)" -or $_.FriendlyName -match "Virtual Gamepad Emulation Bus" } | ForEach-Object { $_.InstanceId + "|" + $_.Class + "|" + $_.FriendlyName }) -join "`n""#;
    if !cfg!(windows) {
        return Vec::new();
    }
    let mut ask = std::process::Command::new("powershell.exe");
    ask.args(["-NoProfile", "-NonInteractive", "-Command", ASK]);
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut ask, 0x0800_0000);
    ask.output().ok().filter(|o| o.status.success()).map_or(Vec::new(), |o| parse(&String::from_utf8_lossy(&o.stdout)))
}

/// A button as the pad in hand prints it, for the hint bar. The keys are a PlayStation pad's: `x o sq tri L1 R1
/// L2 R2 L3 R3 ☰ ⧉`; with `swap`, the right face button confirms, so `x` (the one that confirms) is that one.
pub fn cap(key: &str, style: Glyphs, swap: bool) -> &str {
    let key = match key {
        "x" if swap => "o",
        "o" if swap => "x",
        k => k,
    };
    match (style, key) {
        (Glyphs::Xbox, "x") | (Glyphs::Nintendo, "o") => "A",
        (Glyphs::Xbox, "o") | (Glyphs::Nintendo, "x") => "B",
        (Glyphs::Xbox, "sq") | (Glyphs::Nintendo, "tri") => "X",
        (Glyphs::Xbox, "tri") | (Glyphs::Nintendo, "sq") => "Y",
        (Glyphs::Ps, "x") => "✕",
        (Glyphs::Ps, "o") => "○",
        (Glyphs::Ps, "sq") => "□",
        (Glyphs::Ps, "tri") => "△",
        (Glyphs::Xbox, "L1") => "LB",
        (Glyphs::Xbox, "R1") => "RB",
        (Glyphs::Xbox, "L2") => "LT",
        (Glyphs::Xbox, "R2") => "RT",
        (Glyphs::Nintendo, "L1") => "L",
        (Glyphs::Nintendo, "R1") => "R",
        (Glyphs::Nintendo, "L2") => "ZL",
        (Glyphs::Nintendo, "R2") => "ZR",
        (Glyphs::Xbox | Glyphs::Nintendo, "L3" | "L") => "LS",
        (Glyphs::Xbox | Glyphs::Nintendo, "R3" | "R") => "RS",
        (Glyphs::Xbox, "☰") => "Menu",
        (Glyphs::Xbox, "⧉") => "View",
        (Glyphs::Nintendo, "☰") => "+",
        (Glyphs::Nintendo, "⧉") => "−",
        (Glyphs::Ps, "☰") => "Options",
        (Glyphs::Ps, "⧉") => "Share",
        (_, k) => k,
    }
}

/// The button that confirms (`ok`) or goes back, as a word for running text: "A", "Cross".
pub fn button_word(ok: bool, style: Glyphs, swap: bool) -> &'static str {
    // the button on the right, else the bottom one
    let right = ok == swap;
    match (style, right) {
        (Glyphs::Nintendo, true) | (Glyphs::Xbox, false) => "A",
        (Glyphs::Nintendo, false) | (Glyphs::Xbox, true) => "B",
        (Glyphs::Ps, true) => "Circle",
        (Glyphs::Ps, false) => "Cross",
    }
}

/// A button's printed name for running text: the keys `L1 R1 L3 menu guide`.
pub fn pad_word(key: &str, style: Glyphs) -> &str {
    match (key, style) {
        ("L1", Glyphs::Xbox) => "LB",
        ("L1", Glyphs::Nintendo) => "L",
        ("R1", Glyphs::Xbox) => "RB",
        ("R1", Glyphs::Nintendo) => "R",
        ("L3", Glyphs::Xbox | Glyphs::Nintendo) => "LS",
        ("menu", Glyphs::Xbox) => "Menu",
        ("menu", Glyphs::Nintendo) => "+",
        ("menu", Glyphs::Ps) => "Options",
        ("guide", Glyphs::Xbox) => "the Xbox button",
        ("guide", Glyphs::Nintendo) => "Home",
        ("guide", Glyphs::Ps) => "the PS button",
        (k, _) => k,
    }
}
