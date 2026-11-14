//! The CSS painter against Chromium: `fixtures/cssbg.json` holds backgrounds as Chromium paints them over mid grey
//! (scripts/rust-paint-fixtures.js), as a coarse grid of colours: every theme's backdrop, the themes' tiles and the
//! shelf's own layers.

use rustyshelf_core::cssbg;

#[test]
fn paints_what_chromium_paints() {
    let fx: serde_json::Value = serde_json::from_str(include_str!("fixtures/cssbg.json")).unwrap();
    let (cell, under) = (fx["cell"].as_u64().unwrap() as usize, fx["under"].as_f64().unwrap() as f32);
    let mut failed = Vec::new();
    for c in fx["cases"].as_array().unwrap() {
        let (name, w, h) = (c["name"].as_str().unwrap(), c["width"].as_u64().unwrap() as usize, c["height"].as_u64().unwrap() as usize);
        let p = cssbg::paint(c["css"].as_str().unwrap(), w as u32, h as u32, 1.0);
        let theirs: Vec<f32> = c["grid"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
        let (mut sum, mut worst, mut n) = (0.0f32, 0.0f32, 0);
        for gy in 0..h / cell {
            for gx in 0..w / cell {
                let mut ours = [0.0f32; 3];
                for y in gy * cell..(gy + 1) * cell {
                    for x in gx * cell..(gx + 1) * cell {
                        let px = &p.rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
                        let a = f32::from(px[3]) / 255.0;
                        for i in 0..3 {
                            ours[i] += f32::from(px[i]) * a + under * (1.0 - a);
                        }
                    }
                }
                for i in 0..3 {
                    let d = (ours[i] / (cell * cell) as f32 - theirs[n * 3 + i]).abs();
                    sum += d;
                    worst = worst.max(d);
                }
                n += 1;
            }
        }
        let mean = sum / (n * 3) as f32;
        println!("{name}: mean {mean:.2}, worst {worst:.1}");
        // out of 255: rounding and the edges of hard stops differ a little, a layer lost or misplaced a lot; lines that
        // change colour right on the pixels' middles fall either way
        let (most, peak) = if c["ties"] == true { (3.0, 20.0) } else { (1.0, 8.0) };
        if mean > most || worst > peak {
            failed.push(format!("{name} (mean {mean:.2}, worst {worst:.1})"));
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
}

/// Where one background differs: `cargo test -p rustyshelf-core --test cssbg -- --ignored --nocapture where_it_differs` with
/// CASE set to its name.
#[test]
#[ignore]
fn where_it_differs() {
    let fx: serde_json::Value = serde_json::from_str(include_str!("fixtures/cssbg.json")).unwrap();
    let (cell, under) = (fx["cell"].as_u64().unwrap() as usize, fx["under"].as_f64().unwrap() as f32);
    let want = std::env::var("CASE").unwrap_or_default();
    let c = fx["cases"].as_array().unwrap().iter().find(|c| c["name"] == want.as_str()).expect("CASE names no background");
    let (w, h) = (c["width"].as_u64().unwrap() as usize, c["height"].as_u64().unwrap() as usize);
    let css = std::env::var("CSS").unwrap_or(c["css"].as_str().unwrap().to_string());
    let p = cssbg::paint(&css, w as u32, h as u32, 1.0);
    let theirs: Vec<f32> = c["grid"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect();
    for gy in 0..h / cell {
        let mut line = String::new();
        for gx in 0..w / cell {
            let mut ours = 0.0f32;
            for y in gy * cell..(gy + 1) * cell {
                for x in gx * cell..(gx + 1) * cell {
                    let px = &p.rgba[(y * w + x) * 4..(y * w + x) * 4 + 4];
                    let a = f32::from(px[3]) / 255.0;
                    ours += f32::from(px[1]) * a + under * (1.0 - a);
                }
            }
            let d = ours / (cell * cell) as f32 - theirs[(gy * (w / cell) + gx) * 3 + 1];
            line.push(match d.abs() {
                x if x < 1.0 => '.',
                x if x < 3.0 => if d > 0.0 { '+' } else { '-' },
                _ => if d > 0.0 { '#' } else { '=' },
            });
        }
        println!("{line}");
    }
}

/// How long each theme's backdrop takes at 1080p, and at a quarter of that.
#[test]
#[ignore]
fn timing() {
    for t in rustyshelf_core::themes::themes() {
        let css = rustyshelf_core::themes::tint(&t.id, 258.0, 40.0, None);
        let (a, _) = (std::time::Instant::now(), cssbg::paint(&css, 1920, 1080, 1.0));
        let full = a.elapsed();
        let (a, _) = (std::time::Instant::now(), cssbg::paint(&css, 480, 270, 0.25));
        println!("{}: {:?} whole, {:?} at a quarter", t.id, full, a.elapsed());
    }
}
