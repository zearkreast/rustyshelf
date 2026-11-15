// Ported from RustyShelf's main/presence.js.
//! Discord Rich Presence: while a game runs, Discord shows "Playing <title>" with the shelf's name, and clears it
//! when the game closes. Talks to Discord's own app over its local socket; nothing is sent anywhere else. Needs a
//! Discord application id (Settings); off unless it's switched on and an id is set. If Discord isn't running it says
//! nothing and tries again later.

use std::io::{Read, Write};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::js;

const HANDSHAKE: i32 = 0;
const FRAME: i32 = 1;

/// Where Discord's socket can be: ten pipes on Windows; elsewhere ten sockets in each runtime folder, and the
/// Flatpak's and the Snap's one folder further down.
pub fn socket_paths(var: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    if cfg!(windows) {
        return (0..10).map(|n| format!(r"\\.\pipe\discord-ipc-{n}")).collect();
    }
    let mut dirs: Vec<String> = Vec::new();
    for d in [var("XDG_RUNTIME_DIR"), var("TMPDIR"), Some("/tmp".to_string())].into_iter().flatten().filter(|d| !d.is_empty()) {
        if !dirs.contains(&d) {
            dirs.push(d);
        }
    }
    let mut list: Vec<String> = dirs.iter().flat_map(|d| (0..10).map(move |n| js::join(&[d, &format!("discord-ipc-{n}")]))).collect();
    for d in &dirs {
        list.push(js::join(&[d, "app", "com.discordapp.Discord", "discord-ipc-0"]));
        list.push(js::join(&[d, "snap.discord", "discord-ipc-0"]));
    }
    list
}

/// A message as Discord's socket takes it: the kind and the length (little-endian), then JSON.
pub fn frame(op: i32, obj: &Value) -> Vec<u8> {
    let body = obj.to_string().into_bytes();
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend(op.to_le_bytes());
    out.extend((body.len() as i32).to_le_bytes());
    out.extend(body);
    out
}

trait Pipe: Read + Write + Send {}
impl<T: Read + Write + Send> Pipe for T {}

fn open(path: &str) -> Option<Box<dyn Pipe>> {
    #[cfg(unix)]
    {
        let s = std::os::unix::net::UnixStream::connect(path).ok()?;
        // a Discord that stops answering mustn't hold the next game's message back for good
        let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
        Some(Box::new(s))
    }
    #[cfg(not(unix))]
    {
        Some(Box::new(std::fs::OpenOptions::new().read(true).write(true).open(path).ok()?))
    }
}

fn read_frame(pipe: &mut dyn Pipe) -> Option<Value> {
    let mut head = [0u8; 8];
    pipe.read_exact(&mut head).ok()?;
    let len = i32::from_le_bytes([head[4], head[5], head[6], head[7]]);
    let mut body = vec![0u8; usize::try_from(len).ok().filter(|l| *l <= 1 << 20)?];
    pipe.read_exact(&mut body).ok()?;
    Some(serde_json::from_slice(&body).unwrap_or(Value::Null))
}

enum Msg {
    Configure { enabled: bool, app_id: String },
    Game { title: String, shelf: String, image: String },
    Clear,
}

struct Worker {
    paths: Vec<String>,
    on: bool,
    id: String,
    pipe: Option<Box<dyn Pipe>>,
    /// what Discord should show; None: nothing
    want: Option<Value>,
    retries: u32,
    retry: Option<Duration>,
}

impl Worker {
    fn connect(&mut self) {
        if self.pipe.is_some() || !self.on || self.id.is_empty() {
            return;
        }
        for p in &self.paths {
            let Some(mut pipe) = open(p) else { continue };
            if pipe.write_all(&frame(HANDSHAKE, &json!({ "v": 1, "client_id": self.id }))).is_err() {
                continue;
            }
            let ready = (0..8).map_while(|_| read_frame(pipe.as_mut())).any(|m| m["evt"] == "READY");
            if ready {
                self.pipe = Some(pipe);
                self.retries = 0;
                self.retry = None;
                self.flush();
                return;
            }
        }
        // Discord isn't running: a quiet retry, longer each time
        self.retries += 1;
        self.retry = Some(Duration::from_millis(u64::from(self.retries.min(12)) * 5000));
    }

    fn flush(&mut self) {
        let Some(pipe) = self.pipe.as_mut() else { return };
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis()).to_string();
        let msg = json!({ "cmd": "SET_ACTIVITY", "args": { "pid": std::process::id(), "activity": self.want }, "nonce": nonce });
        // its answer is read so the socket never fills
        if pipe.write_all(&frame(FRAME, &msg)).is_err() || read_frame(pipe.as_mut()).is_none() {
            self.pipe = None;
            self.retries += 1;
            self.retry = Some(Duration::from_millis(5000));
        }
    }

    fn take(&mut self, msg: Msg) {
        match msg {
            Msg::Configure { enabled, app_id } => {
                let id: String = app_id.chars().filter(char::is_ascii_digit).take(25).collect();
                if id != self.id {
                    self.pipe = None;
                }
                self.on = enabled && !id.is_empty();
                self.id = id;
                if !self.on {
                    self.retry = None;
                    self.pipe = None;
                    self.want = None;
                } else if self.want.is_some() {
                    self.connect();
                }
            }
            Msg::Game { title, shelf, image } if self.on => {
                let start = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
                self.want = Some(activity(&title, &shelf, &image, start));
                if self.pipe.is_some() {
                    self.flush();
                } else {
                    self.connect();
                }
            }
            Msg::Clear if self.on => {
                self.want = None;
                self.flush();
            }
            Msg::Game { .. } | Msg::Clear => {}
        }
    }
}

/// RustyShelf's icon, where Discord can fetch it: the picture when there's no cover, and the small one beside a cover.
const ICON: &str = "https://raw.githubusercontent.com/zearkreast/rustyshelf/rustyshelf/renderer/app-icon.png";

/// What Discord shows: the game's title, the shelf (or store) it's from, the time it began, and `image` (an https
/// address) its cover. Keep in sync with main/presence.js setGame.
fn activity(title: &str, shelf: &str, image: &str, start: u64) -> Value {
    let title = if title.is_empty() { "a game" } else { js::slice(title, 120) };
    let shelf = if shelf.is_empty() { "RustyShelf" } else { js::slice(shelf, 120) };
    let good = image.starts_with("https://") && !image.contains(char::is_whitespace) && image.encode_utf16().count() <= 256;
    let assets = if good {
        json!({ "large_image": image, "large_text": title, "small_image": ICON, "small_text": "RustyShelf" })
    } else {
        json!({ "large_image": ICON, "large_text": "RustyShelf" })
    };
    json!({ "type": 0, "details": title, "state": shelf, "timestamps": { "start": start }, "assets": assets, "instance": false })
}

/// What's shown on Discord, kept on a thread of its own: the socket never makes a frame wait.
pub struct Presence(Sender<Msg>);

impl Presence {
    pub fn start(var: &dyn Fn(&str) -> Option<String>) -> Presence {
        Presence::at(socket_paths(var))
    }

    /// With the sockets to try.
    pub fn at(paths: Vec<String>) -> Presence {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut w = Worker { paths, on: false, id: String::new(), pipe: None, want: None, retries: 0, retry: None };
            loop {
                let msg = match w.retry {
                    Some(wait) => rx.recv_timeout(wait),
                    None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
                };
                match msg {
                    Ok(m) => w.take(m),
                    Err(RecvTimeoutError::Timeout) => {
                        w.retry = None;
                        w.connect();
                    }
                    Err(RecvTimeoutError::Disconnected) => return,
                }
            }
        });
        Presence(tx)
    }

    /// The settings' switch and application id; called at the start and whenever they change.
    pub fn configure(&self, enabled: bool, app_id: &str) {
        let _ = self.0.send(Msg::Configure { enabled, app_id: app_id.to_string() });
    }

    /// A game started: "Playing <title>" with the shelf's name and the time it began.
    pub fn set_game(&self, title: &str, shelf: &str, image: Option<&str>) {
        let _ = self.0.send(Msg::Game { title: title.to_string(), shelf: shelf.to_string(), image: image.unwrap_or("").to_string() });
    }

    /// The game closed: nothing shown.
    pub fn clear(&self) {
        let _ = self.0.send(Msg::Clear);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_and_paths() {
        assert_eq!(frame(1, &json!({ "a": 1 })), [&[1, 0, 0, 0, 7, 0, 0, 0][..], b"{\"a\":1}"].concat());
        if cfg!(unix) {
            let var = |k: &str| (k == "XDG_RUNTIME_DIR").then(|| "/run/user/1000".to_string());
            let p = socket_paths(&var);
            assert_eq!(p.len(), 24);
            assert_eq!((p[0].as_str(), p[19].as_str()), ("/run/user/1000/discord-ipc-0", "/tmp/discord-ipc-9"));
            assert_eq!(p[20], "/run/user/1000/app/com.discordapp.Discord/discord-ipc-0");
            assert_eq!(p[23], "/tmp/snap.discord/discord-ipc-0");
        }
    }

    #[cfg(unix)]
    #[test]
    fn tells_discord_what_is_played() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("discord-ipc-0");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let p = Presence::at(vec![tmp.path().join("nothing").to_string_lossy().into_owned(), path.to_string_lossy().into_owned()]);
        // off: nothing is said
        p.set_game("Zeta Run", "GameCube", None);
        p.configure(true, "id 12345");
        p.set_game("Zeta Run", "GameCube", Some("https://images.example/zeta.png"));
        p.clear();
        let (mut s, _) = listener.accept().unwrap();
        let hello = read_frame(&mut s).unwrap();
        assert_eq!(hello, json!({ "v": 1, "client_id": "12345" }));
        s.write_all(&frame(FRAME, &json!({ "evt": "READY" }))).unwrap();
        let set = read_frame(&mut s).unwrap();
        assert_eq!(set["cmd"], "SET_ACTIVITY");
        assert_eq!(set["args"]["pid"], std::process::id());
        let a = &set["args"]["activity"];
        assert_eq!((a["details"].as_str(), a["state"].as_str(), a["instance"].as_bool()), (Some("Zeta Run"), Some("GameCube"), Some(false)));
        assert_eq!((a["assets"]["large_image"].as_str(), a["assets"]["small_image"].as_str()), (Some("https://images.example/zeta.png"), Some(ICON)));
        assert!(a["timestamps"]["start"].as_u64().unwrap() > 1_700_000_000);
        s.write_all(&frame(FRAME, &json!({ "evt": null }))).unwrap();
        let cleared = read_frame(&mut s).unwrap();
        assert_eq!(cleared["args"]["activity"], Value::Null);
        s.write_all(&frame(FRAME, &json!({ "evt": null }))).unwrap();
    }
}
