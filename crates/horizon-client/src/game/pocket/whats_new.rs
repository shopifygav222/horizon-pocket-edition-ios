// game-module-cfg: target_os = "ios"
//! iOS: what this release brought, on a card over the title screen at the first start with it,
//! after an update and after a new install alike (`horizon_client::whats_new` has the words).
//! "Got it" closes it for good: the version is written down in the private folder.
//!
//! Nobody is to miss it by accident: the card takes every touch, only a finger that goes down
//! and comes up on "Got it" closes it, and not in its first moments (a tap meant for the title
//! screen as it appears). And it can be read again: the title screen has a "What's New" button
//! above Catalog for as long as this version has notes.
//!
//! The card is drawn under the update's card and takes its touches after it, so a newer release
//! is offered first; it steps aside for the catalog and the options.
//!
//! Its notes scroll under a finger, between its title and "Got it", which stay where they are:
//! a release that carries the one before's notes (`whats_new::Earlier`) has more to say than a
//! phone's screen is tall. While there is more below, the card says so beside the button.
use super::Game;
use horizon_client::{
    hud::HudBatch,
    text::Text,
    ui,
    ui_theme::{Face, Ui, color},
    whats_new::{self, Notes},
};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};
use winit::event::{Touch, TouchPhase};

/// The version whose notes were closed, in the private folder.
const SEEN_FILE: &str = "whats-new-seen";
/// How long the card shows before "Got it" answers.
const HOLD: Duration = Duration::from_millis(1500);

struct Card {
    /// The file was read.
    checked: bool,
    /// This version's notes, seen or not: the title's button opens them.
    notes: Option<&'static Notes>,
    /// The card shows.
    open: bool,
    /// Since when it has been drawn.
    since: Option<Instant>,
    /// The finger that went down on "Got it".
    finger: Option<u64>,
    /// The button as last drawn (UI units).
    button: Option<[f32; 4]>,
    /// How far the notes are scrolled, and how far they can be (UI units; as last drawn).
    scroll: f32,
    most: f32,
    /// The finger that scrolls, and where it last was (UI units down the screen).
    drag: Option<(u64, f32)>,
}

/// The card is one for the whole app, as the title screen is.
static CARD: Mutex<Card> = Mutex::new(Card {
    checked: false,
    notes: None,
    open: false,
    since: None,
    finger: None,
    button: None,
    scroll: 0.0,
    most: 0.0,
    drag: None,
});

fn card() -> std::sync::MutexGuard<'static, Card> {
    let mut card = CARD.lock().unwrap_or_else(PoisonError::into_inner);
    if !card.checked {
        card.checked = true;
        let seen = std::fs::read_to_string(seen_path()).ok();
        if let Some((version, _)) = crate::android::app_version() {
            card.notes = whats_new::notes(&version);
            card.open = whats_new::due(&version, seen.as_deref()).is_some();
        }
    }
    card
}

fn seen_path() -> std::path::PathBuf {
    horizon_client::logging::private_home().join(SEEN_FILE)
}

/// One line of the card.
enum Line {
    Heading(&'static str),
    /// The title of an earlier release's notes, under a rule.
    Earlier(&'static str),
    /// An item's line; its first has the dot.
    Item(String, bool),
    Gap(f32),
}

impl Line {
    fn height(&self) -> f32 {
        match self {
            Line::Heading(_) => HEADING[1],
            Line::Earlier(_) => EARLIER[1],
            Line::Item(..) => ITEM[1],
            Line::Gap(gap) => *gap,
        }
    }
}

/// The lines of `parts` (a heading and its items each), the items wrapped to `room`.
fn lay_out(
    ui: &Ui,
    lines: &mut Vec<Line>,
    parts: &[(&'static str, &'static [&'static str])],
    room: f32,
) {
    for &(heading, items) in parts {
        if items.is_empty() {
            continue;
        }
        if !lines.is_empty() {
            lines.push(Line::Gap(14.0));
        }
        lines.push(Line::Heading(heading));
        for item in items {
            let mut line = String::new();
            let mut first = true;
            for word in item.split(' ') {
                let next = if line.is_empty() {
                    word.to_owned()
                } else {
                    format!("{line} {word}")
                };
                if !line.is_empty() && ui.measure(Face::Regular, ITEM[0], &next) > room {
                    let full = std::mem::replace(&mut line, word.to_owned());
                    lines.push(Line::Item(full, first));
                    first = false;
                } else {
                    line = next;
                }
            }
            lines.push(Line::Item(line, first));
            lines.push(Line::Gap(6.0));
        }
    }
}

/// The card is a big one: it is the only thing on screen, and it is read once.
const CARD_W: f32 = 720.0;
const PAD: f32 = 30.0;
const BUTTON: [f32; 2] = [220.0, 50.0];
const ITEM_INDENT: f32 = 22.0;
/// The sizes of the title, a heading and an item's text, each with its line's height.
const TITLE: [f32; 2] = [26.0, 46.0];
const HEADING: [f32; 2] = [19.0, 32.0];
const ITEM: [f32; 2] = [17.0, 26.0];
/// An earlier release's title: a rule, then the words a little larger than a heading's.
const EARLIER: [f32; 2] = [21.0, 50.0];
/// What the card says beside its button while notes are below what shows.
const MORE: &str = "Scroll down for more";

impl Game<'_> {
    /// The notes to show now: over the title's main page only, and not over the catalog or the
    /// options.
    fn whats_new_shown(&self) -> Option<&'static Notes> {
        let on_title = self
            .title
            .as_ref()
            .is_some_and(|t| matches!(t.mode, horizon_client::title::Mode::Main))
            && !matches!(self.screen, super::Screen::Options | super::Screen::Win);
        if !on_title || self.catalog_open() {
            return None;
        }
        let card = card();
        if card.open { card.notes } else { None }
    }

    pub(super) fn draw_whats_new(&self, batch: &mut HudBatch) {
        let Some(notes) = self.whats_new_shown() else {
            return;
        };
        let ready = card().since.get_or_insert_with(Instant::now).elapsed() >= HOLD;
        let mut ui = Ui::new(&self.gui.atlas, batch, (-1.0, -1.0));
        let (w, h) = (ui.w, ui.h);
        ui.fill(0.0, 0.0, w, h, color::with_alpha(color::bg(), 0.72));
        let card_w = (w - 48.0).min(CARD_W);
        let room = card_w - 2.0 * PAD - ITEM_INDENT;
        let mut lines = Vec::new();
        let parts = [
            ("Controls", notes.controls),
            ("New", notes.new),
            ("Fixed", notes.fixed),
        ];
        lay_out(&ui, &mut lines, &parts, room);
        if let Some(earlier) = &notes.earlier {
            lines.push(Line::Gap(12.0));
            lines.push(Line::Earlier(earlier.heading));
            let parts = [("New", earlier.new), ("Fixed", earlier.fixed)];
            lay_out(&ui, &mut lines, &parts, room);
        }
        let body: f32 = lines.iter().map(Line::height).sum();
        // The title above the notes and the button under them stay; the notes take what is
        // left of the screen, and scroll when they are taller.
        let around = PAD + TITLE[1] + 16.0 + BUTTON[1] + PAD;
        let card_h = (around + body).min(h - 16.0).max(around + ITEM[1]);
        let view = card_h - around;
        let most = (body - view).max(0.0);
        let scroll = {
            let mut card = card();
            card.most = most;
            card.scroll = card.scroll.clamp(0.0, most);
            card.scroll
        };
        let (x, y) = ((w - card_w) / 2.0, ((h - card_h) / 2.0).max(8.0));
        ui.rounded(x, y, card_w, card_h, 14.0, color::bg());
        ui.border(x, y, card_w, card_h, 14.0, color::border());
        let title = format!("What's new in {}", notes.version);
        ui.text(
            Face::Semibold,
            TITLE[0],
            x + PAD,
            y + PAD,
            color::text(),
            &title,
        );
        let top = y + PAD + TITLE[1];
        ui.clipped([x, top, card_w, view], |ui| {
            let mut ty = top - scroll;
            for line in &lines {
                let height = line.height();
                // Only what shows, whole or in part.
                if ty + height > top && ty < top + view {
                    match line {
                        Line::Heading(words) => {
                            ui.text(
                                Face::Semibold,
                                HEADING[0],
                                x + PAD,
                                ty,
                                color::text(),
                                words,
                            );
                        }
                        Line::Earlier(words) => {
                            ui.fill(x + PAD, ty + 6.0, card_w - 2.0 * PAD, 1.0, color::border());
                            ui.text(
                                Face::Semibold,
                                EARLIER[0],
                                x + PAD,
                                ty + 16.0,
                                color::text(),
                                words,
                            );
                        }
                        Line::Item(words, first) => {
                            if *first {
                                ui.rounded(x + PAD + 3.0, ty + 9.0, 6.0, 6.0, 3.0, color::ACCENT);
                            }
                            let left = x + PAD + ITEM_INDENT;
                            ui.text(Face::Regular, ITEM[0], left, ty, color::MUTED, words);
                        }
                        Line::Gap(_) => {}
                    }
                }
                ty += height;
            }
        });
        let button = [
            x + card_w - PAD - BUTTON[0],
            y + card_h - PAD - BUTTON[1],
            BUTTON[0],
            BUTTON[1],
        ];
        // More notes are below what shows: said beside the button until the end is reached.
        if scroll < most - 1.0 {
            let ty = button[1] + (BUTTON[1] - 15.0) / 2.0 - 3.0;
            ui.text(Face::Regular, 15.0, x + PAD, ty, color::MUTED, MORE);
        }
        // Pale until it answers.
        let fill = if ready {
            color::ACCENT
        } else {
            color::with_alpha(color::ACCENT, 0.35)
        };
        ui.rounded(button[0], button[1], button[2], button[3], 12.0, fill);
        ui.text_centered(
            Face::Semibold,
            18.0,
            button[0] + button[2] / 2.0,
            button[1] + (button[3] - 18.0) / 2.0 - 2.0,
            color::ON_ACCENT,
            "Got it",
        );
        ui.finish();
        card().button = Some(button);
    }

    /// While the card shows it takes every touch. Only a finger that went down on "Got it" and
    /// is lifted on it closes the card, and not before it has shown for [`HOLD`].
    pub(super) fn whats_new_touch(&mut self, touch: &Touch) -> bool {
        let Some(notes) = self.whats_new_shown() else {
            return false;
        };
        let [w, h] = self.touch_size();
        // The UI's scale for a window of this size (as `Ui::new`).
        let d = (w as f32 / 1040.0).min(h as f32 / 700.0).clamp(1.0, 4.0);
        let p = [touch.location.x as f32 / d, touch.location.y as f32 / d];
        let mut card = card();
        let on_button = card.button.is_some_and(|r| {
            p[0] >= r[0] && p[0] <= r[0] + r[2] && p[1] >= r[1] && p[1] <= r[1] + r[3]
        });
        let ready = card.since.is_some_and(|since| since.elapsed() >= HOLD);
        let drags = card.drag.is_some_and(|(id, _)| id == touch.id);
        match touch.phase {
            TouchPhase::Started => {
                if on_button && ready {
                    card.finger = Some(touch.id);
                } else if !on_button && card.drag.is_none() {
                    // Anywhere else a finger takes hold of the notes.
                    card.drag = Some((touch.id, p[1]));
                }
            }
            TouchPhase::Moved => {
                if let Some((id, last)) = card.drag
                    && drags
                {
                    card.scroll = (card.scroll + last - p[1]).clamp(0.0, card.most);
                    card.drag = Some((id, p[1]));
                }
            }
            TouchPhase::Cancelled => {
                if card.finger == Some(touch.id) {
                    card.finger = None;
                }
                if drags {
                    card.drag = None;
                }
            }
            TouchPhase::Ended => {
                if drags {
                    card.drag = None;
                }
                if card.finger == Some(touch.id) {
                    card.finger = None;
                    if on_button {
                        card.open = false;
                        card.since = None;
                        card.button = None;
                        card.scroll = 0.0;
                        card.drag = None;
                        if let Err(e) = std::fs::write(seen_path(), notes.version) {
                            log::warn!("Horizon PE: notes of {} show again: {e}", notes.version);
                        }
                    }
                }
            }
        }
        true
    }

    /// The title screen's "What's New" button: right above Catalog, the same size, for as long
    /// as this version has notes. x, y and width in GUI pixels.
    fn title_whats_new_layout(&self) -> Option<[f32; 3]> {
        let [x, y, w] = self.title_catalog_layout()?;
        let has_notes = card().notes.is_some();
        has_notes.then_some([x, y - 24.0, w])
    }

    /// The title screen's "What's New" button, drawn as the menu draws its own.
    pub(super) fn draw_title_whats_new(&self, batch: &mut HudBatch) {
        let Some([x, y, w]) = self.title_whats_new_layout() else {
            return;
        };
        let hover = ui::hovered(self.cursor, x, y, w, 20.0);
        let label = Text::literal("What's New");
        ui::button(batch, &self.gui.atlas, x, y, w, 20.0, &label, true, hover);
    }

    /// A finger on the title screen's "What's New" button: lifting on it opens the card again.
    /// True when the button took the touch.
    pub(super) fn title_whats_new_touch(&mut self, touch: &Touch) -> bool {
        let b = self.interface_batch(self.touch_size());
        let [ox, oy] = self.interface_origin();
        let at = (
            touch.location.x as f32 / b.scale - ox,
            touch.location.y as f32 / b.scale - oy,
        );
        let on = self
            .title_whats_new_layout()
            .is_some_and(|[x, y, w]| ui::hovered(at, x, y, w, 20.0));
        if !on || self.catalog_open() {
            return false;
        }
        if touch.phase == TouchPhase::Ended {
            self.audio.play_button_click();
            let mut card = card();
            card.open = true;
            card.since = None;
            card.finger = None;
            card.scroll = 0.0;
            card.drag = None;
        }
        true
    }
}
