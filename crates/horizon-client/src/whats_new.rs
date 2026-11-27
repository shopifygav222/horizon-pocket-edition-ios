//! Horizon PE: what a release brought, said once on the title screen of the first start with it
//! (game/pocket/whats_new.rs), whether the app was updated or installed new.
//!
//! Only what a player notices or asked for: the features, and the faults reported in this
//! port's issues. Each line is one short sentence, so the card is read in a moment.

/// A release's notes.
#[derive(Debug, PartialEq, Eq)]
pub struct Notes {
    pub version: &'static str,
    /// How the touch controls are played now, said first: what a player must know to enjoy
    /// them. None where a release changed nothing about them.
    pub controls: &'static [&'static str],
    pub new: &'static [&'static str],
    pub fixed: &'static [&'static str],
}

const NOTES: &[Notes] = &[
    Notes {
        version: "1.1.1",
        // 1.1.0 came out the day before: its controls and its news are said again here, for
        // whoever went from 1.0.0 straight to this one.
        controls: &[
            "Hold Attack to keep hitting. No tapping: it hits each time your weapon is ready.",
            "Hold Use to keep placing. Blocks go the same way, so a bridge stays straight.",
            "Hitbox size: raise it and a near miss still hits. Options > Controls > Touch.",
        ],
        new: &[
            "Your own music keeps playing while you play.",
            "A much newer game engine, and a bigger hotbar you can slide along.",
            "A Catalog button for mods, shaders and packs. Mods are built into the app.",
        ],
        fixed: &[
            "Leaves and plants wave again with a shader pack.",
            "The mods button on the title screen follows a build to its end.",
        ],
    },
    NOTES_1_1_0,
];

const NOTES_1_1_0: Notes = Notes {
    version: "1.1.0",
    controls: &[],
    new: &[
        "The game engine is over 31,000 changes newer (Starling 0.8 to 0.9).",
        "Hold Attack on a monster: no tapping, it hits each time your weapon is ready.",
        "Hold Use with blocks: they all go the same way, so a bridge stays straight.",
        "A bigger hotbar you can slide along. Its size is in the Control Hub.",
        "A Catalog button on the title screen: mods, shaders and resource packs.",
        "Mods from the catalog are built into the app from Options > Mods.",
    ],
    fixed: &[
        "A mouse now works on iPhone and iPad.",
        "HDR can be turned on in the video settings.",
        "The catalog lists far more mods.",
        "The game no longer crashes at a grindstone.",
    ],
};

/// The notes of `version`, if it has any.
pub fn notes(version: &str) -> Option<&'static Notes> {
    NOTES.iter().find(|notes| notes.version == version)
}

/// The notes to show an app of `version`: its own, unless they were seen. `seen`: the version
/// whose notes the player last closed, as the app wrote it down.
pub fn due(version: &str, seen: Option<&str>) -> Option<&'static Notes> {
    notes(version).filter(|notes| seen.map(str::trim) != Some(notes.version))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notes_show_once_for_an_update_and_for_a_new_install() {
        let notes = due("1.1.0", None).expect("a new install");
        assert_eq!(notes.version, "1.1.0");
        assert!(due("1.1.0", Some("1.0.0")).is_some(), "an update");
        assert!(due("1.1.0", Some("1.1.0\n")).is_none(), "seen");
        assert!(due("1.0.0", None).is_none(), "a version without notes");
        assert!(due("", None).is_none());
        // Each release has its own: an app of 1.1.0 that updates reads 1.1.1's.
        assert_eq!(
            due("1.1.1", Some("1.1.0")).map(|n| n.version),
            Some("1.1.1")
        );
    }

    #[test]
    fn the_notes_are_short() {
        for notes in NOTES {
            assert!(!notes.new.is_empty() && !notes.fixed.is_empty());
            // A heading costs a line and a half: the card is no taller with three of them.
            let headings = if notes.controls.is_empty() { 2 } else { 3 };
            let lines = notes.controls.len() + notes.new.len() + notes.fixed.len();
            assert!(lines + 2 * headings <= 14, "a card, not a page");
            let all = notes.controls.iter().chain(notes.new).chain(notes.fixed);
            for line in all {
                assert!(line.chars().count() <= 80, "{line}");
                assert!(line.ends_with('.'), "{line}");
            }
        }
    }
}
