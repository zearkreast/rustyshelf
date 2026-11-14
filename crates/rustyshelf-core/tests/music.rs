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

#[test]
fn the_music_sounds_as_the_electron_apps_does() {
    let fx: Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/music.json")).unwrap()).unwrap();
    let mut wrong = Vec::new();
    for s in fx["sounds"].as_array().unwrap() {
        let (name, shelf) = (s["track"].as_str().unwrap(), s["shelf"].as_str().unwrap());
        let seed = s["seed"].as_i64().unwrap() as i32 as u32;
        // the fixtures were rendered in the Aero voice, at the interface's own volume and the music's usual level
        let got = rustyshelf_core::music::render(track(name).unwrap(), seed, tonic(theme("aero").root(), key_of(shelf).0), 30.0, 0.4, 0.7, 44_100);
        let mut line = format!("{name}:");
        let mut bad = false;
        for (ch, ours) in [&got.left, &got.right].into_iter().enumerate() {
            let theirs: Vec<f64> = s["rms"][ch].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
            let ours: Vec<f64> = ours.chunks(11_025).map(|w| (w.iter().map(|x| f64::from(x * x)).sum::<f64>() / 11_025.0).sqrt()).collect();
            // the room and the air differ take by take, and a held chord's level in the room with them (two of the
            // Electron app's own renders are up to a quarter apart in some second): the level is compared a second
            // at a time
            let seconds = |v: &[f64]| -> Vec<f64> { v.chunks(4).map(|w| (w.iter().map(|x| x * x).sum::<f64>() / 4.0).sqrt()).collect() };
            let (theirs, ours) = (seconds(&theirs), seconds(&ours));
            let top = theirs.iter().copied().fold(1e-12, f64::max);
            let worst = theirs.iter().zip(&ours).map(|(t, o)| (t - o).abs() / (t + 0.1 * top)).fold(0.0, f64::max);
            let energy = (ours.iter().map(|x| x * x).sum::<f64>() / theirs.iter().map(|x| x * x).sum::<f64>()).sqrt();
            line += &format!(" off by {:.0}%, energy x{energy:.2};", worst * 100.0);
            bad |= worst > 0.4 || !(0.88..=1.15).contains(&energy);
            if std::env::var("SYNTH_SHOW").is_ok() {
                let show = |v: &[f64]| v.iter().take(48).map(|x| format!("{:.4}", x)).collect::<Vec<_>>().join(" ");
                println!("  theirs {}\n  ours   {}", show(&theirs), show(&ours));
            }
        }
        println!("{}{line}", if bad { "WRONG  " } else { "ok     " });
        if bad {
            wrong.push(name.to_string());
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// Writes half a minute of each track as a WAV file into the folder `CUES_OUT` names, for listening to it.
#[test]
#[ignore]
fn listen() {
    let dir = std::path::PathBuf::from(std::env::var("CUES_OUT").unwrap());
    for t in &rustyshelf_core::music::TRACKS {
        let out = rustyshelf_core::music::render(t, 5, tonic(theme("aero").root(), 0), 30.0, 0.4, 0.7, 44_100);
        std::fs::write(dir.join(format!("music-{}.wav", t.id)), out.wav()).unwrap();
    }
}
