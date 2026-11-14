//! The covers drawn for games without box art against the Electron app's: `fixtures/drawn.json` holds covers drawn by
//! renderer/covers.js in Chromium (scripts/rust-sound-fixtures.js), as a coarse grid of their colours. The same
//! title in the same case must draw the same cover here: the same scene in the same colours, the title in its place.

use rustyshelf_core::drawn;
use rustyshelf_core::looks::look;
use serde_json::Value;

#[test]
fn covers_are_drawn_as_the_electron_app_draws_them() {
    let fx: Value = serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/drawn.json")).unwrap()).unwrap();
    let mut wrong = Vec::new();
    for c in fx["covers"].as_array().unwrap() {
        let (id, title, side) = (c["look"].as_str().unwrap(), c["title"].as_str().unwrap(), c["side"].as_str().unwrap());
        let pic = if side == "back" {
            let facts: Vec<(String, String)> = c["facts"].as_array().unwrap().iter().map(|f| (f[0].as_str().unwrap().to_string(), f[1].as_str().unwrap().to_string())).collect();
            drawn::back(look(id), title, &facts, c["blurb"].as_str().unwrap())
        } else if side == "surprise" {
            drawn::surprise(look(id))
        } else if side == "add" {
            drawn::add_games(look(id))
        } else {
            drawn::front(look(id), title, c["region"].as_str().unwrap_or(""), c["scene"].as_str(), None)
        };
        let (w, h, cell) = (c["width"].as_u64().unwrap() as usize, c["height"].as_u64().unwrap() as usize, c["cell"].as_u64().unwrap() as usize);
        assert_eq!((pic.width as usize, pic.height as usize), (w, h), "{id} {title}");
        // the same grid of colours: each cell the average of its pixels
        let (mut total, mut worst, mut cells) = (0.0f64, 0.0f64, 0usize);
        let theirs = c["grid"].as_array().unwrap();
        for gy in (0..=h - cell).step_by(cell) {
            for gx in (0..=w - cell).step_by(cell) {
                let mut sum = [0u64; 3];
                for py in gy..gy + cell {
                    for px in gx..gx + cell {
                        let o = (py * w + px) * 4;
                        for (k, s) in sum.iter_mut().enumerate() {
                            *s += u64::from(pic.rgba[o + k]);
                        }
                    }
                }
                let want = &theirs[cells];
                let off = (0..3).map(|k| (sum[k] as f64 / (cell * cell) as f64 - want[k].as_f64().unwrap()).abs()).sum::<f64>() / 3.0;
                // WHERE=1 says which cells are far off
                if off > 45.0 && std::env::var("WHERE").is_ok() {
                    println!("  at ({gx}, {gy}): ours {:?}, theirs {want}", sum.map(|v| v / (cell * cell) as u64));
                }
                total += off;
                worst = worst.max(off);
                cells += 1;
            }
        }
        assert_eq!(cells, theirs.len(), "{id} {title}");
        let mean = total / cells as f64;
        // letters' edges and a JPEG's grain differ a little; a scene, a colour or a line of the title out of place
        // differs a lot
        let bad = mean > 4.0 || worst > 45.0;
        println!("{}{id} {side} \"{title}\": cells differ by {mean:.1} of 255 on average, {worst:.0} at most", if bad { "WRONG  " } else { "ok     " });
        if bad {
            wrong.push(format!("{id} {title}"));
        }
    }
    assert!(wrong.is_empty(), "{wrong:?}");
}
