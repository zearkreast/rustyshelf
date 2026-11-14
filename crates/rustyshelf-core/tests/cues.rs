//! The interface's cues against the Electron app's: `fixtures/cues.json` holds the voices each cue asks
//! renderer/sound.js for (scripts/rust-sound-fixtures.js), in every voice of the interface and several shelves'
//! keys, moving along the shelf at different speeds. The same steps here must ask for the same voices.

use rustyshelf_core::cues::Sound;
use rustyshelf_core::synth::{Filter, Voice};
use serde_json::{json, Map, Value};

/// A voice as the fixture writes one: its kind and the settings it gives.
fn written(v: &Voice) -> (&'static str, Map<String, Value>) {
    let mut m = Map::new();
    let mut put = |k: &str, v: Option<f32>| {
        if let Some(v) = v {
            m.insert(k.into(), json!(v));
        }
    };
    let kind = match v {
        Voice::Hit(o) => {
            for (k, v) in [("freq", o.freq), ("dur", o.dur), ("vol", o.vol), ("at", o.at), ("pan", o.pan), ("verb", o.verb), ("bright", o.bright), ("thump", o.thump)] {
                put(k, v);
            }
            for (k, v) in [("attack", o.attack), ("noise", o.noise), ("glide", o.glide), ("glideTime", o.glide_time), ("sub", o.under), ("breath", o.breath)] {
                put(k, v);
            }
            if let Some(p) = o.partials {
                m.insert("partials".into(), json!(p.iter().map(|p| [p.0, p.1, p.2]).collect::<Vec<_>>()));
            }
            "hit"
        }
        Voice::Air(o) => {
            for (k, v) in [("at", o.at), ("dur", o.dur), ("vol", o.vol), ("freq", o.freq), ("sweepTo", o.sweep_to), ("q", o.q), ("verb", o.verb), ("pan", o.pan), ("attack", o.attack), ("hold", o.hold)] {
                put(k, v);
            }
            let filter = match o.filter {
                Filter::Lowpass => "lowpass",
                Filter::Highpass => "highpass",
                Filter::Bandpass => "bandpass",
            };
            m.insert("filter".into(), json!(filter));
            m.insert("soft".into(), json!(o.soft));
            "air"
        }
        Voice::Fm(o) => {
            for (k, v) in [("freq", o.freq), ("at", o.at), ("dur", o.dur), ("vol", o.vol), ("ratio", o.ratio), ("index", o.index), ("floor", o.floor), ("bright", o.bright), ("pan", o.pan), ("verb", o.verb), ("detune", o.detune), ("attack", o.attack)] {
                put(k, v);
            }
            "fm"
        }
        Voice::Pad(o) => {
            for (k, v) in [("at", o.at), ("dur", o.dur), ("vol", o.vol), ("from", o.from), ("to", o.to), ("open", o.open), ("attack", o.attack), ("hold", o.hold), ("q", o.q), ("verb", o.verb)] {
                put(k, v);
            }
            m.insert("freqs".into(), json!(o.freqs));
            "pad"
        }
        Voice::Swell(o) => {
            for (k, v) in [("at", o.at), ("dur", o.dur), ("vol", o.vol), ("attack", o.attack), ("verb", o.verb)] {
                put(k, v);
            }
            m.insert("freqs".into(), json!(o.freqs));
            "swell"
        }
    };
    (kind, m)
}

fn close(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| close(p, q)),
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            (x - y).abs() <= 2e-4 * x.abs().max(y.abs()).max(0.05)
        }
        _ => a == b,
    }
}

/// What differs between the voice a cue asked the Electron app for and ours, if anything.
fn differs(theirs: &Value, ours: &Voice) -> Option<String> {
    let (kind, got) = written(ours);
    if theirs[0] != kind {
        return Some(format!("a {} where theirs is a {}", kind, theirs[0]));
    }
    let want = theirs[1].as_object().unwrap();
    // not sound: the music stepping back for a cue, the lights that follow a note, how a swell's chord was named
    let skip = ["duck", "fx", "notes", "octave"];
    let unset = |k: &str| match k {
        // these are nothing when they aren't given
        "at" | "pan" => json!(0.0),
        "filter" => json!("bandpass"),
        "soft" => json!(false),
        _ => Value::Null,
    };
    for k in want.keys().chain(got.keys()).filter(|k| !skip.contains(&k.as_str())) {
        let (w, g) = (want.get(k).cloned().unwrap_or_else(|| unset(k)), got.get(k).cloned().unwrap_or_else(|| unset(k)));
        if !close(&w, &g) {
            return Some(format!("{kind}.{k}: theirs {w}, ours {g}"));
        }
    }
    None
}

#[test]
fn cues_ask_for_the_voices_the_electron_app_asks_for() {
    let fx: Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/cues.json")).unwrap()).unwrap();
    let mut wrong = Vec::new();
    let mut seen = 0;
    for run in fx["runs"].as_array().unwrap() {
        let (theme, shelf, boot) = (run["theme"].as_str().unwrap(), run["shelf"].as_str().unwrap(), run["boot"].as_str().unwrap());
        let mut sound = Sound::default();
        sound.set_theme(theme);
        sound.set_shelf(shelf);
        sound.set_boot(boot, 1.0);
        sound.steady();
        for (i, step) in run["cues"].as_array().unwrap().iter().enumerate() {
            let (name, dir, at) = (step["cue"].as_str().unwrap(), step["dir"].as_i64().unwrap() as i32, step["at"].as_f64().unwrap());
            // the fixture's clock starts at a million ms
            let cue = sound.cue(name, dir, 1e6 + at * 1000.0);
            let theirs = step["voices"].as_array().unwrap();
            seen += theirs.len();
            let off = if theirs.len() != cue.voices.len() {
                Some(format!("{} voices where theirs has {}", cue.voices.len(), theirs.len()))
            } else {
                theirs.iter().zip(&cue.voices).enumerate().find_map(|(k, (t, o))| differs(t, o).map(|d| format!("voice {k}: {d}")))
            };
            if let Some(off) = off {
                wrong.push(format!("{theme} on {shelf} ({boot}), step {i} {name}: {off}"));
            }
        }
    }
    assert!(seen > 2000, "only {seen} voices in the fixture");
    assert!(wrong.is_empty(), "{} differ:\n{}", wrong.len(), wrong.iter().take(40).cloned().collect::<Vec<_>>().join("\n"));
}
