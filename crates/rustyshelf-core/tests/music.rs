//! The menu music's score against the Electron app's: `fixtures/music.json` holds what renderer/sound.js asks to be
//! played over four minutes of each track, for a seed and a shelf's key (scripts/rust-sound-fixtures.js). The same
//! seed here must write the same music.

use rustyshelf_core::cues::{key_of, theme};
use rustyshelf_core::music::{tonic, track, Event, Score};
use serde_json::{json, Value};

fn pitch(note: i32) -> f64 {
    440.0 * 2f64.powf(f64::from(note - 69) / 12.0)
}

fn close(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| close(p, q)),
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            (x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0)
        }
        _ => a == b,
    }
}

#[test]
fn the_music_is_the_electron_apps_note_for_note() {
    let fx: Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/music.json")).unwrap()).unwrap();
    let (start, seconds) = (fx["start"].as_f64().unwrap(), fx["seconds"].as_f64().unwrap());
    for s in fx["scores"].as_array().unwrap() {
        let (name, shelf, voice) = (s["track"].as_str().unwrap(), s["shelf"].as_str().unwrap(), s["theme"].as_str().unwrap());
        // the seed is a 32-bit number, whichever way it's written
        let seed = s["seed"].as_i64().unwrap() as i32 as u32;
        let mut score = Score::new(track(name).unwrap(), seed, start, tonic(theme(voice).root(), key_of(shelf).0));
        let mut ours = Vec::new();
        while score.next_at() < seconds {
            for e in score.step() {
                ours.push(match e {
                    Event::Pad { notes, at, glide } => json!(["pad", notes, at, glide]),
                    Event::PadLevel { level, at } => json!(["level", level, at]),
                    Event::Drone { note, at, dur, vel } => json!(["drone", pitch(note), at, dur, vel]),
                    Event::Glass { note, at, vel, pan } => json!(["glass", pitch(note), at, vel, pan]),
                    Event::Ep { note, at, dur, vel, pan } => json!(["ep", pitch(note), at, dur, vel, pan]),
                    Event::Bell { note, at, vel, pan } => json!(["bell", pitch(note), at, vel, pan]),
                });
            }
        }
        let theirs = s["events"].as_array().unwrap();
        assert!(theirs.len() > 40, "{name}: only {} events in the fixture", theirs.len());
        for (i, (t, o)) in theirs.iter().zip(&ours).enumerate() {
            assert!(close(t, o), "{name} (seed {seed}, {shelf}), event {i}: theirs {t}, ours {o}");
        }
        assert_eq!(theirs.len(), ours.len(), "{name} (seed {seed}, {shelf})");
    }
}
