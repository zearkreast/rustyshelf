// Ported from RustyShelf's main/windowmode.js.
//! A PC game's own window mode (Details → Window), for games the shelf starts itself (Quiver, GOG, HoYoPlay, the PC
//! games folder, programs added by hand). Kept as the settings' `games[<game id>].window`:
//!   - `""`: the game does it its own way (nothing is changed)
//!   - `"borderless"`: for a game with no fullscreen of its own. Once its window is up, its frame and title bar are
//!     taken off and it is stretched over the screen it is on, and kept that way while it runs
//!   - `"windowed"`: the game stays a window and the taskbar stays visible
//!
//! Borderless is Windows only. A small C# program does the work (compiled by PowerShell's Add-Type): it follows the
//! game's process and the processes it starts (a launcher that starts the real game), finds their visible top-level
//! windows, and takes the frame off the big ones. It can't change the window of a game that runs as administrator,
//! so that game keeps its own mode.

use std::process::{Child, Command, Stdio};

use base64::Engine;

use crate::config::Config;

pub const MODES: [&str; 3] = ["", "borderless", "windowed"];
/// The PC games the shelf starts itself, by where they come from.
pub const SOURCES: [&str; 5] = ["custom", "quiver", "gog", "hoyoplay", "folder"];

/// A game's window mode; "" for one that has none or one that isn't a mode.
pub fn mode_of(cfg: &Config, id: &str) -> &'static str {
    let m = cfg.game(id).and_then(|g| g.get("window")).and_then(|w| w.as_str()).unwrap_or("");
    MODES.into_iter().find(|x| *x == m).unwrap_or("")
}

// has no single quote, so PowerShell takes it as one string (keep in sync with main/windowmode.js CSHARP)
const CSHARP: &str = r##"using System; using System.Collections.Generic; using System.Diagnostics; using System.Runtime.InteropServices; using System.Threading;
public static class BL {
 delegate bool EnumProc(IntPtr h, IntPtr l);
 [StructLayout(LayoutKind.Sequential)] struct RECT { public int L; public int T; public int R; public int B; }
 [StructLayout(LayoutKind.Sequential)] struct MONITORINFO { public int cbSize; public RECT rcMonitor; public RECT rcWork; public uint dwFlags; }
 [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Ansi)] struct PROCESSENTRY32 { public uint dwSize; public uint cntUsage; public uint th32ProcessID; public IntPtr th32DefaultHeapID; public uint th32ModuleID; public uint cntThreads; public uint th32ParentProcessID; public int pcPriClassBase; public uint dwFlags; [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 260)] public string szExeFile; }
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc cb, IntPtr l);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
 [DllImport("user32.dll")] static extern IntPtr GetWindow(IntPtr h, uint cmd);
 [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr h, int i);
 [DllImport("user32.dll")] static extern int SetWindowLong(IntPtr h, int i, int v);
 [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] static extern bool SetWindowPos(IntPtr h, IntPtr after, int x, int y, int cx, int cy, uint flags);
 [DllImport("user32.dll")] static extern IntPtr MonitorFromWindow(IntPtr h, uint flags);
 [DllImport("user32.dll")] static extern bool GetMonitorInfo(IntPtr m, ref MONITORINFO mi);
 [DllImport("kernel32.dll")] static extern IntPtr CreateToolhelp32Snapshot(uint flags, uint pid);
 [DllImport("kernel32.dll")] static extern bool Process32First(IntPtr s, ref PROCESSENTRY32 e);
 [DllImport("kernel32.dll")] static extern bool Process32Next(IntPtr s, ref PROCESSENTRY32 e);
 [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);
 static Dictionary<int, int> Parents() {
  Dictionary<int, int> m = new Dictionary<int, int>(); IntPtr s = CreateToolhelp32Snapshot(2, 0);
  if (s == IntPtr.Zero || s == new IntPtr(-1)) return m;
  PROCESSENTRY32 e = new PROCESSENTRY32(); e.dwSize = (uint)Marshal.SizeOf(typeof(PROCESSENTRY32));
  if (Process32First(s, ref e)) { do { m[(int)e.th32ProcessID] = (int)e.th32ParentProcessID; } while (Process32Next(s, ref e)); }
  CloseHandle(s); return m; }
 static bool UnderDir(int pid, string dir, Dictionary<int, bool> cache) {
  bool v; if (cache.TryGetValue(pid, out v)) return v; v = false;
  try { string f = Process.GetProcessById(pid).MainModule.FileName; v = f != null && dir.Length > 0 && f.StartsWith(dir, StringComparison.OrdinalIgnoreCase); } catch (Exception) { v = false; }
  cache[pid] = v; return v; }
 static bool Fix(IntPtr h) {
  int st = GetWindowLong(h, -16); int ex = GetWindowLong(h, -20);
  int nst = st & ~(0xC00000 | 0x40000 | 0x20000 | 0x10000 | 0x80000); int nex = ex & ~(0x1 | 0x100 | 0x200 | 0x20000);
  MONITORINFO mi = new MONITORINFO(); mi.cbSize = Marshal.SizeOf(typeof(MONITORINFO));
  if (!GetMonitorInfo(MonitorFromWindow(h, 2), ref mi)) return false;
  RECT r; if (!GetWindowRect(h, out r)) return false;
  bool same = r.L == mi.rcMonitor.L && r.T == mi.rcMonitor.T && r.R == mi.rcMonitor.R && r.B == mi.rcMonitor.B;
  if (nst == st && nex == ex && same) return false;
  SetWindowLong(h, -16, nst); SetWindowLong(h, -20, nex);
  SetWindowPos(h, IntPtr.Zero, mi.rcMonitor.L, mi.rcMonitor.T, mi.rcMonitor.R - mi.rcMonitor.L, mi.rcMonitor.B - mi.rcMonitor.T, 0x20 | 0x40 | 0x200);
  return true; }
 public static void Run(int root, string dir, int maxSeconds) {
  DateTime end = DateTime.UtcNow.AddSeconds(maxSeconds); HashSet<int> fam = new HashSet<int>(); fam.Add(root);
  Dictionary<int, bool> cache = new Dictionary<int, bool>(); int gone = 0; bool seen = false;
  while (DateTime.UtcNow < end) {
   Dictionary<int, int> parents = Parents(); bool grew = true;
   while (grew) { grew = false; foreach (KeyValuePair<int, int> kv in parents) { if (!fam.Contains(kv.Key) && fam.Contains(kv.Value)) { fam.Add(kv.Key); grew = true; } } }
   bool alive = false; foreach (int p in fam) { if (parents.ContainsKey(p)) alive = true; }
   List<IntPtr> wins = new List<IntPtr>();
   EnumWindows(delegate (IntPtr h, IntPtr l) {
    if (!IsWindowVisible(h) || IsIconic(h) || GetWindow(h, 4) != IntPtr.Zero) return true;
    uint pid; GetWindowThreadProcessId(h, out pid); int ip = (int)pid;
    if (!fam.Contains(ip) && !UnderDir(ip, dir, cache)) return true;
    RECT r; if (!GetWindowRect(h, out r) || r.R - r.L < 320 || r.B - r.T < 240) return true;
    wins.Add(h); alive = true; return true; }, IntPtr.Zero);
   foreach (IntPtr w in wins) { if (Fix(w)) seen = true; }
   if (!alive) { gone++; if (gone > 6 && (seen || gone > 300)) return; } else gone = 0;
   Thread.Sleep(1000); } }
}"##;

/// The PowerShell command that compiles that program and runs it for a game: `pid` the game's process, `dir` its
/// folder, followed for `seconds` at most.
pub fn ps_script(pid: u32, dir: &str, seconds: u32) -> String {
    let q = |s: &str| format!("'{}'", s.replace('\'', "''"));
    format!("$ErrorActionPreference = 'Stop'; Add-Type -TypeDefinition {}; [BL]::Run({pid}, {}, {})", q(CSHARP), q(dir), if seconds == 0 { 60 } else { seconds })
}

/// Starts following a game's window (borderless). Windows only; None when PowerShell can't be started (the game
/// just keeps its own mode). The helper leaves by itself once the game is gone; `Child::kill` stops it sooner.
pub fn borderless(pid: u32, dir: &str) -> Option<Child> {
    if !cfg!(windows) || pid == 0 {
        return None;
    }
    let utf16: Vec<u8> = ps_script(pid, dir, 8 * 3600).encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut cmd = Command::new("powershell.exe");
    cmd.args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-ExecutionPolicy", "Bypass", "-EncodedCommand", &base64::engine::general_purpose::STANDARD.encode(utf16)]);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut cmd, 0x0800_0000); // CREATE_NO_WINDOW
    cmd.spawn().ok()
}
