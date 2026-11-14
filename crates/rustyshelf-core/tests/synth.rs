//! The sound engine against the Electron app's: `fixtures/synth.json` holds voices rendered by renderer/sound.js in
//! Chromium (scripts/rust-sound-fixtures.js), and the same voices rendered here must sound the same.

use rustyshelf_core::synth::{render, Air, Filter, Fm, Hit, Pad, Partial, Swell, Tone, Voice};
use serde_json::Value;

const AERO: Tone = Tone { partials: &[(1.0, 1.0, 1.0), (2.0, 0.12, 0.5), (3.01, 0.04, 0.35)], decay: 0.32, bright: 9000.0, noise: 0.06, thump: 1.0, verb: 0.7, attack: 0.012, breath: 0.5 };

fn voice(kind: &str, o: &Value) -> Voice {
    let f = |k: &str| o.get(k).and_then(Value::as_f64).map(|x| x as f32);
    let list = |k: &str| -> Vec<f32> { o.get(k).and_then(Value::as_array).map(|a| a.iter().map(|x| x.as_f64().unwrap() as f32).collect()).unwrap_or_default() };
    match kind {
        "hit" => {
            let partials = o.get("partials").and_then(Value::as_array).map(|a| {
                let parts: Vec<Partial> = a.iter().map(|p| (p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32, p[2].as_f64().unwrap() as f32)).collect();
                &*Box::leak(parts.into_boxed_slice())
            });
            Voice::Hit(Hit {
                freq: f("freq"),
                dur: f("dur"),
                vol: f("vol"),
                at: f("at"),
                pan: f("pan"),
                verb: f("verb"),
                bright: f("bright"),
                thump: f("thump"),
                partials,
                attack: f("attack"),
                noise: f("noise"),
                glide: f("glide"),
                glide_time: f("glideTime"),
                sub: f("sub"),
                breath: f("breath"),
            })
        }
        "air" => Voice::Air(Air {
            at: f("at"),
            dur: f("dur"),
            vol: f("vol"),
            filter: match o["filter"].as_str() {
                Some("lowpass") => Filter::Lowpass,
                Some("highpass") => Filter::Highpass,
                _ => Filter::Bandpass,
            },
            freq: f("freq"),
            sweep_to: f("sweepTo"),
            q: f("q"),
            verb: f("verb"),
            pan: f("pan"),
            attack: f("attack"),
            hold: f("hold"),
            soft: o["soft"].as_bool().unwrap_or(false),
        }),
        "fm" => Voice::Fm(Fm { freq: f("freq"), at: f("at"), dur: f("dur"), vol: f("vol"), ratio: f("ratio"), index: f("index"), floor: f("floor"), bright: f("bright"), pan: f("pan"), verb: f("verb"), detune: f("detune"), attack: f("attack") }),
        "pad" => Voice::Pad(Pad { freqs: list("freqs"), at: f("at"), dur: f("dur"), vol: f("vol"), from: f("from"), to: f("to"), open: f("open"), attack: f("attack"), hold: f("hold"), q: f("q"), verb: f("verb") }),
        "swell" => Voice::Swell(Swell { freqs: list("freqs"), at: f("at"), dur: f("dur"), vol: f("vol"), attack: f("attack"), verb: f("verb") }),
        other => panic!("unknown voice {other}"),
    }
}

fn numbers(v: &Value) -> Vec<f32> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap() as f32).collect()
}

#[test]
fn voices_sound_as_the_electron_apps_do() {
    let fx: Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/synth.json")).unwrap()).unwrap();
    let rate = fx["rate"].as_u64().unwrap() as u32;
    let mut wrong = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let (name, mode) = (case["name"].as_str().unwrap(), case["mode"].as_str().unwrap());
        let voices: Vec<Voice> = case["voices"].as_array().unwrap().iter().map(|v| voice(v[0].as_str().unwrap(), &v[1])).collect();
        // the fixtures were rendered at the interface's own volume
        let got = render(&voices, &AERO, rate, 0.7, 7);
        let chans = [&got.left, &got.right];
        let level = |window: usize, of: &dyn Fn(&[f32]) -> f32| -> Vec<Vec<f32>> { chans.iter().map(|c| c.chunks(window).map(of).collect()).collect() };
        let peaks = level(441, &|w| w.iter().fold(0.0f32, |m, x| m.max(x.abs())));
        let rms = level(2205, &|w| (w.iter().map(|x| x * x).sum::<f32>() / 2205.0).sqrt());
        // how far two runs of levels are apart, as a part of the loudest one; the quiet end counts for little
        let apart = |ours: &[Vec<f32>], theirs: &Value| -> f32 {
            let mut worst = 0.0f32;
            for ch in 0..2 {
                let want = numbers(&theirs[ch]);
                let top = want.iter().copied().fold(1e-9, f32::max);
                for (i, w) in want.iter().enumerate() {
                    let g = ours[ch].get(i).copied().unwrap_or(0.0);
                    worst = worst.max((g - w).abs() / (w + 0.05 * top));
                }
            }
            worst
        };
        // noise and the room differ take by take: their energy is compared a quarter of a second at a time
        let coarse = |v: &[f32]| -> Vec<f32> { v.chunks(5).map(|w| (w.iter().map(|x| x * x).sum::<f32>() / 5.0).sqrt()).collect() };
        let (limit, off) = match mode {
            "exact" => (0.04, apart(&peaks, &case["peaks"])),
            "shape" => (0.08, apart(&rms, &case["rms"])),
            _ => {
                let theirs = Value::from(vec![coarse(&numbers(&case["rms"][0])), coarse(&numbers(&case["rms"][1]))]);
                (0.3, apart(&[coarse(&rms[0]), coarse(&rms[1])], &theirs))
            }
        };
        let total = |v: &Value| numbers(&v[0]).iter().chain(numbers(&v[1]).iter()).map(|x| x * x).sum::<f32>().sqrt();
        let energy = rms.iter().flatten().map(|x| x * x).sum::<f32>().sqrt() / total(&case["rms"]);
        let mut line = format!("{name}: {mode}, off by {:.1}% (limit {:.0}%), energy x{energy:.3}", off * 100.0, limit * 100.0);
        let mut bad = off > limit || !(0.87..=1.15).contains(&energy);
        if mode == "exact" {
            // from where it starts
            let from = (case["voices"][0][1]["at"].as_f64().unwrap_or(0.0) * rate as f64).round() as usize;
            let want = numbers(&case["samples"]);
            let top = want.iter().fold(1e-9f32, |m, x| m.max(x.abs()));
            let worst = want.iter().enumerate().map(|(i, w)| (got.left.get(from + i).copied().unwrap_or(0.0) - w).abs()).fold(0.0f32, f32::max) / top;
            line += &format!(", samples off by {:.1}%", worst * 100.0);
            bad |= worst > 0.08;
        }
        println!("{}{line}", if bad { "WRONG  " } else { "ok     " });
        if bad && std::env::var("SYNTH_SHOW").is_ok() {
            let show = |v: &[f32]| v.iter().take(64).map(|x| format!("{:.4}", x)).collect::<Vec<_>>().join(" ");
            println!("  theirs L {}\n  ours   L {}\n  theirs R {}\n  ours   R {}", show(&numbers(&case["rms"][0])), show(&rms[0]), show(&numbers(&case["rms"][1])), show(&rms[1]));
        }
        if bad {
            wrong.push(name.to_string());
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}

/// For looking into a difference by hand: renders the voices in the file `SYNTH_VOICES` names to `SYNTH_OUT`.
#[test]
#[ignore]
fn dump() {
    let voices: Value = serde_json::from_str(&std::fs::read_to_string(std::env::var("SYNTH_VOICES").unwrap()).unwrap()).unwrap();
    let voices: Vec<Voice> = voices.as_array().unwrap().iter().map(|v| voice(v[0].as_str().unwrap(), &v[1])).collect();
    let got = render(&voices, &AERO, 44_100, 0.7, 7);
    std::fs::write(std::env::var("SYNTH_OUT").unwrap(), serde_json::json!({ "l": got.left, "r": got.right }).to_string()).unwrap();
}
