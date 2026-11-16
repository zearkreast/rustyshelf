//! Frame times, for finding what makes the app stutter: with `RUSTYSHELF_FRAMES=1` every slow frame is logged with
//! what happened during it (`note`, `span`), and every 5 seconds the frame rate, the 99th-percentile frame and the
//! slowest. Off, they cost an atomic load.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Instant;

use bevy::prelude::*;

static ON: AtomicBool = AtomicBool::new(false);
static NOTES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// a frame slower than this many 60 Hz frames is logged
const SLOW: f32 = 1.5;

pub fn enabled() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Something that happened this frame, logged with it if the frame turns out slow.
pub fn note(what: impl Into<String>) {
    if enabled() {
        if let Ok(mut n) = NOTES.lock() {
            n.push(what.into());
        }
    }
}

/// Notes how long the rest of the scope took, when it ends (`span`).
pub struct Span(&'static str, Instant);

impl Drop for Span {
    fn drop(&mut self) {
        let ms = self.1.elapsed().as_secs_f32() * 1000.0;
        if ms >= 0.5 {
            note(format!("{} {ms:.1} ms", self.0));
        }
    }
}

/// A `Span` when frames are being logged.
pub fn span(what: &'static str) -> Option<Span> {
    enabled().then(|| Span(what, Instant::now()))
}

#[derive(Default)]
pub struct Window {
    times: Vec<f32>,
    since: f32,
}

pub fn plugin(app: &mut App) {
    if std::env::var_os("RUSTYSHELF_FRAMES").is_some_and(|v| !v.is_empty() && v != "0") {
        ON.store(true, Ordering::Relaxed);
        app.add_systems(Last, report)
            .add_systems(PostUpdate, (ui_start.before(bevy::ui::UiSystems::Prepare), ui_end.after(bevy::ui::UiSystems::Stack)))
            .add_systems(First, update_start)
            .add_systems(PostUpdate, update_end.before(bevy::ui::UiSystems::Prepare));
    }
}

/// When the frame's Update work and the UI systems (text shaping and layout) started.
static UPDATE_AT: Mutex<Option<Instant>> = Mutex::new(None);
static UI_AT: Mutex<Option<Instant>> = Mutex::new(None);

fn mark(at: &Mutex<Option<Instant>>) {
    if let Ok(mut a) = at.lock() {
        *a = Some(Instant::now());
    }
}

/// Notes the time since `at`'s mark when it's at least `min` milliseconds.
fn since(at: &Mutex<Option<Instant>>, what: &str, min: f32) {
    if let Some(t) = at.lock().ok().and_then(|mut a| a.take()) {
        let ms = t.elapsed().as_secs_f32() * 1000.0;
        if ms >= min {
            note(format!("{what} {ms:.1} ms"));
        }
    }
}

fn update_start() {
    mark(&UPDATE_AT);
}

fn update_end() {
    since(&UPDATE_AT, "main work", 4.0);
}

fn ui_start() {
    mark(&UI_AT);
}

fn ui_end() {
    since(&UI_AT, "ui layout", 2.0);
}

fn report(time: Res<Time<Real>>, mut window: Local<Window>) {
    let dt = time.delta_secs() * 1000.0;
    let notes = NOTES.lock().map(|mut n| std::mem::take(&mut *n)).unwrap_or_default();
    // the first frames load everything: not stutter
    if time.elapsed_secs() < 3.0 {
        return;
    }
    if dt > SLOW * 1000.0 / 60.0 {
        info!("slow frame {dt:.1} ms{}{}", if notes.is_empty() { "" } else { ": " }, notes.join(", "));
    }
    window.times.push(dt);
    window.since += dt / 1000.0;
    if window.since >= 5.0 {
        let mut t = std::mem::take(&mut window.times);
        t.sort_by(f32::total_cmp);
        let n = t.len().max(1);
        let avg = t.iter().sum::<f32>() / n as f32;
        info!("frames: {:.0} fps, 99% under {:.1} ms, slowest {:.1} ms ({n} frames)", 1000.0 / avg.max(0.001), t[(n * 99 / 100).min(n - 1)], t[n - 1]);
        window.since = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_keeps_nothing() {
        assert!(span("x").is_none());
        note("ignored");
        assert!(NOTES.lock().unwrap().is_empty());
    }
}
