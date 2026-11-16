// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the easing curves
// and timings of Boxshelf's renderer/settings.css and renderer/style.css, as RustyShelf has them.
//! What the screens over the shelf move with: values eased toward a target over a set time, and whole trees of UI
//! faded in and out. Bevy's UI has no opacity for a tree, so a fade scales each node's own colours, keeping the
//! originals in `Orig`.

use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, Gradient};

use crate::shelf::bezier;

/// CSS's cubic-bezier(.2,.8,.2,1): what glides and slides (settings.css)
pub const OUT: [f32; 4] = [0.2, 0.8, 0.2, 1.0];
/// cubic-bezier(.4,0,.2,1): fading in and out
pub const FADE: [f32; 4] = [0.4, 0.0, 0.2, 1.0];
/// cubic-bezier(.34,1.56,.64,1): a switch's knob, past its place and back
pub const SPRING: [f32; 4] = [0.34, 1.56, 0.64, 1.0];
/// cubic-bezier(.22,.9,.25,1): a panel coming in from the side
pub const PANEL: [f32; 4] = [0.22, 0.9, 0.25, 1.0];

/// How far along a move of `dur` seconds that began at `since` is at `now`, on `curve`: 0 to 1 (past 1 for a
/// spring). A move of no length is done.
pub fn progress(now: f32, since: f32, dur: f32, curve: [f32; 4]) -> f32 {
    if dur <= 0.0 {
        return 1.0;
    }
    bezier(curve, (now - since) / dur)
}

/// Something that can be eased from one value to another.
pub trait Lerp: Copy + PartialEq {
    fn lerp_to(self, to: Self, k: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp_to(self, to: f32, k: f32) -> f32 {
        self + (to - self) * k
    }
}

impl Lerp for Vec4 {
    fn lerp_to(self, to: Vec4, k: f32) -> Vec4 {
        self + (to - self) * k
    }
}

/// A value easing toward the last one it was sent to.
#[derive(Clone, Copy, Default)]
pub struct Ease<T: Lerp + Default> {
    from: T,
    to: T,
    since: f32,
    set: bool,
}

impl<T: Lerp + Default> Ease<T> {
    pub fn now(&self, now: f32, dur: f32, curve: [f32; 4]) -> T {
        self.from.lerp_to(self.to, progress(now, self.since, dur, curve))
    }

    /// Heads for `to` from wherever it is now (or is there at once, the first time).
    pub fn go(&mut self, to: T, now: f32, dur: f32, curve: [f32; 4]) {
        if !self.set {
            return self.snap(to, now);
        }
        if to != self.to {
            (self.from, self.to, self.since) = (self.now(now, dur, curve), to, now);
        }
    }

    pub fn snap(&mut self, to: T, now: f32) {
        (self.from, self.to, self.since, self.set) = (to, to, now, true);
    }

    pub fn target(&self) -> T {
        self.to
    }

}

/// A node's colours as drawn, before any fade.
#[derive(Component, Clone)]
pub struct Orig {
    bg: Option<Color>,
    border: Option<[Color; 4]>,
    text: Option<Color>,
    gradient: Option<Vec<Color>>,
    shadow: Option<Vec<Color>>,
    image: Option<Color>,
}

type Colours<'a> = (
    Option<&'a mut BackgroundColor>,
    Option<&'a mut BorderColor>,
    Option<&'a mut TextColor>,
    Option<&'a mut BackgroundGradient>,
    Option<&'a mut BoxShadow>,
    Option<&'a mut ImageNode>,
    Option<&'a Orig>,
);

fn stops(g: &BackgroundGradient) -> Vec<Color> {
    g.0.iter()
        .flat_map(|g| match g {
            Gradient::Linear(l) => l.stops.iter().map(|s| s.color).collect::<Vec<_>>(),
            Gradient::Radial(r) => r.stops.iter().map(|s| s.color).collect(),
            Gradient::Conic(c) => c.stops.iter().map(|s| s.color).collect(),
        })
        .collect()
}

fn set_stops(g: &mut BackgroundGradient, colours: &[Color], k: f32) {
    let mut it = colours.iter();
    for g in g.0.iter_mut() {
        let mut put = |c: &mut Color| {
            if let Some(o) = it.next() {
                *c = o.with_alpha(o.alpha() * k);
            }
        };
        match g {
            Gradient::Linear(l) => l.stops.iter_mut().for_each(|s| put(&mut s.color)),
            Gradient::Radial(r) => r.stops.iter_mut().for_each(|s| put(&mut s.color)),
            Gradient::Conic(c) => c.stops.iter_mut().for_each(|s| put(&mut s.color)),
        }
    }
}

/// Scales the colours of `root` and everything under it to `k` of their own (1: as drawn); a node `deeper` names
/// fades its own part of the tree further. A node seen for the first time is taken as drawn and its colours kept.
pub fn fade_tree(root: Entity, k: f32, deeper: &dyn Fn(Entity) -> f32, children: &Query<&Children>, colours: &mut Query<Colours>, commands: &mut Commands) {
    let mut todo = vec![(root, k)];
    while let Some((e, k)) = todo.pop() {
        let k = k * deeper(e);
        if let Ok(kids) = children.get(e) {
            todo.extend(kids.iter().map(|c| (c, k)));
        }
        let Ok((bg, border, text, gradient, shadow, image, orig)) = colours.get_mut(e) else { continue };
        let o = match orig {
            Some(o) => o.clone(),
            None => {
                let o = Orig {
                    bg: bg.as_ref().map(|b| b.0),
                    border: border.as_ref().map(|b| [b.top, b.right, b.bottom, b.left]),
                    text: text.as_ref().map(|t| t.0),
                    gradient: gradient.as_ref().map(|g| stops(g)),
                    shadow: shadow.as_ref().map(|s| s.0.iter().map(|s| s.color).collect()),
                    image: image.as_ref().map(|i| i.color),
                };
                commands.entity(e).try_insert(o.clone());
                o
            }
        };
        let a = |c: Color| c.with_alpha(c.alpha() * k);
        if let (Some(mut b), Some(c)) = (bg, o.bg) {
            b.0 = a(c);
        }
        if let (Some(mut b), Some([t, r, bo, l])) = (border, o.border) {
            (b.top, b.right, b.bottom, b.left) = (a(t), a(r), a(bo), a(l));
        }
        if let (Some(mut t), Some(c)) = (text, o.text) {
            t.0 = a(c);
        }
        if let (Some(mut g), Some(cs)) = (gradient, o.gradient.as_ref()) {
            set_stops(&mut g, cs, k);
        }
        if let (Some(mut s), Some(cs)) = (shadow, o.shadow.as_ref()) {
            for (s, c) in s.0.iter_mut().zip(cs) {
                s.color = a(*c);
            }
        }
        if let (Some(mut i), Some(c)) = (image, o.image) {
            i.color = a(c);
        }
    }
}

/// The query `fade_tree` works through.
pub type FadeColours<'w, 's> = Query<'w, 's, Colours<'static>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eases_from_where_it_is() {
        let mut e: Ease<f32> = Ease::default();
        e.go(10.0, 0.0, 1.0, OUT);
        assert!((e.now(0.0, 1.0, OUT) - 10.0).abs() < 1e-3, "the first value is there at once");
        e.go(20.0, 1.0, 1.0, OUT);
        assert!((e.now(1.0, 1.0, OUT) - 10.0).abs() < 1e-3);
        assert!((e.now(2.0, 1.0, OUT) - 20.0).abs() < 1e-4);
        let mid = e.now(1.5, 1.0, OUT);
        assert!(mid > 15.0 && mid < 20.0, "out-curves are past halfway at half time: {mid}");
        // a new target midway starts from where it was
        e.go(0.0, 1.5, 1.0, OUT);
        assert!((e.now(1.5, 1.0, OUT) - mid).abs() < 1e-3);
    }

    #[test]
    fn a_spring_overshoots() {
        let peak = (1..100).map(|i| progress(i as f32 / 100.0, 0.0, 1.0, SPRING)).fold(0.0, f32::max);
        assert!(peak > 1.05, "{peak}");
        assert_eq!(progress(2.0, 0.0, 1.0, SPRING), 1.0);
        assert_eq!(progress(0.3, 0.0, 0.0, OUT), 1.0);
    }
}
