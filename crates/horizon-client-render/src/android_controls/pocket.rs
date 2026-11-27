//! Horizon PE: the parts of the phone controls that are this port's own (a held Attack button,
//! a held Use button that builds, the finger on the hotbar, the hotbar's size), kept apart from
//! the engine's and free of the game, so each rule has a test that runs on any machine.
use std::time::{Duration, Instant};

/// What a held Attack button finds under the crosshair.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldTarget {
    /// A hostile creature: a hold hits it without being asked twice.
    Monster(i32),
    /// Any other creature, a player included: hit only by the press that began on it.
    Living(i32),
    /// Something that is not alive (a boat, a frame): a press each.
    Thing,
    Block,
    Nothing,
}

/// A press this short that found nothing to act on still swings once, as a click does.
pub const AIR_TAP: Duration = Duration::from_millis(250);
/// The least between two hits a hold makes by itself, where the rules have no recharge.
pub const HOLD_HIT_GAP: Duration = Duration::from_millis(250);

/// What the game does for a held Attack button this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HoldStep {
    /// Keep the attack key down on the block (mining).
    pub mine: bool,
    /// Hit what the crosshair is on, now.
    pub hit: bool,
}

/// One press of the Attack button, from the finger landing to its lifting.
///
/// A mouse's held button mines and hits once. A thumb that holds the button to steer with it
/// wants the hit again when the weapon has recharged, and nothing it did not mean:
/// - on a block it mines, until this hold has hit a creature (a fight breaks no blocks);
/// - on a monster it hits each time the weapon has recharged;
/// - on any other creature it hits only if the press began on it, so nothing that walks in
///   front (a villager, a pet, another player) is hit by a hold that was mining;
/// - on a thing that is not alive only the press itself hits;
/// - on nothing it does nothing: a swing at air would reset the recharge. A press shorter
///   than [`AIR_TAP`] that found nothing still swings once when lifted, as a click.
#[derive(Debug, Default)]
pub struct AttackHold {
    /// When the finger landed; None while the button is up.
    since: Option<Instant>,
    /// The creature the press landed on.
    started_on: Option<i32>,
    /// This hold has hit something alive: it breaks no blocks any more.
    fought: bool,
    /// This hold has hit or mined: lifting it is not a tap in the air.
    acted: bool,
    /// The last hit this hold made.
    hit_at: Option<Instant>,
}

impl AttackHold {
    pub fn held(&self) -> bool {
        self.since.is_some()
    }
    /// The button is down this frame, with `target` under the crosshair. `recharged`: the
    /// weapon's attack strength is full.
    pub fn down(&mut self, now: Instant, target: HoldTarget, recharged: bool) -> HoldStep {
        let pressed = self.since.is_none();
        if pressed {
            self.since = Some(now);
        }
        let mut step = HoldStep::default();
        match target {
            HoldTarget::Block if !self.fought => {
                self.acted = true;
                step.mine = true;
            }
            HoldTarget::Block | HoldTarget::Nothing => {}
            HoldTarget::Thing => {
                if pressed {
                    self.acted = true;
                    step.hit = true;
                }
            }
            HoldTarget::Monster(id) | HoldTarget::Living(id) => {
                if pressed {
                    self.started_on = Some(id);
                }
                let chosen =
                    matches!(target, HoldTarget::Monster(_)) || self.started_on == Some(id);
                let rested = self
                    .hit_at
                    .is_none_or(|at| now.duration_since(at) >= HOLD_HIT_GAP);
                if chosen && (pressed || (recharged && rested)) {
                    self.fought = true;
                    self.acted = true;
                    self.hit_at = Some(now);
                    step.hit = true;
                }
            }
        }
        step
    }
    /// The button is up this frame. True when the press that just ended was a tap that found
    /// nothing: it swings once.
    pub fn up(&mut self, now: Instant) -> bool {
        let hold = std::mem::take(self);
        hold.since
            .is_some_and(|since| !hold.acted && now.duration_since(since) < AIR_TAP)
    }
}

/// One hold of the Use button with a block in hand.
///
/// A held Use places again every fourth tick, on whatever side of whatever block the crosshair
/// is on by then: a thumb that steers while it holds gets blocks on the top and the sides of the
/// ones it just placed. A hold builds one way instead: its first block chooses the side (the top
/// of a block, or its north side, ...), and for as long as the button stays down a block goes
/// only on that same side. A pillar, a bridge, a row along the ground and a wall all keep one
/// side from their first block to their last. Lifting the finger ends it.
#[derive(Debug, Default)]
pub struct PlaceHold {
    /// The side the hold's first use was on (the direction from the block that was touched to
    /// the one that was placed).
    face: Option<[i32; 3]>,
}

impl PlaceHold {
    /// The button is up, or pressed anew: the next use may be on any side.
    pub fn reset(&mut self) {
        self.face = None;
    }
    /// May the hold's repeat use the side `face` of a block?
    pub fn allows(&self, face: [i32; 3]) -> bool {
        self.face.is_none_or(|first| first == face)
    }
    /// A use on the side `face` did something: the hold keeps to it.
    pub fn used(&mut self, face: [i32; 3]) {
        self.face.get_or_insert(face);
    }
}

/// The finger that landed on the hotbar and slides along it.
#[derive(Debug, Default)]
pub struct HotbarSlide {
    finger: Option<u64>,
}

impl HotbarSlide {
    pub fn active(&self) -> bool {
        self.finger.is_some()
    }
    pub fn cancel(&mut self) {
        self.finger = None;
    }
    /// One touch event of finger `id`. `landed`: the slot under it if it is on the hotbar (asked
    /// of a finger that starts); `column`: the slot in its column, wherever it is. The slot to
    /// select when the event is the hotbar's, None when it is some other touch.
    ///
    /// iOS names a finger by an address it hands to a later finger once the first has lifted, so
    /// a start is always a new finger whatever its id: it never goes on with an old slide.
    pub fn touch(
        &mut self,
        id: u64,
        started: bool,
        ended: bool,
        landed: Option<usize>,
        column: usize,
    ) -> Option<usize> {
        if started {
            if self.finger == Some(id) {
                self.finger = None;
            }
            let slot = landed?;
            if !ended {
                self.finger = Some(id);
            }
            return Some(slot);
        }
        if self.finger != Some(id) {
            return None;
        }
        if ended {
            self.finger = None;
        }
        Some(column)
    }
}

/// The hotbar slot a finger at `point` means, in the HUD's own area of `w` x `h` (its bottom
/// edge is the hotbar's), the hotbar drawn `zoom` times its size.
///
/// A finger landing: the hotbar's strip, a little above it and past its two ends, and all of
/// what is under it down to the screen's edge. `sliding`: the finger landed on the hotbar and
/// slides; wherever it is now, the slot is the one in its column, so the thumb need not stay
/// on the strip.
pub fn hotbar_slot_at(w: f32, h: f32, zoom: f32, point: [f32; 2], sliding: bool) -> Option<usize> {
    let zoom = zoom.max(1.0);
    let x = (point[0] - w / 2.0) / zoom + 90.0;
    // A strip of the usual size gets more room above it than one already made bigger.
    let above = (34.0 - 22.0 * zoom).max(3.0);
    let landed = point[1] >= h - 22.0 * zoom - above && (-4.0..184.0).contains(&x);
    (sliding || landed).then(|| (x / 20.0).floor().clamp(0.0, 8.0) as usize)
}

/// The HUD's margins `zone` (left, top, right, bottom, in GUI pixels) with the bottom one no
/// less than the home bar's `strip`, in a view `height` high: the hotbar stands above the strip
/// iOS keeps for the home bar. The strip is taken whole, and never as more than a quarter of the
/// view whatever it is said to be.
pub fn above_home_bar(mut zone: [f32; 4], strip: f32, height: f32) -> [f32; 4] {
    if strip.is_finite() && strip > 0.0 {
        zone[3] = zone[3].max(strip.ceil().min((height / 4.0).floor()));
    }
    zone
}

/// A slot's width a thumb finds without looking, in millimetres. Apple's least for anything
/// tapped is 44 points, 6.9 mm.
pub const SLOT_MM: f32 = 6.6;
/// The slot widths that may be asked for (the "Hotbar size" slider's ends). One no wider than
/// the hotbar already is leaves it as it is.
pub const SLOT_MM_RANGE: [f32; 2] = [3.0, 10.0];
/// How much of the HUD's width the hotbar may take.
const WIDEST: f32 = 0.62;
/// Two buttons may overlap by this much of each one's size before they count as in each
/// other's way: their art does not reach its rectangle's corners.
const INSET: f32 = 0.06;

/// Where a bigger hotbar goes.
pub struct HotbarRoom<'a> {
    /// The surface, in pixels.
    pub surface: [f32; 2],
    /// The HUD's area in it (x, y, width, height, in pixels): the hotbar is at its bottom middle.
    pub area: [f32; 4],
    /// Pixels to a GUI pixel: the hotbar's size as it is.
    pub gui: f32,
    /// Pixels to a millimetre; 0 where it is not known (the hotbar stays as it is).
    pub px_per_mm: f32,
    /// The slot width asked for, in millimetres ([`SLOT_MM`] unless the player chose another).
    pub slot_mm: f32,
    /// The buttons on screen (x, y, width, height as fractions of the surface), the Inventory
    /// button left out when it is the factory's.
    pub obstacles: &'a [[f32; 4]],
    /// The Inventory button where the factory put it, with what stands beside it: it moves to
    /// the hotbar's end when the hotbar grows over it, as Bedrock has it.
    pub inventory: Option<[f32; 4]>,
    /// How much of `inventory`'s width is the Inventory button itself (the rest is its neighbour).
    pub inventory_own: f32,
    pub left_handed: bool,
}

/// A hotbar's size and what moves for it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HotbarFit {
    /// How many times its usual size the hotbar is drawn; 1 leaves everything as it was.
    pub zoom: f32,
    /// The Inventory button's centre (a fraction of the surface's width) when it moves.
    pub inventory_x: Option<f32>,
    /// A slot's width as it is drawn, in millimetres; 0 when the hotbar is left as it is.
    pub slot_mm: f32,
}

impl Default for HotbarFit {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            inventory_x: None,
            slot_mm: 0.0,
        }
    }
}

fn inset([x, y, w, h]: [f32; 4]) -> [f32; 4] {
    let keep = 1.0 - 2.0 * INSET;
    [x + w * INSET, y + h * INSET, w * keep, h * keep]
}

fn overlap(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3]
}

/// The biggest hotbar, up to slots of the width asked for, that is in no button's way. Its size
/// is a
/// whole number of pixels to a GUI pixel, so its art and its numbers stay sharp.
pub fn fit_hotbar(room: &HotbarRoom) -> HotbarFit {
    let [sw, sh] = room.surface;
    let [ax, ay, aw, ah] = room.area;
    let none = HotbarFit::default();
    if room.px_per_mm <= 0.0 || room.gui < 1.0 || sw < 1.0 || sh < 1.0 {
        return none;
    }
    let px = |[x, y, w, h]: [f32; 4]| [x * sw, y * sh, w * sw, h * sh];
    let obstacles: Vec<[f32; 4]> = room.obstacles.iter().map(|r| inset(px(*r))).collect();
    let [least, most] = SLOT_MM_RANGE;
    let slot_mm = if room.slot_mm.is_finite() {
        room.slot_mm.clamp(least, most)
    } else {
        SLOT_MM
    };
    let wanted = (slot_mm * room.px_per_mm / 20.0).ceil();
    let mut size = wanted.min((aw * WIDEST / 182.0).floor());
    while size > room.gui {
        let hot = [
            ax + aw / 2.0 - 91.0 * size,
            ay + ah - 22.0 * size,
            182.0 * size,
            22.0 * size,
        ];
        let fit = HotbarFit {
            zoom: size / room.gui,
            inventory_x: None,
            slot_mm: 20.0 * size / room.px_per_mm,
        };
        size -= 1.0;
        if obstacles.iter().any(|o| overlap(hot, *o)) {
            continue;
        }
        let Some(inventory) = room.inventory.map(px) else {
            return fit;
        };
        if !overlap(hot, inset(inventory)) {
            return fit;
        }
        // Beside the hotbar's end, on the side it was on.
        let gap = 0.015 * sh;
        let x = if room.left_handed {
            hot[0] - gap - inventory[2]
        } else {
            hot[0] + hot[2] + gap
        };
        let moved = [x, inventory[1], inventory[2], inventory[3]];
        if x < 0.0 || x + inventory[2] > sw || obstacles.iter().any(|o| overlap(inset(moved), *o)) {
            continue;
        }
        let own = inventory[2] * room.inventory_own;
        let centre = if room.left_handed {
            x + inventory[2] - own / 2.0
        } else {
            x + own / 2.0
        };
        return HotbarFit {
            inventory_x: Some(centre / sw),
            ..fit
        };
    }
    none
}

/// What a player set for the phone and the hand, not for one layout: where the virtual mouse's
/// LMB and RMB stand, the hotbar's size, how holding Attack and Use act, and the hitboxes' size. A preset, the other
/// control system and "Default" bring layouts of their own, and upstream carries a list of such
/// settings over each of them by hand; these are ours on that list. Without it the pair of
/// buttons was back at its factory place after any of them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Own {
    mouse_buttons_pos: Option<[f32; 2]>,
    hotbar_mm: f32,
    big_hotbar: bool,
    hold_attack: bool,
    place_one_way: bool,
    hitbox_scale: f32,
    hitbox_on_servers: bool,
}

impl super::layout::Settings {
    pub fn own(&self) -> Own {
        Own {
            mouse_buttons_pos: self.mouse_buttons_pos,
            hotbar_mm: self.hotbar_mm,
            big_hotbar: self.big_hotbar,
            hold_attack: self.hold_attack,
            place_one_way: self.place_one_way,
            hitbox_scale: self.hitbox_scale,
            hitbox_on_servers: self.hitbox_on_servers,
        }
    }

    pub fn keep_own(&mut self, own: Own) {
        self.mouse_buttons_pos = own.mouse_buttons_pos;
        self.hotbar_mm = own.hotbar_mm;
        self.big_hotbar = own.big_hotbar;
        self.hold_attack = own.hold_attack;
        self.place_one_way = own.place_one_way;
        self.hitbox_scale = own.hitbox_scale;
        self.hitbox_on_servers = own.hitbox_on_servers;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preset_keeps_what_the_player_set_for_the_phone() {
        use super::super::layout::{ControlStyle, Settings};
        let tmp = std::env::temp_dir().join(format!("horizon-own-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let mut s = Settings {
            mouse_buttons_pos: Some([0.25, 0.4]),
            hotbar_mm: 5.0,
            big_hotbar: false,
            hold_attack: false,
            place_one_way: false,
            hitbox_scale: 1.5,
            hitbox_on_servers: true,
            ..Settings::default()
        };
        let own = s.own();
        // A preset of the player's, saved before the pair was moved again.
        s.save_profile(&tmp, "Mine").unwrap();
        s.mouse_buttons_pos = Some([0.6, 0.3]);
        let own_now = s.own();
        assert_ne!(own, own_now);
        s.select_preset(&tmp, ControlStyle::Mcpe, "mine");
        assert_eq!(s.own(), own_now, "a preset of the player's");
        s.select_preset(&tmp, ControlStyle::Mcpe, "default");
        assert_eq!(s.own(), own_now, "the Default preset");
        s.switch_control_system(&tmp, ControlStyle::GoyControl);
        assert_eq!(s.own(), own_now, "the other control system");
        s.switch_control_system(&tmp, ControlStyle::Mcpe);
        assert_eq!(s.own(), own_now, "and back");
        s.apply_preset("default");
        assert_eq!(s.own(), own_now, "Default applied");
        let loaded = Settings::load_profile(&tmp, "mine");
        assert_eq!(loaded.own(), own_now, "a saved profile");
        // And the file a later start reads has it.
        let read = Settings::load_from(&tmp.join("horizon-touch.json"));
        assert_eq!(read.own(), own_now, "the saved file");
        let _ = std::fs::remove_dir_all(&tmp);
    }
    use HoldTarget::{Block, Living, Monster, Nothing, Thing};

    const HIT: HoldStep = HoldStep {
        mine: false,
        hit: true,
    };
    const MINE: HoldStep = HoldStep {
        mine: true,
        hit: false,
    };
    const IDLE: HoldStep = HoldStep {
        mine: false,
        hit: false,
    };

    /// A hold and its clock: `down` and `up` at so many milliseconds from the start.
    struct Press {
        hold: AttackHold,
        start: Instant,
    }
    impl Press {
        fn new() -> Self {
            Self {
                hold: AttackHold::default(),
                start: Instant::now(),
            }
        }
        fn down(&mut self, ms: u64, target: HoldTarget, recharged: bool) -> HoldStep {
            let now = self.start + Duration::from_millis(ms);
            self.hold.down(now, target, recharged)
        }
        fn up(&mut self, ms: u64) -> bool {
            self.hold.up(self.start + Duration::from_millis(ms))
        }
    }

    #[test]
    fn a_hold_on_a_monster_hits_each_time_the_weapon_has_recharged() {
        let mut p = Press::new();
        assert_eq!(p.down(0, Monster(7), false), HIT, "the press");
        assert_eq!(p.down(16, Monster(7), false), IDLE);
        assert_eq!(p.down(400, Monster(7), false), IDLE);
        assert_eq!(p.down(625, Monster(7), true), HIT, "recharged");
        assert_eq!(p.down(641, Monster(7), false), IDLE);
        assert_eq!(p.down(1250, Monster(7), true), HIT);
        assert!(!p.up(1300), "a hold that hit is no tap in the air");
        assert!(!p.hold.held());
    }

    #[test]
    fn a_hold_never_swings_at_air_and_a_short_tap_swings_once() {
        let mut p = Press::new();
        assert_eq!(p.down(0, Nothing, true), IDLE);
        assert_eq!(p.down(100, Nothing, true), IDLE);
        assert!(p.up(120), "a tap at nothing is a click");
        assert_eq!(p.down(1000, Nothing, true), IDLE);
        assert!(!p.up(1400), "a long press at nothing is not");
        assert!(!p.up(1500), "nothing swings while the button is up");
    }

    #[test]
    fn a_monster_that_comes_into_a_hold_is_hit_and_ends_the_mining() {
        let mut p = Press::new();
        assert_eq!(p.down(0, Block, true), MINE);
        assert_eq!(p.down(500, Block, true), MINE);
        assert_eq!(p.down(516, Monster(3), true), HIT);
        assert_eq!(p.down(532, Block, true), IDLE, "a fight breaks none");
        assert_eq!(p.down(600, Monster(3), true), IDLE, "too soon");
        assert_eq!(p.down(800, Monster(3), true), HIT);
        p.up(900);
        assert_eq!(p.down(1000, Block, true), MINE, "the next press mines");
    }

    #[test]
    fn another_creature_is_hit_only_by_the_press_that_began_on_it() {
        let mut p = Press::new();
        assert_eq!(p.down(0, Block, true), MINE);
        assert_eq!(p.down(300, Living(9), true), IDLE, "it walked in front");
        p.up(310);
        assert_eq!(p.down(400, Living(9), false), HIT, "the press is on it");
        assert_eq!(p.down(1100, Living(9), true), HIT, "the hold goes on");
        assert_eq!(p.down(1800, Living(4), true), IDLE, "not on another");
    }

    #[test]
    fn a_thing_takes_a_press_each() {
        let mut p = Press::new();
        assert_eq!(p.down(0, Thing, true), HIT);
        assert_eq!(p.down(900, Thing, true), IDLE);
        assert!(!p.up(950));
    }

    #[test]
    fn the_hud_stands_above_the_home_bar() {
        // A phone with a home bar, 63 pixels at 4 to a GUI pixel: 16 GUI pixels, rounded up.
        assert_eq!(
            above_home_bar([0.0; 4], 15.75, 292.0),
            [0.0, 0.0, 0.0, 16.0]
        );
        // A margin the player set is kept when it is the bigger one, and the sides are theirs.
        let set = [10.0, 8.0, 10.0, 20.0];
        assert_eq!(above_home_bar(set, 15.75, 292.0), set);
        assert_eq!(
            above_home_bar([10.0, 8.0, 10.0, 8.0], 15.75, 292.0)[3],
            16.0
        );
        // A phone with a home button has no strip.
        assert_eq!(above_home_bar(set, 0.0, 292.0), set);
        assert_eq!(above_home_bar([0.0; 4], f32::NAN, 292.0), [0.0; 4]);
        assert_eq!(above_home_bar([0.0; 4], 900.0, 292.0)[3], 73.0, "a quarter");
    }

    #[test]
    fn a_hold_of_use_builds_on_the_side_of_its_first_block() {
        const UP: [i32; 3] = [0, 1, 0];
        const NORTH: [i32; 3] = [0, 0, -1];
        let mut hold = PlaceHold::default();
        assert!(hold.allows(UP) && hold.allows(NORTH), "none chosen yet");
        hold.used(UP);
        assert!(hold.allows(UP), "the next block of a pillar or a row");
        assert!(!hold.allows(NORTH), "not the new block's side");
        // The first side stays the hold's, whatever is used after it.
        hold.used(NORTH);
        assert!(hold.allows(UP) && !hold.allows(NORTH));
        // Lifting the finger ends it: the next press chooses again.
        hold.reset();
        assert!(hold.allows(NORTH));
        hold.used(NORTH);
        assert!(!hold.allows(UP));
    }

    #[test]
    fn a_finger_on_the_hotbar_slides_and_its_id_is_free_once_it_lifts() {
        let mut slide = HotbarSlide::default();
        assert_eq!(slide.touch(5, true, false, Some(2), 2), Some(2));
        assert!(slide.active());
        assert_eq!(slide.touch(5, false, false, None, 6), Some(6), "slid");
        assert_eq!(slide.touch(8, false, false, None, 0), None, "another");
        assert_eq!(slide.touch(5, false, true, None, 6), Some(6), "lifted");
        assert!(!slide.active());
        // iOS gives the lifted finger's id to the next one, which is on the Attack button.
        assert_eq!(slide.touch(5, true, false, None, 8), None);
        assert_eq!(slide.touch(5, false, false, None, 8), None, "no slot");
        assert_eq!(slide.touch(5, false, true, None, 8), None);
    }

    #[test]
    fn a_slide_whose_end_was_never_seen_does_not_take_the_next_finger() {
        let mut slide = HotbarSlide::default();
        assert_eq!(slide.touch(5, true, false, Some(0), 0), Some(0));
        // A screen opened under the finger: its lift went to the screen. A start is a new finger.
        assert_eq!(slide.touch(5, true, false, None, 8), None);
        assert_eq!(slide.touch(5, false, false, None, 8), None);
        assert!(!slide.active());
    }

    #[test]
    fn hotbar_touch_is_generous_and_slides() {
        let at = |x: f32, y: f32, sliding| hotbar_slot_at(427.0, 240.0, 1.0, [x, y], sliding);
        for i in 0..9 {
            assert_eq!(at(133.5 + i as f32 * 20.0, 230.0, false), Some(i));
        }
        assert_eq!(at(133.5, 210.0, false), Some(0), "a band above");
        assert_eq!(at(133.5, 200.0, false), None, "no higher");
        assert_eq!(at(133.5, 250.0, false), Some(0), "under it");
        assert_eq!(at(121.0, 230.0, false), Some(0), "left of the first");
        assert_eq!(at(306.0, 230.0, false), Some(8), "right of the last");
        assert_eq!(at(320.0, 230.0, false), None);
        assert_eq!(at(213.5, 100.0, true), Some(4), "the column decides");
        assert_eq!(at(0.0, 100.0, true), Some(0));
        assert_eq!(at(427.0, 100.0, true), Some(8));
    }

    #[test]
    fn a_bigger_hotbar_is_touched_where_it_is_drawn() {
        // Twice the size: 364 wide and 44 high, in the middle of 600 x 300.
        let at = |x: f32, y: f32| hotbar_slot_at(600.0, 300.0, 2.0, [x, y], false);
        assert_eq!(at(125.0, 280.0), Some(0));
        assert_eq!(at(300.0, 280.0), Some(4));
        assert_eq!(at(475.0, 280.0), Some(8));
        assert_eq!(at(300.0, 254.0), Some(4), "just above");
        assert_eq!(at(300.0, 240.0), None);
        assert_eq!(at(100.0, 280.0), None, "left of it");
    }

    fn room(surface: [f32; 2], gui: f32, obstacles: &[[f32; 4]]) -> HotbarRoom<'_> {
        HotbarRoom {
            surface,
            area: [0.0, 0.0, surface[0], surface[1]],
            gui,
            px_per_mm: 12.84,
            slot_mm: SLOT_MM,
            obstacles,
            inventory: None,
            inventory_own: 1.0,
            left_handed: false,
        }
    }
    const WIDE: [f32; 2] = [2000.0, 900.0];
    const SMALL: [f32; 2] = [1334.0, 750.0];

    #[test]
    fn the_hotbar_grows_to_a_thumbs_width_where_nothing_is_in_the_way() {
        // 12.84 pixels a millimetre: 6.6 mm is 84.7 pixels, so 5 pixels to a GUI pixel (100).
        let fit = fit_hotbar(&room(WIDE, 3.0, &[]));
        assert_eq!(fit.zoom, 5.0 / 3.0);
        assert_eq!(fit.inventory_x, None);
        assert!((fit.slot_mm - 100.0 / 12.84).abs() < 0.001, "as drawn");
        // Never smaller than it is, and never touched where the size is unknown.
        assert_eq!(fit_hotbar(&room(WIDE, 6.0, &[])).zoom, 1.0);
        let mut unknown = room(WIDE, 3.0, &[]);
        unknown.px_per_mm = 0.0;
        assert_eq!(fit_hotbar(&unknown).zoom, 1.0);
    }

    #[test]
    fn the_hotbar_is_the_size_that_is_asked_for() {
        let asked = |mm: f32| {
            let mut r = room(WIDE, 3.0, &[]);
            r.slot_mm = mm;
            fit_hotbar(&r)
        };
        // 12.84 pixels a millimetre and 3 pixels to a GUI pixel: a slot is 4.67 mm as it is.
        assert_eq!(asked(3.0), HotbarFit::default(), "no smaller than it is");
        assert_eq!(asked(4.6), HotbarFit::default());
        // A slot is never narrower than was asked for: the next whole size up.
        assert_eq!(asked(4.7).zoom, 4.0 / 3.0);
        assert_eq!(asked(5.0).zoom, 4.0 / 3.0);
        assert_eq!(asked(6.6).zoom, 5.0 / 3.0);
        // 62% of 2000 pixels is 1240: 6 pixels to a GUI pixel (1092), and no more.
        assert_eq!(asked(9.0).zoom, 2.0);
        assert_eq!(asked(10.0).zoom, 2.0);
        assert_eq!(asked(400.0).zoom, 2.0, "the slider's end is the most");
        assert_eq!(asked(f32::NAN).zoom, 5.0 / 3.0, "the usual size");
    }

    #[test]
    fn the_hotbar_stops_at_a_button_and_at_its_share_of_the_width() {
        // A button at the bottom, from 1400 on: 5 would reach 1455, 4 reaches 1364.
        let button = [[0.7, 800.0 / 900.0, 0.05, 0.1]];
        assert_eq!(fit_hotbar(&room(WIDE, 3.0, &button)).zoom, 4.0 / 3.0);
        // A button on the hotbar's own place leaves it as it is.
        let middle = [[0.49, 0.95, 0.02, 0.05]];
        assert_eq!(fit_hotbar(&room(WIDE, 3.0, &middle)).zoom, 1.0);
        // 62% of 1334 is 827 pixels: 4 (728), not 5 (910).
        assert_eq!(fit_hotbar(&room(SMALL, 3.0, &[])).zoom, 4.0 / 3.0);
    }

    #[test]
    fn the_factory_inventory_button_moves_to_the_hotbars_end() {
        // The factory's Inventory button on a 1334 x 750 screen: 56.25 pixels, at 0.73, 0.95.
        let size = [56.25 / 1334.0, 0.075];
        let at = |x: f32| Some([x - size[0] / 2.0, 0.9125, size[0], size[1]]);
        let mut r = room(SMALL, 3.0, &[]);
        r.inventory = at(0.73);
        let fit = fit_hotbar(&r);
        assert_eq!(fit.zoom, 4.0 / 3.0);
        // The hotbar ends at 667 + 364 = 1031; a gap of 11.25, then half the button.
        let x = fit.inventory_x.expect("it moves") * 1334.0;
        assert!((x - (1031.0 + 11.25 + 28.125)).abs() < 0.01, "{x}");
        // Left-handed, it goes to the other end.
        r.inventory = at(0.27);
        r.left_handed = true;
        let x = fit_hotbar(&r).inventory_x.expect("it moves") * 1334.0;
        assert!((x - (303.0 - 11.25 - 28.125)).abs() < 0.01, "{x}");
        // With no room beside the hotbar for it, the hotbar stays as it is.
        r.left_handed = false;
        r.inventory = at(0.73);
        let wall = [[1040.0 / 1334.0, 0.85, 0.2, 0.15]];
        r.obstacles = &wall;
        assert_eq!(fit_hotbar(&r), HotbarFit::default());
    }
}
