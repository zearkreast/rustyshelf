//! Fetching from the Internet: small JSON answers and files, with the limits the Electron app's downloads have.

use std::io::{Read, Write};
use std::path::Path;
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

/// A server's answer to a request with its own headers: the status, how long it asks to wait (`Retry-After`) and
/// at most `max` bytes. An error is the network's; `true` when it was a timeout.
pub fn ask(url: &str, headers: &[(&str, &str)], wait: Duration, max: usize) -> Result<(u16, Option<String>, Vec<u8>), bool> {
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

/// A page's bytes, at most `max` of them; None when the server says it isn't there (404, 403).
pub fn get_bytes(url: &str, wait: Duration, max: usize) -> Result<Option<Vec<u8>>, Failure> {
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
    let started = std::time::Instant::now();
    let res = AGENT.get(url).timeout(cap).call().map_err(|e| match e {
        ureq::Error::Status(code, _) => format!("the download server said {code}"),
        ureq::Error::Transport(t) => t.to_string(),
    })?;
    let total: u64 = res.header("content-length").and_then(|l| l.parse().ok()).unwrap_or(0);
    let mut r = res.into_reader();
    let mut w = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let (mut buf, mut got, mut last) = (vec![0u8; 64 << 10], 0u64, None);
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
        let p = (got * 100).checked_div(total).unwrap_or(0) as u32;
        if last != Some(p) {
            last = Some(p);
            on_pct(p);
        }
    }
    if total > 0 && got < total {
        return Err("the download was cut short".into());
    }
    Ok(())
}
