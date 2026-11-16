// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): the motion of
// Boxshelf's renderer/settings.css and renderer/details2.css, as RustyShelf has it.
//! Settings', Details' and the shelf menu's motion: one highlight that glides from row to row, the list scrolling to
//! keep the chosen row in view, a value sliding in on ←/→, a switch's knob springing over, a row giving a little as
//! it's pressed, a page coming in, and the whole of it fading in and out. The UI itself is rebuilt on every change
//! (`draw`), so what moves is kept here, between rebuilds.

use bevy::prelude::*;

use super::{Settings, SettingsUi};
use crate::motion::{self, Ease, FADE, OUT, PANEL, SPRING};

/// seconds: the highlight's glide, the list's scroll, a value's slide, a knob's spring, a press, Settings fading in
/// and out, and a page coming in (settings.css: .bss-hl, .bss-list, bssvl/bssvr, .bss-sw i, bsspress, #bss, bssenter)
const GLIDE: f32 = 0.14;
const SCROLL: f32 = 0.22;
const VALUE: f32 = 0.18;
const KNOB: f32 = 0.34;
const PRESS: f32 = 0.16;
const OPEN: f32 = 0.26;
const CLOSE: f32 = 0.2;
const ENTER: f32 = 0.16;
/// Details' page coming in from the right (details2.css #details)
const DETAILS: f32 = 0.55;
/// Reduce motion: fades only, and quicker
const QUICK: f32 = 0.12;

#[derive(Default)]
pub struct SettingsMotion {
    /// the highlight's place in the list (x, y, width, height, logical pixels from the top of the list's content)
    glide: Ease<Vec4>,
    /// how far down the list is scrolled, and can be
    scroll: Ease<f32>,
    most: f32,
    /// the page on screen: a redraw of the same one keeps its motion going, another starts afresh
    page: String,
    page_at: f32,
    details: bool,
    /// the highlight is placed (not glided) at its next measure, and has a place yet on this page
    snap: bool,
    placed: bool,
    opened_at: f32,
    was_open: bool,
    /// fully in: the shelf behind needn't be drawn
    pub(super) faded_in: bool,
    /// a fade was under way last frame, so the colours are put back once it's done
    fading: bool,
    /// the last ←/→ on a row and when, and the last press
    pub(super) stepped: Option<(i32, f32)>,
    pub(super) pressed: Option<f32>,
    reduce: bool,
}

/// The rows' scrolling frame.
#[derive(Component)]
pub struct ListFrame;

/// The marks that say the list goes on above and below.
#[derive(Component)]
pub struct MoreAbove;

#[derive(Component)]
pub struct MoreBelow;

/// The highlight that glides to the chosen row.
#[derive(Component)]
pub struct Highlight;

#[derive(Component)]
pub struct ChosenRow;

/// The chosen row's value, sliding in from the side its ←/→ came from.
#[derive(Component)]
pub struct ValueSlide(pub i32);

/// A switch's knob, springing over from the side it was on.
#[derive(Component)]
pub struct Knob(pub bool);

/// The chosen row, just pressed.
#[derive(Component)]
pub struct PressedRow;

/// A page coming in: from below, or (Details) from the right.
#[derive(Component)]
pub struct PageBody;

/// A Settings that's closing, fading out from when.
#[derive(Component)]
pub struct Closing(pub f32);

impl SettingsMotion {
    fn length(&self, normal: f32) -> f32 {
        if self.reduce {
            0.0
        } else {
            normal
        }
    }

    /// Called as the UI is rebuilt: notes Settings opening or closing, and a new page. True when the old tree should
    /// fade out rather than go at once.
    pub(super) fn redrawn(&mut self, open: bool, page: String, details: bool, reduce: bool, now: f32) -> bool {
        self.reduce = reduce;
        if open && !self.was_open {
            (self.opened_at, self.was_open, self.faded_in) = (now, true, false);
            self.page.clear();
        }
        if !open {
            let closing = self.was_open;
            (self.was_open, self.faded_in) = (false, false);
            return closing;
        }
        if page != self.page {
            (self.page, self.page_at, self.details, self.snap, self.placed) = (page, now, details, true, false);
        }
        false
    }

    /// Where the highlight and the scroll stand now, for a tree being built: (highlight's rect when it has one, scroll).
    pub(super) fn now(&self, now: f32) -> (Option<Vec4>, f32) {
        (self.placed.then(|| self.glide.now(now, self.length(GLIDE), OUT)), self.scroll.now(now, self.length(SCROLL), OUT))
    }

    /// A ←/→ on the chosen row this recently slides its value in; a press this recently gives the row a nudge.
    pub(super) fn slid(&self, now: f32) -> Option<i32> {
        self.stepped.filter(|s| now - s.1 < VALUE && !self.reduce).map(|s| s.0)
    }

    pub(super) fn flipped(&self, now: f32) -> bool {
        !self.reduce && (self.stepped.is_some_and(|s| now - s.1 < KNOB) || self.pressed.is_some_and(|p| now - p < KNOB))
    }

    pub(super) fn just_pressed(&self, now: f32) -> bool {
        !self.reduce && self.pressed.is_some_and(|p| now - p < PRESS)
    }
}

/// Once laid out: where the chosen row is in the list, and how far the list should be scrolled to show it.
#[allow(clippy::type_complexity)]
pub fn measure(
    time: Res<Time>,
    mut s: ResMut<Settings>,
    frame: Query<(&ComputedNode, &UiGlobalTransform), (With<ListFrame>, Without<Closing>)>,
    chosen: Query<(&ComputedNode, &UiGlobalTransform), With<ChosenRow>>,
) {
    if !s.open {
        return;
    }
    let now = time.elapsed_secs();
    let Ok((f, ft)) = frame.single() else { return };
    let Ok((r, rt)) = chosen.single() else {
        // nothing in the list chosen (Details' Play): no highlight, and a new page starts at its top
        let m = &mut s.motion;
        if std::mem::take(&mut m.snap) {
            m.scroll.snap(0.0, now);
        }
        m.placed = false;
        return;
    };
    let inv = f.inverse_scale_factor;
    let pad_box = ft.translation - f.size / 2.0 + Vec2::new(f.border.min_inset.x, f.border.min_inset.y);
    let at = rt.translation - r.size / 2.0 - pad_box + f.scroll_position;
    let rect = Vec4::new(at.x, at.y, r.size.x, r.size.y) * inv;
    let view = (f.size.y - f.border.min_inset.y - f.border.max_inset.y) * inv;
    let most = (f.content_size.y * inv - view).max(0.0);
    let m = &mut s.motion;
    m.most = most;
    // keep the chosen row, and a little of the rows round it, in view
    let room = (rect.w * 0.6).min(view / 3.0);
    let mut to = m.scroll.target();
    if rect.y - room < to {
        to = rect.y - room;
    }
    if rect.y + rect.w + room > to + view {
        to = rect.y + rect.w + room - view;
    }
    let to = to.clamp(0.0, most);
    // (a new page, or the list's first row chosen after none: placed, not glided to)
    if std::mem::take(&mut m.snap) || !m.placed {
        m.glide.snap(rect, now);
        m.scroll.snap(to, now);
        m.placed = true;
        return;
    }
    let (glide, scroll) = (m.length(GLIDE), m.length(SCROLL));
    m.glide.go(rect, now, glide, OUT);
    m.scroll.go(to, now, scroll, OUT);
}

/// Every frame: the highlight and the scroll where their glides have got to, and the slides, springs and nudges.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn animate(
    time: Res<Time>,
    s: Res<Settings>,
    mut hl: Query<(&mut Node, &mut Visibility), (With<Highlight>, Without<Closing>)>,
    mut frame: Query<&mut ScrollPosition, (With<ListFrame>, Without<Closing>)>,
    mut value: Query<(&ValueSlide, &mut UiTransform), (Without<Knob>, Without<PressedRow>, Without<PageBody>)>,
    mut knob: Query<(&Knob, &mut UiTransform), (Without<PressedRow>, Without<PageBody>)>,
    mut pressed: Query<&mut UiTransform, (With<PressedRow>, Without<PageBody>)>,
    mut page: Query<&mut UiTransform, With<PageBody>>,
    mut above: Query<&mut Visibility, (With<MoreAbove>, Without<Highlight>, Without<MoreBelow>)>,
    mut below: Query<&mut Visibility, (With<MoreBelow>, Without<Highlight>, Without<MoreAbove>)>,
) {
    if !s.open {
        return;
    }
    let now = time.elapsed_secs();
    let m = &s.motion;
    let (rect, scroll) = m.now(now);
    for (mut node, mut seen) in &mut hl {
        match rect {
            Some(r) => {
                (node.left, node.top, node.width, node.height) = (px(r.x), px(r.y), px(r.z), px(r.w));
                seen.set_if_neq(Visibility::Inherited);
            }
            None => {
                seen.set_if_neq(Visibility::Hidden);
            }
        }
    }
    for mut at in &mut frame {
        if at.0.y != scroll {
            at.0.y = scroll;
        }
    }
    let shown = |on: bool| if on { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut above {
        v.set_if_neq(shown(scroll > 1.0));
    }
    for mut v in &mut below {
        v.set_if_neq(shown(scroll < m.most - 1.0));
    }
    let slide = m.stepped.map_or(1.0, |s| motion::progress(now, s.1, VALUE, OUT));
    for (v, mut t) in &mut value {
        t.translation = Val2::px(v.0 as f32 * 16.0 * (1.0 - slide), 0.0);
    }
    let flip = m.stepped.map(|s| s.1).into_iter().chain(m.pressed).fold(f32::MIN, f32::max);
    let spring = motion::progress(now, flip, KNOB, SPRING);
    for (k, mut t) in &mut knob {
        // (from the other end of the switch, 22 of its 52 wide)
        let from = if k.0 { -22.0 } else { 22.0 };
        t.translation = Val2::new(Val::Vh(from * (1.0 - spring) / 10.8), Val::Px(0.0));
    }
    let nudge = m.pressed.map_or(0.0, |p| {
        let k = ((now - p) / PRESS).clamp(0.0, 1.0);
        // bsspress: down to .985 at 40%, back by the end
        if k < 0.4 { k / 0.4 } else { (1.0 - k) / 0.6 }
    });
    for mut t in &mut pressed {
        t.scale = Vec2::splat(1.0 - 0.015 * nudge);
    }
    let enter = 1.0 - motion::progress(now, m.page_at, m.length(if m.details { DETAILS } else { ENTER }), if m.details { PANEL } else { OUT });
    for mut t in &mut page {
        t.translation = if m.details { Val2::new(Val::Vw(6.0 * enter), Val::Px(0.0)) } else { Val2::px(0.0, 10.0 * enter) };
    }
}

/// Settings fading in as it opens and out as it closes, and a new page fading up from .35 as it comes in.
#[allow(clippy::too_many_arguments)]
pub fn fade(
    mut commands: Commands,
    time: Res<Time>,
    mut s: ResMut<Settings>,
    roots: Query<(Entity, Option<&Closing>), With<SettingsUi>>,
    pages: Query<Entity, With<PageBody>>,
    children: Query<&Children>,
    mut colours: motion::FadeColours,
) {
    let now = time.elapsed_secs();
    let reduce = s.motion.reduce;
    let length = |normal: f32| if reduce { QUICK } else { normal };
    let mut fading = false;
    for (root, closing) in &roots {
        match closing {
            Some(c) => {
                let k = 1.0 - motion::progress(now, c.0, length(CLOSE), FADE);
                if k <= 0.0 {
                    commands.entity(root).try_despawn();
                } else {
                    motion::fade_tree(root, k, &|_| 1.0, &children, &mut colours, &mut commands);
                }
            }
            None => {
                let m = &s.motion;
                let open = motion::progress(now, m.opened_at, length(OPEN), FADE).min(1.0);
                let page = if m.details { motion::progress(now, m.page_at + 0.06, length(0.35), FADE) } else { 0.35 + 0.65 * motion::progress(now, m.page_at, length(ENTER), OUT) }.clamp(0.0, 1.0);
                if open < 1.0 || page < 1.0 || m.fading {
                    fading = open < 1.0 || page < 1.0;
                    motion::fade_tree(root, open, &|e| if pages.contains(e) { page } else { 1.0 }, &children, &mut colours, &mut commands);
                }
                if open >= 1.0 && !s.motion.faded_in {
                    s.motion.faded_in = true;
                }
            }
        }
    }
    s.motion.fading = fading;
}
