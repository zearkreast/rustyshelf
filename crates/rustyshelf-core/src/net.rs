//! Fetching from the Internet: small JSON answers and files, with the limits the Electron app's downloads have.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
use std::time::Duration;

use serde_json::Value;

static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
    let mut b = ureq::AgentBuilder::new().user_agent("RustyShelf").timeout_connect(Duration::from_secs(20)).redirects(5);
    // a proxy the system names is used, as the Electron app's requests go through it
    if let Some(proxy) = ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy"].iter().find_map(|k| std::env::var(k).ok()).filter(|p| !p.is_empty()).and_then(|p| ureq::Proxy::new(p).ok()) {
        b = b.proxy(proxy);
    }
    b.build()
});

static ONLINE: AtomicBool = AtomicBool::new(true);

/// What a request says while Settings → System → Internet access is off.
pub const OFF_SAYS: &str = "Internet access is off (Settings → System)";

/// Settings → System → Internet access: off, every request here fails before it leaves the computer.
pub fn set_online(on: bool) {
    ONLINE.store(on, Ordering::Relaxed);
}

pub fn online() -> bool {
    ONLINE.load(Ordering::Relaxed)
}

/// Whether an address is on this computer or the home network (localhost, a private or link-local address, a
/// one-word or `.local` / `.lan` / `.home` name): a RomM server there is still reached with Internet access off.
pub fn is_local(url: &str) -> bool {
    let rest = url.split_once("://").map_or(url, |(_, r)| r);
    let host_port = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = host_port.rsplit_once('@').map_or(host_port, |(_, h)| h);
    let host = if let Some(v6) = host_port.strip_prefix('[') { v6.split(']').next().unwrap_or("") } else { host_port.rsplit_once(':').map_or(host_port, |(h, _)| h) };
    let host = host.to_ascii_lowercase();
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
            std::net::IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80,
        };
    }
    !host.is_empty() && (host == "localhost" || !host.contains('.') || [".local", ".lan", ".home", ".localhost", ".internal"].iter().any(|t| host.ends_with(t)))
}

fn off() -> Result<(), Failure> {
    if online() { Ok(()) } else { Err(OFF_SAYS.into()) }
}

/// `RustyShelf/<version>`, the User-Agent the app's own requests name.
pub fn agent() -> &'static str {
    static NAME: LazyLock<String> = LazyLock::new(|| format!("RustyShelf/{}", crate::VERSION));
    &NAME
}

/// What went wrong, in the words the shelf shows.
pub type Failure = String;

fn failure(e: ureq::Error) -> Failure {
    match e {
        ureq::Error::Status(code, _) => format!("HTTP {code}"),
        ureq::Error::Transport(t) => t.to_string(),
    }
}

/// A page's status and bytes (at most `max` of them), whatever the status. An error is the network's, not the
/// server's: offline, a timeout, a dropped connection.
pub fn get(url: &str, wait: Duration, max: usize, user_agent: &str) -> Result<(u16, Vec<u8>), Failure> {
    off()?;
    let res = match AGENT.get(url).set("User-Agent", user_agent).timeout(wait).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(code, _)) => return Ok((code, Vec::new())),
        Err(e) => return Err(failure(e)),
    };
    let status = res.status();
    let mut b = Vec::new();
    res.into_reader().take(max as u64).read_to_end(&mut b).map_err(|e| e.to_string())?;
    Ok((status, b))
}

/// A download kept only when it's a picture ([`crate::steamart::is_image`]); None when it isn't, is missing or
/// couldn't be had.
pub fn get_picture(url: &str, wait: Duration, max: usize) -> Option<Vec<u8>> {
    get_bytes(url, wait, max).ok().flatten().filter(|b| crate::steamart::is_image(b))
}

/// A server's answer to a request with its own headers: the status, how long it asks to wait (`Retry-After`) and
/// at most `max` bytes. An error is the network's; `true` when it was a timeout.
pub fn ask(url: &str, headers: &[(&str, &str)], wait: Duration, max: usize) -> Result<(u16, Option<String>, Vec<u8>), bool> {
    off().map_err(|_| false)?;
    let mut req = AGENT.get(url).timeout(wait);
    for (name, value) in headers {
        req = req.set(name, value);
    }
    let res = match req.call() {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(t)) => return Err(t.to_string().to_lowercase().contains("timed out")),
    };
    let (status, after) = (res.status(), res.header("retry-after").map(str::to_string));
    let mut b = Vec::new();
    res.into_reader().take(max as u64).read_to_end(&mut b).map_err(|e| e.kind() == std::io::ErrorKind::TimedOut)?;
    Ok((status, after, b))
}

/// A POST with its own headers and body: the status and at most `max` bytes of the answer, whatever the status. An
/// error is the network's.
pub fn post(url: &str, headers: &[(&str, &str)], body: &str, wait: Duration, max: usize) -> Result<(u16, Vec<u8>), Failure> {
    off()?;
    let mut req = AGENT.post(url).timeout(wait);
    for (name, value) in headers {
        req = req.set(name, value);
    }
    let res = match req.send_string(body) {
        Ok(r) | Err(ureq::Error::Status(_, r)) => r,
        Err(e) => return Err(failure(e)),
    };
    let status = res.status();
    let mut b = Vec::new();
    res.into_reader().take(max as u64).read_to_end(&mut b).map_err(|e| e.to_string())?;
    Ok((status, b))
}

/// A page's bytes, at most `max` of them; None when the server says it isn't there (404, 403).
pub fn get_bytes(url: &str, wait: Duration, max: usize) -> Result<Option<Vec<u8>>, Failure> {
    off()?;
    let res = match AGENT.get(url).timeout(wait).call() {
        Ok(r) => r,
        Err(ureq::Error::Status(404 | 403, _)) => return Ok(None),
        Err(e) => return Err(failure(e)),
    };
    let mut b = Vec::new();
    res.into_reader().take(max as u64 + 1).read_to_end(&mut b).map_err(|e| e.to_string())?;
    if b.len() > max {
        return Err("the answer was too big".into());
    }
    Ok(Some(b))
}

/// A JSON answer of at most 2 MB.
pub fn get_json(url: &str, wait: Duration) -> Result<Value, Failure> {
    let b = get_bytes(url, wait, 2_000_000)?.ok_or("HTTP 404")?;
    serde_json::from_slice(&b).map_err(|_| "bad reply".to_string())
}

/// Downloads a file to `dest`, saying how far it is in percent (when the server says how big it is). Gives up after
/// `cap`: a server trickling forever would keep a launch waiting.
pub fn download(url: &str, dest: &Path, cap: Duration, on_pct: &mut dyn FnMut(u32)) -> Result<(), Failure> {
    let mut last = None;
    download_sized(url, dest, cap, &mut |got, total| {
        let p = (got * 100).checked_div(total).unwrap_or(0) as u32;
        if last != Some(p) {
            last = Some(p);
            on_pct(p);
        }
    })
}

/// [`download`], saying the bytes got and the size (0 when the server doesn't say) at most every 256 KB.
pub fn download_sized(url: &str, dest: &Path, cap: Duration, on: &mut dyn FnMut(u64, u64)) -> Result<(), Failure> {
    off()?;
    let started = std::time::Instant::now();
    let res = AGENT.get(url).timeout(cap).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("the download server said {code}"),
        ureq::Error::Transport(t) => t.to_string(),
    })?;
    let total: u64 = res.header("content-length").and_then(|l| l.parse().ok()).unwrap_or(0);
    let mut r = res.into_reader();
    let mut w = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let (mut buf, mut got, mut said) = (vec![0u8; 64 << 10], 0u64, None);
    on(0, total);
    loop {
        if started.elapsed() > cap {
            return Err("the download took too long".into());
        }
        let n = r.read(&mut buf).map_err(|e| if e.kind() == std::io::ErrorKind::TimedOut { "the download timed out".to_string() } else { e.to_string() })?;
        if n == 0 {
            break;
        }
        w.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        got += n as u64;
        if said.is_none_or(|s| got - s >= 256 << 10) {
            said = Some(got);
            on(got, total);
        }
    }
    if total > 0 && got < total {
        return Err("the download was cut short".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn home_network_addresses_are_told_from_the_internet() {
        for local in ["http://192.168.1.20:8080/api", "https://10.0.0.5", "http://romm.lan", "http://nas:3000", "http://localhost", "https://user:pw@172.16.4.1/x", "http://[fe80::1]:80", "http://romm.local/"] {
            assert!(super::is_local(local), "{local}");
        }
        for far in ["https://romm.example.com", "https://8.8.8.8", "https://api.github.com/repos", "http://[2001:db8::1]/"] {
            assert!(!super::is_local(far), "{far}");
        }
    }
}
