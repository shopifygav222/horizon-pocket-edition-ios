//! Horizon PE: "Hitbox size" (the Touch page; this port's issue 11). An attack from the touch
//! controls that just misses a player or a creature still lands on it. The rules, and their
//! tests, are `horizon_client::android_controls::wide_hits`'s; here is what only the game knows:
//! when the setting applies at all, which bodies there are, and whether one can be seen.
//!
//! It is asked only where an attack is decided (`remote_start_attack`, a tap's and a held
//! Attack button's choice of what they do), never for a use: trading, placing and mining beside
//! a creature stay as exact as they were. And only when the game's own pick found no entity, so
//! an exact hit is never changed.
//!
//! The server is told the entity, as for any hit. This game's own server and a vanilla one
//! check how far its box is, which this keeps to the game's own reach. A server that also checks
//! where the player looked may refuse the hit or punish it: on any server but the player's own
//! it is off unless they turned "Wider hitboxes on servers" on, under a warning that says so.
use super::Game;

impl Game<'_> {
    /// The creature an attack now lands on though the crosshair (or the finger) is on none:
    /// its index in `mobs`, and how far its box is from the eyes. None on a build without the
    /// touch controls.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(in crate::game) fn wide_attack_hit(&self) -> Option<(usize, f64)> {
        None
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub(in crate::game) fn wide_attack_hit(&self) -> Option<(usize, f64)> {
        use horizon_client::android_controls::wide_hits::{self, Body};
        let settings = &self.mobile.controls.settings;
        let scale = settings.hitbox_scale;
        // A mouse and a controller aim as they always did.
        if !wide_hits::on(scale) || !self.touch_shown() {
            return None;
        }
        // On another's server only when the player took that on themselves.
        if !self.has_singleplayer_server() && !settings.hitbox_on_servers {
            return None;
        }
        // A target a mod chose and a spear's stab are aimed by rules of their own.
        if self.targeting().entity.is_some()
            || self
                .held()
                .is_some_and(|s| horizon_client::spear::Kinetic::of(&s.item).is_some())
        {
            return None;
        }
        // What the ray is on before any block is hit as it is: the zone is for a miss only.
        if self.pick_entity_hit_at().is_some() {
            return None;
        }
        let (eye, dir) = self.aim()?;
        // The creatures an exact hit could be on (`mob_hit_within`), that are alive.
        let (indices, bodies): (Vec<usize>, Vec<Body>) = self
            .mobs
            .iter()
            .enumerate()
            .filter(|(_, m)| {
                !m.removed
                    && m.alive()
                    && Some(m.id) != self.riding
                    && Some(m.id) != self.remote_camera_id()
            })
            .map(|(i, m)| {
                let bounds = m.bounds();
                let (lo, hi) = (bounds.min_position(), bounds.max_position());
                let body = Body {
                    lo: [lo.x, lo.y, lo.z],
                    hi: [hi.x, hi.y, hi.z],
                };
                (i, body)
            })
            .unzip();
        wide_hits::wide_hits(
            eye,
            dir,
            self.entity_reach,
            self.block_distance(),
            scale,
            &bodies,
        )
        .into_iter()
        .find(|hit| {
            let body = &bodies[hit.index];
            let middle = std::array::from_fn(|axis| (body.lo[axis] + body.hi[axis]) / 2.0);
            // Not round a corner and not through a door: where the ray passed it, or its middle,
            // is in sight. And an item with a range of its own keeps to it.
            self.held_range_allows(hit.distance)
                && (self.in_sight(eye, hit.point) || self.in_sight(eye, middle))
        })
        .map(|hit| (indices[hit.index], hit.distance))
    }

    /// No block between the eyes and `point`, by the outlines the crosshair itself stops at.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    fn in_sight(&self, eye: [f64; 3], point: [f64; 3]) -> bool {
        // A body stands on the ground and against walls: a block met at the point itself is
        // the one it touches, not one in the way.
        const TOUCHING: f64 = 0.05;
        let to: [f64; 3] = std::array::from_fn(|axis| point[axis] - eye[axis]);
        let length = to.iter().map(|v| v * v).sum::<f64>().sqrt();
        if length <= TOUCHING {
            return true;
        }
        let dir = to.map(|v| v / length);
        self.clip_ray(eye, dir, length).is_none_or(|hit| {
            let met = (0..3)
                .map(|axis| (hit.point[axis] - eye[axis]).powi(2))
                .sum::<f64>()
                .sqrt();
            met > length - TOUCHING
        })
    }

    /// Whether an attack now lands on an entity: the one the crosshair (or the finger) is on,
    /// or with "Hitbox size" the creature it just misses. What a tap asks, to hit or to use.
    pub(in crate::game) fn attack_finds_entity(&self) -> bool {
        self.pick_entity_hit().is_some() || self.wide_attack_hit().is_some()
    }
}
