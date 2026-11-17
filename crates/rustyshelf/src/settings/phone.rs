//! Settings → Phone companion: its switch, the address and the PIN a phone pairs with, and the phones that have.

use super::*;

impl Settings {
    /// Something of the companion changed (a phone paired, a new PIN): its page is drawn again.
    pub fn phone_changed(&mut self) {
        if self.stack.last().is_some_and(|p| p.0 == Screen::Phone) {
            self.redraw = true;
        }
    }

    /// What the phone's camera reads to pair, while the companion's page is up.
    pub(super) fn pairing_link(&self) -> Option<String> {
        self.phone.as_ref().filter(|_| self.stack.last().is_some_and(|p| p.0 == Screen::Phone))?.pairing_link()
    }

    pub(super) fn phone_page(&self) -> Page {
        let title = "Phone companion".to_string();
        let Some(phone) = self.phone.clone() else { return Page { title, lead: String::new(), rows: Vec::new() } };
        let st = phone.state();
        let lead = if !st.error.is_empty() {
            st.error.clone()
        } else if st.on {
            format!("On the phone, point the camera at the code, or open {} in its browser and type the PIN. The phone has to be on the same network as this PC. {}", st.url, st.note).trim_end().to_string()
        } else {
            "A phone on your home network browses your games, starts one on this screen, reads its manual, and works as a remote and a keyboard.".into()
        };
        let (a, b, on) = (phone.clone(), phone.clone(), st.enabled);
        let flip = move |s: &mut Settings, phone: &std::sync::Arc<rustyshelf_core::companion::Companion>| {
            if let Err(e) = phone.set(Some(!on), None) {
                s.note = e;
            }
            s.redraw = true;
        };
        let flip2 = flip;
        let mut rows = vec![Row {
            label: "Phone companion".into(),
            value: on_off(st.enabled),
            hint: "Only phones paired with the PIN get in, and only from your home network. Nothing is sent to the internet.".into(),
            press: Some(Box::new(move |s: &mut Settings| flip(s, &a))),
            step: Some(Box::new(move |s: &mut Settings, _| flip2(s, &b))),
        }];
        if st.on {
            rows.push(Row { label: "Address".into(), value: st.url.clone(), hint: "What the phone's browser opens. The code on the right holds it, with the PIN.".into(), ..Row::default() });
            let renew = phone.clone();
            rows.push(Row {
                label: "PIN".into(),
                value: format!("{} {}", st.pin.get(..3).unwrap_or(""), st.pin.get(3..).unwrap_or("")),
                hint: match st.locked {
                    0 => "The phone asks for it once. A PIN pairs one phone; Enter makes a new one.".into(),
                    n => format!("Too many wrong PINs: pairing is locked for {n} more minute{}. Enter makes a new PIN and ends that.", plural_s(n)),
                },
                press: Some(Box::new(move |s: &mut Settings| {
                    renew.renew_pin();
                    s.redraw = true;
                })),
                step: None,
            });
        }
        let (typed, stepped, port) = (phone.clone(), phone.clone(), st.want_port);
        let move_to = |s: &mut Settings, phone: &std::sync::Arc<rustyshelf_core::companion::Companion>, port: Option<u16>| {
            if let Err(e) = port.ok_or("Choose a port from 1024 to 65535.".to_string()).and_then(|p| phone.set(None, Some(p))) {
                s.note = e;
            }
            s.redraw = true;
        };
        rows.push(Row {
            label: "Port".into(),
            value: if st.on && st.port != port { format!("{port} (busy: {} is used)", st.port) } else { port.to_string() },
            hint: "◀ ▶ changes it, Enter types one. From 1024 to 65535; 8730 unless another program has it.".into(),
            press: Some(Box::new(move |s: &mut Settings| {
                let typed = typed.clone();
                s.edit("Port", "A number from 1024 to 65535.", port.to_string(), Box::new(move |s: &mut Settings, v: String| move_to(s, &typed, v.trim().parse().ok())));
            })),
            step: Some(Box::new(move |s: &mut Settings, by| move_to(s, &stepped, u16::try_from(i32::from(port) + by).ok()))),
        });
        for p in &st.phones {
            let (forget, id, name) = (phone.clone(), p.id.clone(), p.name.clone());
            rows.push(Row {
                label: p.name.clone(),
                value: "Forget".into(),
                hint: format!("Paired on {}. Enter takes its access away; it can pair again with the PIN.", setup::day(p.added as u64)),
                press: Some(Box::new(move |s: &mut Settings| {
                    if s.twice(&format!("forget-{id}"), &format!("Enter again to forget {name}.")) {
                        forget.forget(&id);
                        s.redraw = true;
                    }
                })),
                step: None,
            });
        }
        if st.phones.len() > 1 {
            let forget = phone.clone();
            rows.push(Row {
                label: "Forget every phone".into(),
                hint: "Takes every paired phone's access away.".into(),
                press: Some(Box::new(move |s: &mut Settings| {
                    if s.twice("forget-all", "Enter again to forget every phone.") {
                        forget.forget("all");
                        s.redraw = true;
                    }
                })),
                ..Row::default()
            });
        }
        Page { title, lead, rows }
    }
}

/// The pairing code as a picture: a module a pixel, with the quiet edge a camera needs around it.
pub(super) fn qr_image(link: &str) -> Option<Image> {
    use bevy::image::ImageSampler;
    const QUIET: usize = 4;
    let modules = rustyshelf_core::companion::qr_matrix(link).ok()?;
    let side = modules.len() + QUIET * 2;
    let mut data = vec![255u8; side * side * 4];
    for (y, row) in modules.iter().enumerate() {
        for (x, _) in row.iter().enumerate().filter(|(_, dark)| **dark) {
            let at = ((y + QUIET) * side + x + QUIET) * 4;
            data[at..at + 3].fill(0);
        }
    }
    let mut image = crate::loading::to_image(side as u32, side as u32, data);
    image.sampler = ImageSampler::nearest();
    Some(image)
}
