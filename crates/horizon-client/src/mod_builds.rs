//! Horizon PE's mod builds, the app's side (game/pocket/native_builds_ios.rs draws and follows
//! them): what is asked of the mod builds service (packaging/mod-builds-worker) and what it
//! answers.
//!
//! An iPhone runs no code its app did not ship with, so a catalog mod is compiled into the app:
//! the service starts that build on GitHub for the player, who needs no account there, and hands
//! the finished .ipa over. A build is asked for with the release it is of (this app's edition and
//! version, as its tag), the mods (`id@version`, as the catalog names them) and a number this
//! device made up, which only counts how many builds it started today.
use crate::ios_releases::Edition;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// The service this app was built to ask (`HORIZON_MOD_BUILDS_URL` when the app was built, a
/// variable of the repository it was built in): none in a build made without one.
const BUILT_WITH: Option<&str> = option_env!("HORIZON_MOD_BUILDS_URL");

/// The catalog's native mods this engine builds into the app, as the catalog-mods-check
/// workflow found them when the app was built (packaging/ios/catalog-buildable.json): each
/// compiled against this source's mod API.
const BUILDABLE: &str = include_str!("../../../packaging/ios/catalog-buildable.json");

/// The ids of the list `text` for the engine `engine`; None when it is not a list or is another
/// engine's.
fn buildable_in(
    text: &str,
    engine: Option<&str>,
) -> Option<(String, std::collections::BTreeSet<String>)> {
    let list: serde_json::Value = serde_json::from_str(text).ok()?;
    let of = list["engine"].as_str()?;
    if engine.is_some_and(|engine| engine != of) {
        return None;
    }
    let mods = list["mods"]
        .as_object()?
        .keys()
        .filter(|id| names_a_mod(id, "0"))
        .cloned()
        .collect();
    Some((of.to_owned(), mods))
}

/// The catalog's native mods a build of this app can take, by id: the list the app was built
/// with, or the repository's newer one for the same engine (mods published since), asked for
/// once. It blocks: the catalog asks from its search thread.
pub fn buildable() -> &'static std::collections::BTreeSet<String> {
    LIST.get_or_init(|| {
        let (engine, built_with) = buildable_in(BUILDABLE, None).unwrap_or_default();
        let url = format!(
            "https://raw.githubusercontent.com/{}/main/packaging/ios/catalog-buildable.json",
            crate::ios_releases::REPOSITORY
        );
        ureq::AgentBuilder::new()
            .timeout_connect(std::time::Duration::from_secs(10))
            .timeout_read(std::time::Duration::from_secs(20))
            .build()
            .get(&url)
            .call()
            .ok()
            .and_then(|response| response.into_string().ok())
            .and_then(|text| buildable_in(&text, Some(&engine)))
            .map_or(built_with, |(_, newer)| newer)
    })
}

/// [`buildable`]'s list, once it was asked for.
static LIST: std::sync::OnceLock<std::collections::BTreeSet<String>> = std::sync::OnceLock::new();

/// [`buildable`] without the network, for what is drawn: the list the catalog asked for, if it
/// has, else the one the app was built with.
pub fn buildable_known() -> &'static std::collections::BTreeSet<String> {
    static OWN: std::sync::OnceLock<std::collections::BTreeSet<String>> =
        std::sync::OnceLock::new();
    LIST.get()
        .unwrap_or_else(|| OWN.get_or_init(|| buildable_in(BUILDABLE, None).unwrap_or_default().1))
}

/// The most mods a build takes (the service's own limit is its to change; this is the app's).
pub const MOST_MODS: usize = 24;

/// The service's address, without a slash at its end; None when this build has none.
pub fn service() -> Option<&'static str> {
    address(BUILT_WITH)
}

fn address(built_with: Option<&'static str>) -> Option<&'static str> {
    let url = built_with?.trim().trim_end_matches('/');
    (url.starts_with("https://") && url.len() > "https://".len()).then_some(url)
}

/// The release tag a build of `edition` at `version` is made at (`v1.1.0-shared-worlds`).
pub fn tag(edition: Edition, version: &str) -> String {
    format!("v{version}{}", edition.tag_suffix())
}

/// Whether `id` and `version` name a mod as the service takes it (its `MOD`): a build is not
/// asked for with anything else, which it would refuse whole.
pub fn names_a_mod(id: &str, version: &str) -> bool {
    let id_ok = (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        && !id.starts_with('_');
    let version_ok = (1..=32).contains(&version.len())
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'_' | b'-'))
        && version.as_bytes()[0].is_ascii_alphanumeric();
    id_ok && version_ok
}

/// `id@version`, as a request names a mod.
pub fn named(id: &str, version: &str) -> String {
    format!("{id}@{version}")
}

/// The id of a request's mod.
pub fn id_of(named: &str) -> &str {
    named.split_once('@').map_or(named, |(id, _)| id)
}

/// The body of the request for a build.
pub fn request(tag: &str, mods: &[String], device: &str) -> String {
    serde_json::json!({ "tag": tag, "mods": mods, "device": device }).to_string()
}

/// Where a build stands, as the service says it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Built, and its file is there to fetch (its size in bytes).
    Ready { size: u64 },
    /// Asked for; GitHub has not started on it.
    Queued,
    /// Being built since then (GitHub's time, as it gave it).
    Building { since: String },
    /// It failed, and where.
    Failed { reason: String },
    /// The service knows no such build (never asked for, or its file has expired).
    None,
}

/// The service's answer about a build: its key and its state. Err when the answer is not one.
pub fn read_state(text: &str) -> Result<(String, State), String> {
    let answer: serde_json::Value =
        serde_json::from_str(text).map_err(|_| "The build service's answer was not one.")?;
    let key = answer["key"]
        .as_str()
        .filter(|key| key.len() == 24 && key.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("The build service's answer names no build.")?;
    let state = match answer["state"].as_str().unwrap_or_default() {
        "ready" => State::Ready {
            size: answer["size"].as_u64().unwrap_or_default(),
        },
        "queued" => State::Queued,
        "building" => State::Building {
            since: answer["since"].as_str().unwrap_or_default().to_owned(),
        },
        "failed" => State::Failed {
            reason: answer["reason"]
                .as_str()
                .filter(|reason| !reason.is_empty())
                .unwrap_or("The build failed.")
                .to_owned(),
        },
        "none" => State::None,
        other => {
            return Err(format!(
                "The build service said {other:?}, which this app does not know."
            ));
        }
    };
    Ok((key.to_owned(), state))
}

/// Why the service refused, from its answer's body (`{"error": ...}`); None when it gave none.
pub fn refusal(text: &str) -> Option<String> {
    let answer: serde_json::Value = serde_json::from_str(text).ok()?;
    answer["error"]
        .as_str()
        .filter(|why| !why.is_empty())
        .map(str::to_owned)
}

/// The number this device counts its builds by, kept in `path`: 32 hex digits made up once. It
/// says nothing of the device or its player.
pub fn device(path: &Path) -> String {
    if let Ok(kept) = std::fs::read_to_string(path) {
        let kept = kept.trim();
        if kept.len() == 32 && kept.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return kept.to_owned();
        }
    }
    use std::hash::{BuildHasher, Hasher};
    // Two of the standard library's randomly keyed hashers: 128 bits of the system's randomness.
    let half = || {
        std::collections::hash_map::RandomState::new()
            .build_hasher()
            .finish()
    };
    let made = format!("{:016x}{:016x}", half(), half());
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, &made);
    made
}

/// A build the player asked for, as the app keeps it between starts.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    /// The release it is of.
    pub tag: String,
    /// Its mods (`id@version`), sorted.
    pub mods: Vec<String>,
    /// When it was asked for (seconds since 1970).
    pub asked: u64,
    /// Its key, once the service gave it.
    #[serde(default)]
    pub key: Option<String>,
    /// The finished .ipa, downloaded.
    #[serde(default)]
    pub ipa: Option<String>,
    #[serde(default)]
    pub failed: Option<String>,
}

impl Pending {
    /// Still being built or fetched.
    pub fn under_way(&self) -> bool {
        self.failed.is_none() && self.ipa.is_none()
    }
}

/// A list of mods in words: "a", "a and b", "a, b and c".
pub fn mods_label(mods: &[String]) -> String {
    match mods {
        [] => "no mods".to_owned(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_build_is_asked_for_at_its_editions_tag() {
        assert_eq!(tag(Edition::Standard, "1.1.0"), "v1.1.0");
        assert_eq!(tag(Edition::SharedWorlds, "1.1.0"), "v1.1.0-shared-worlds");
        let body = request("v1.1.0", &["zoom@1.0.0".into()], &"a".repeat(32));
        let read: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(read["tag"], "v1.1.0");
        assert_eq!(read["mods"][0], "zoom@1.0.0");
        assert_eq!(read["device"].as_str().unwrap().len(), 32);
    }

    #[test]
    fn only_what_the_service_takes_is_named() {
        assert!(names_a_mod("zoom", "1.0.0"));
        assert!(names_a_mod("coords_hud", "3.0.0-beta+1"));
        assert_eq!(named("zoom", "1.0.0"), "zoom@1.0.0");
        assert_eq!(id_of("zoom@1.0.0"), "zoom");
        for (id, version) in [
            ("", "1"),
            ("Zoom", "1"),
            ("zoom mod", "1"),
            ("_zoom", "1"),
            ("zoom", ""),
            ("zoom", "1 0"),
            ("zoom", ".1"),
            ("zoom", "1@2"),
        ] {
            assert!(!names_a_mod(id, version), "{id:?} {version:?}");
        }
        assert!(!names_a_mod(&"a".repeat(65), "1"));
        assert!(!names_a_mod("a", &"1".repeat(33)));
    }

    #[test]
    fn the_services_answers_are_read() {
        let key = "ba70882d6f53e8e4d6c2b19a";
        let read = |body: &str| read_state(&format!(r#"{{"key":"{key}",{body}}}"#));
        assert_eq!(
            read(r#""state":"ready","size":123"#),
            Ok((key.to_owned(), State::Ready { size: 123 }))
        );
        assert_eq!(read(r#""state":"queued""#).unwrap().1, State::Queued);
        assert_eq!(
            read(r#""state":"building","since":"2026-11-24T11:50:00Z""#)
                .unwrap()
                .1,
            State::Building {
                since: "2026-11-24T11:50:00Z".into()
            }
        );
        assert_eq!(
            read(r#""state":"failed","reason":"The build failed at: Build the game.""#)
                .unwrap()
                .1,
            State::Failed {
                reason: "The build failed at: Build the game.".into()
            }
        );
        assert_eq!(read(r#""state":"none""#).unwrap().1, State::None);
        // Not an answer, no key, a state this app does not know.
        assert!(read_state("<html>").is_err());
        assert!(read_state(r#"{"state":"ready"}"#).is_err());
        assert!(read_state(r#"{"key":"short","state":"ready"}"#).is_err());
        assert!(read(r#""state":"paused""#).is_err());
        assert_eq!(
            refusal(r#"{"error":"This device has started 6 builds today."}"#).as_deref(),
            Some("This device has started 6 builds today.")
        );
        assert_eq!(refusal("no"), None);
        assert_eq!(refusal(r#"{"error":""}"#), None);
    }

    #[test]
    fn a_device_keeps_the_number_it_made_up() {
        let dir = std::env::temp_dir().join(format!("horizon-mod-builds-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("private").join("mod-builds-device");
        let first = device(&path);
        assert_eq!(first.len(), 32);
        assert!(first.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(device(&path), first);
        // A file that is not one is made again.
        std::fs::write(&path, "me").unwrap();
        let again = device(&path);
        assert_eq!(again.len(), 32);
        assert_ne!(again, "me");
        assert_eq!(device(&path), again);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_buildable_list_is_this_engines_or_none() {
        let (engine, built_with) = buildable_in(BUILDABLE, None).expect("the list the app carries");
        // What is drawn knows the list without asking for it; the mods that change the game's own
        // source (in the catalog, and in no build) are not on it.
        assert_eq!(buildable_known(), &built_with);
        assert!(!built_with.contains("voice_chat") && !built_with.contains("freecam_plus"));
        assert!(!engine.is_empty());
        assert!(built_with.iter().all(|id| names_a_mod(id, "0")));
        let text =
            r#"{"engine":"starling-0.9.0","mods":{"zoom":"1.0.0","Bad Id":"1","gamma":"2"}}"#;
        let (of, mods) = buildable_in(text, Some("starling-0.9.0")).unwrap();
        assert_eq!(of, "starling-0.9.0");
        assert_eq!(mods.into_iter().collect::<Vec<_>>(), ["gamma", "zoom"]);
        // Another engine's list, and what is no list.
        assert!(buildable_in(text, Some("starling-0.10.0")).is_none());
        assert!(buildable_in("<html>", None).is_none());
        assert!(buildable_in(r#"{"engine":"x"}"#, None).is_none());
    }

    #[test]
    fn a_service_is_an_https_address_without_its_last_slash() {
        assert_eq!(address(None), None);
        assert_eq!(address(Some("")), None);
        assert_eq!(address(Some("http://builds.example")), None);
        assert_eq!(address(Some("https://")), None);
        assert_eq!(
            address(Some("https://builds.example/ ")),
            Some("https://builds.example")
        );
    }

    #[test]
    fn a_pending_build_reads_back_and_older_files_too() {
        let p = Pending {
            tag: "v1.1.0".into(),
            mods: vec!["zoom@1.0.0".into()],
            asked: 7,
            key: Some("k".into()),
            ..Pending::default()
        };
        assert!(p.under_way());
        let text = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<Pending>(&text).unwrap(), p);
        // 1.0.0's file (a build followed on GitHub with a token) is not one of these.
        assert!(serde_json::from_str::<Pending>(r#"{"mods":["zoom"],"asked":1,"run":5}"#).is_err());
        assert_eq!(mods_label(&[]), "no mods");
        assert_eq!(
            mods_label(&["a".into(), "b".into(), "c".into()]),
            "a, b and c"
        );
    }
}
