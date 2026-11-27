//! Horizon PE: "Hitbox size" on the Touch page, asked for in this port's issue 11. A thumb on
//! glass is no mouse: an attack that just misses a player or a creature still lands on it.
//!
//! Each body gets a zone around its box, wider by the same margin on every side, and an attack
//! whose ray misses every box but enters a zone hits that zone's body. What it must never do is
//! decided here, free of the game, so each rule has a test that runs on any machine:
//! - an exact hit stays the game's own: this is only asked when the ray found no entity;
//! - only the aim is forgiven, never the reach: the body itself is no farther than an exact
//!   hit's could be, which is all a server measures;
//! - a block the ray meets first stops it, as it stops an exact hit;
//! - of two bodies the ray passes, the one it passes nearest is hit, not the one in front.
//!
//! Whether the body can be seen at all (not round a corner, not through a door) is the game's
//! to say: it has the blocks. The hits come best first so it can take the first it sees.

/// The setting's range: 1 is the game's own hitboxes, 2 a player twice as wide. Its end is the
/// owner's choice (2026-11-27, after playing with 2): a little past twice.
pub const SCALE_RANGE: [f32; 2] = [1.0, 2.25];
/// The half width a body's margin is worked out from is held between these, in blocks: a
/// silverfish is no harder to hit than a player, and a ghast's zone is not blocks deep.
const HALF_WIDTH: [f64; 2] = [0.3, 0.6];
/// Below this over 1 the setting is off: the slider's lowest stop, however it was rounded.
const OFF: f32 = 0.005;

/// The setting as it is used: within [`SCALE_RANGE`], and 1 when the file had no number.
pub fn clean(scale: f32) -> f32 {
    if scale.is_finite() {
        scale.clamp(SCALE_RANGE[0], SCALE_RANGE[1])
    } else {
        SCALE_RANGE[0]
    }
}

/// Whether the setting widens anything.
pub fn on(scale: f32) -> bool {
    clean(scale) - SCALE_RANGE[0] > OFF
}

/// A body an attack may land on: its box, as the game has it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub lo: [f64; 3],
    pub hi: [f64; 3],
}

/// How far past its box, on every side, `body` is hit at `scale`. The same up and down as to
/// the sides: at 2 a player is twice as wide and a little taller, not twice as tall.
pub fn margin(scale: f32, body: &Body) -> f64 {
    let width = (body.hi[0] - body.lo[0]).max(body.hi[2] - body.lo[2]);
    let half = (width / 2.0).clamp(HALF_WIDTH[0], HALF_WIDTH[1]);
    f64::from(clean(scale) - SCALE_RANGE[0]) * half
}

/// A body the ray missed and still hits.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WideHit {
    /// Which of the bodies given.
    pub index: usize,
    /// From the eyes to the body, at its nearest: what reach is measured by.
    pub distance: f64,
    /// The body's point nearest the ray: what must be in sight.
    pub point: [f64; 3],
    /// How far from the body the ray passed.
    pub miss: f64,
}

/// The stretch of the ray, between 0 and `limit` along it, inside the box.
fn span(
    eye: [f64; 3],
    dir: [f64; 3],
    limit: f64,
    lo: [f64; 3],
    hi: [f64; 3],
) -> Option<(f64, f64)> {
    let (mut enters, mut leaves) = (0.0f64, limit);
    for axis in 0..3 {
        let (from, along) = (eye[axis], dir[axis]);
        if along.abs() < 1e-9 {
            if from < lo[axis] || from > hi[axis] {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo[axis] - from) / along, (hi[axis] - from) / along);
        enters = enters.max(a.min(b));
        leaves = leaves.min(a.max(b));
    }
    (enters <= leaves).then_some((enters, leaves))
}

/// The point of `body` nearest `point`.
fn nearest(point: [f64; 3], body: &Body) -> [f64; 3] {
    std::array::from_fn(|axis| point[axis].clamp(body.lo[axis], body.hi[axis]))
}

/// How far `point` is from `body`: 0 inside it.
fn gap(point: [f64; 3], body: &Body) -> f64 {
    let on_body = nearest(point, body);
    (0..3)
        .map(|axis| (point[axis] - on_body[axis]).powi(2))
        .sum::<f64>()
        .sqrt()
}

/// The bodies an attack along the ray from `eye` along `dir` (of length 1) lands on at `scale`
/// though it met none of their boxes, the one it passed nearest first. `reach`: how far an
/// attack goes (the entity interaction range). `block`: how far along the ray its first block
/// is, if it meets one.
pub fn wide_hits(
    eye: [f64; 3],
    dir: [f64; 3],
    reach: f64,
    block: Option<f64>,
    scale: f32,
    bodies: &[Body],
) -> Vec<WideHit> {
    if !on(scale) {
        return Vec::new();
    }
    let limit = block.map_or(reach, |block| block.min(reach));
    let at = |t: f64| -> [f64; 3] { std::array::from_fn(|axis| eye[axis] + dir[axis] * t) };
    let mut hits: Vec<WideHit> = bodies
        .iter()
        .enumerate()
        .filter_map(|(index, body)| {
            // The reach is the body's, not its zone's: no hit from farther than the game's own.
            let distance = gap(eye, body);
            if distance > reach {
                return None;
            }
            let margin = margin(scale, body);
            let (enters, leaves) = span(
                eye,
                dir,
                limit,
                body.lo.map(|v| v - margin),
                body.hi.map(|v| v + margin),
            )?;
            // A block the ray meets no later than the zone is in front of it.
            if block.is_some_and(|block| enters >= block) {
                return None;
            }
            // Where, inside the zone and before the block, the ray comes nearest the body: the
            // gap to a box along a line has one low point, so thirds close in on it.
            let (mut from, mut to) = (enters, leaves);
            for _ in 0..48 {
                let third = (to - from) / 3.0;
                if gap(at(from + third), body) <= gap(at(to - third), body) {
                    to -= third;
                } else {
                    from += third;
                }
            }
            let passes = at((from + to) / 2.0);
            Some(WideHit {
                index,
                distance,
                point: nearest(passes, body),
                // To a millionth of a millimetre: two bodies passed equally near are told apart
                // by how near they are, not by the last digit of a search.
                miss: (gap(passes, body) * 1e9).round() / 1e9,
            })
        })
        .collect();
    hits.sort_by(|a, b| {
        a.miss
            .total_cmp(&b.miss)
            .then(a.distance.total_cmp(&b.distance))
    });
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The eyes of a player standing at the origin, looking along +z.
    const EYE: [f64; 3] = [0.0, 1.62, 0.0];
    const AHEAD: [f64; 3] = [0.0, 0.0, 1.0];
    const REACH: f64 = 3.0;

    /// A player's box (0.6 wide, 1.8 tall) whose near side is `z` ahead and whose nearer edge
    /// is `beside` to the right of the ray (negative: the ray goes through it).
    fn player(z: f64, beside: f64) -> Body {
        Body {
            lo: [beside, 0.0, z],
            hi: [beside + 0.6, 1.8, z + 0.6],
        }
    }

    fn first(block: Option<f64>, scale: f32, bodies: &[Body]) -> Option<WideHit> {
        wide_hits(EYE, AHEAD, REACH, block, scale, bodies)
            .first()
            .copied()
    }

    #[test]
    fn off_is_the_games_own_hitboxes() {
        let near = [player(2.0, 0.05)];
        for scale in [1.0, 1.004, 0.5, -3.0, f32::NAN, f32::NEG_INFINITY] {
            assert!(!on(scale), "{scale}");
            assert_eq!(first(None, scale, &near), None, "{scale}");
        }
        // A number past the range is the range's end, not more.
        assert_eq!(clean(9.0), 2.25);
        assert_eq!(clean(f32::INFINITY), 1.0);
        assert!(on(1.05));
    }

    #[test]
    fn a_near_miss_lands_and_a_wide_one_does_not() {
        // The ray passes 0.2 beside a player 2 blocks ahead.
        let body = [player(2.0, 0.2)];
        let hit = first(None, 2.0, &body).expect("a margin of 0.3 covers a miss of 0.2");
        assert_eq!(hit.index, 0);
        assert!((hit.miss - 0.2).abs() < 1e-6, "{}", hit.miss);
        // The body's own point, on its near edge at the eyes' height.
        assert!((hit.point[0] - 0.2).abs() < 1e-6 && (hit.point[1] - 1.62).abs() < 1e-6);
        assert!((2.0..=2.6).contains(&hit.point[2]), "{:?}", hit.point);
        // At 150% the margin is 0.15: the same aim misses.
        assert_eq!(first(None, 1.5, &body), None);
        assert!(first(None, 1.7, &body).is_some());
        // And 0.35 beside is a miss at any size.
        assert_eq!(first(None, 2.0, &[player(2.0, 0.35)]), None);
    }

    #[test]
    fn the_zone_is_above_and_below_too_but_no_taller_than_it_is_wide() {
        // Over the head by 0.2: the eyes at 2.0, the head's top at 1.8.
        let over = |height: f64, scale: f32| {
            wide_hits(
                [0.0, height, 0.0],
                AHEAD,
                REACH,
                None,
                scale,
                &[player(2.0, -0.3)],
            )
            .first()
            .copied()
        };
        assert!(over(2.0, 2.0).is_some());
        // Twice the size is 0.3 more on every side, not 1.8 more above.
        assert_eq!(over(2.2, 2.0), None);
    }

    #[test]
    fn only_the_aim_is_forgiven_never_the_reach() {
        // The zone begins inside the reach, the body does not: no hit.
        assert_eq!(first(None, 2.0, &[player(3.1, 0.2)]), None);
        // The body's edge at 2.9 ahead and 0.2 beside is 2.907 from the eyes: inside.
        let hit = first(None, 2.0, &[player(2.9, 0.2)]).expect("a body within reach");
        assert!(hit.distance <= REACH && (hit.distance - 2.907).abs() < 1e-3);
        // Behind the player the zone is no closer than the body is.
        assert_eq!(first(None, 2.0, &[player(-3.9, 0.2)]), None);
    }

    #[test]
    fn a_block_in_front_stops_it() {
        let body = [player(2.0, 0.2)];
        // The zone begins 1.7 along the ray. A block met before that, or right there, is in front.
        assert_eq!(first(Some(1.0), 2.0, &body), None);
        assert_eq!(first(Some(1.7), 2.0, &body), None);
        // One behind the body is behind it.
        assert!(first(Some(2.8), 2.0, &body).is_some());
        // One met inside the zone: the ray is in the zone before it, and how near it came to the
        // body is measured up to the block, not past it.
        let cut = first(Some(1.8), 2.0, &body).expect("in the zone before the block");
        assert!((cut.miss - 0.08f64.sqrt()).abs() < 1e-6, "{}", cut.miss);
        assert!((cut.point[2] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn the_body_passed_nearest_is_hit_not_the_one_in_front() {
        // One player in front, 0.25 beside the ray; another behind it, 0.05 beside.
        let two = [player(1.2, 0.25), player(2.2, 0.05)];
        let hits = wide_hits(EYE, AHEAD, REACH, None, 2.0, &two);
        assert_eq!(hits.iter().map(|h| h.index).collect::<Vec<_>>(), [1, 0]);
        // The same distance from the ray: the nearer body.
        let level = [player(2.2, 0.1), player(1.2, 0.1)];
        assert_eq!(first(None, 2.0, &level).map(|h| h.index), Some(1));
        // A body the ray goes through is no miss at all, and is first.
        let through = [player(1.2, 0.05), player(2.2, -0.3)];
        let hit = first(None, 2.0, &through).unwrap();
        assert_eq!((hit.index, hit.miss), (1, 0.0));
    }

    #[test]
    fn small_bodies_get_a_players_margin_and_big_ones_a_bounded_one() {
        let wide = |width: f64| Body {
            lo: [0.0, 0.0, 0.0],
            hi: [width, 1.0, width],
        };
        for (width, at_two) in [(0.2, 0.3), (0.6, 0.3), (0.9, 0.45), (1.4, 0.6), (4.0, 0.6)] {
            assert!((margin(2.0, &wide(width)) - at_two).abs() < 1e-9, "{width}");
        }
        assert!((margin(1.5, &wide(0.6)) - 0.15).abs() < 1e-6);
        // The range's end: a player's zone is 0.375 of a block past its box.
        assert!((margin(SCALE_RANGE[1], &wide(0.6)) - 0.375).abs() < 1e-6);
        assert_eq!(margin(1.0, &wide(0.6)), 0.0);
        assert_eq!(margin(f32::NAN, &wide(0.6)), 0.0);
        // A box that is not square is as wide as its wider side.
        let long = Body {
            lo: [0.0, 0.0, 0.0],
            hi: [0.4, 1.0, 1.0],
        };
        assert!((margin(2.0, &long) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_ray_along_an_axis_or_of_no_length_is_worked_out_all_the_same() {
        // Straight down onto a creature's zone from above.
        let below = Body {
            lo: [0.2, -2.0, -0.3],
            hi: [0.8, -1.0, 0.3],
        };
        let down = wide_hits([0.0; 3], [0.0, -1.0, 0.0], REACH, None, 2.0, &[below]);
        assert_eq!(down.len(), 1);
        assert!((down[0].miss - 0.2).abs() < 1e-6);
        // No direction at all: only a zone the eyes are in.
        let none = wide_hits([0.0; 3], [0.0; 3], REACH, None, 2.0, &[below]);
        assert!(none.is_empty());
        let around = Body {
            lo: [0.1, -0.5, -0.3],
            hi: [0.7, 0.5, 0.3],
        };
        let inside = wide_hits([0.0; 3], [0.0; 3], REACH, None, 2.0, &[around]);
        assert_eq!(inside.len(), 1);
        assert!(
            inside
                .iter()
                .all(|h| h.miss.is_finite() && h.distance.is_finite())
        );
    }
}
