// game-module-cfg: target_os = "ios"
//! iOS: catalog mods (docs/ios.md). iOS runs no code its app did not ship with, so where Starling's
//! Android app loads a library a mod was built into (native_builds.rs), a mod here is compiled
//! into the app itself: a build of this app's release with the mods in, which the player installs
//! over this app.
//!
//! The Mods page's top (`ModList::native_game`, drawn as Android's) says which mods this app was
//! built with, and when the mods turned on are not all in it, builds the app with them: the mod
//! builds service (packaging/mod-builds-worker, `horizon_client::mod_builds`) starts the build on
//! GitHub for the player, who needs no account there. The build is followed while they play, and
//! the finished .ipa is downloaded and handed to the share sheet (SideStore, AltStore,
//! LiveContainer or any other signer), or saved to Files. The same mods asked for by anyone are
//! one build: when someone built them already, the .ipa is there at once.
//!
//! A build that ends says so in a toast wherever the player is, and the title screen keeps a
//! line about it under the account button until it is installed or dismissed, which opens the
//! Mods page. One that ended while the app was closed is caught up with at the next start.
use super::Game;
use horizon_client::hud::HudBatch;
use horizon_client::mod_builds::{self, Pending, State, mods_label};
use horizon_client::mod_list::{NOT_BUILT, NativeGame, NativeGameAct, NativePage, NativePart};
use horizon_client::{options_ui::Page, text::Text, toasts::SystemToast, ui};
use horizon_mod_package::index::native::Any;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Where the app was built from: an app of another repository than this port's own is not
/// updated from this port's releases (`official_build`).
const REPO: &str = match option_env!("HORIZON_IOS_REPO") {
    Some(repo) => repo,
    None => "shopifygav222/horizon-pocket-edition-ios",
};
/// The build being followed, in the private folder: it survives a restart of the app.
const PENDING_FILE: &str = "mod-build.json";
/// The number this device counts its builds by (`mod_builds::device`).
const DEVICE_FILE: &str = "mod-builds-device";
/// What the program was built with, beside it: the edition's own mods (tools/ios_native_mods.py
/// `info`) and the catalog mods compiled in (tools/ios_native_sdk.py `add`).
const BUILD_INFO: [&str; 2] = ["native-mods.json", "catalog-mods.json"];
/// Where the finished .ipa is kept until it is installed: the private folder's.
const BUILDS_DIR: &str = "native-builds";
/// The largest download taken for an .ipa's artifact.
const LARGEST_ARTIFACT: u64 = 512 << 20;
/// How often a build under way is looked at.
const POLL: Duration = Duration::from_secs(20);
/// The app was away (in the background, or the phone asleep) when this long passed between two
/// frames, in seconds: a build under way is then looked at now, not at its next turn.
const AWAY_SECS: u64 = 3;
/// A build under way that no thread follows is followed again, this often at most (seconds).
const REVIVE_SECS: u64 = 30;
/// A build the service still doesn't know by then was not started.
const NEVER_STARTED: Duration = Duration::from_secs(5 * 60);

#[derive(Default)]
struct Shared {
    pending: Option<Pending>,
    /// How the build is going, in words.
    progress: String,
    /// A message for the player (and whether it is a warning), until the next action.
    message: Option<(String, bool)>,
    /// A toast to show once: the build ended (its title and line).
    announce: Option<(String, String)>,
    /// A thread follows the build.
    following: bool,
    /// The build is made and its file is being downloaded.
    fetching: bool,
    /// The thread is asked to look at the build now, not when its wait is over.
    nudge: bool,
    /// Bumped on every change, so the page is redrawn once per change.
    changes: u64,
}

#[derive(Default)]
pub(super) struct NativeBuilds {
    shared: Arc<Mutex<Shared>>,
    shown: u64,
    loaded: bool,
    /// When the last frame looked at the build (seconds since 1970).
    frame: u64,
    /// When a thread was last started for a build none followed.
    revived: u64,
}

fn shared(builds: &NativeBuilds) -> std::sync::MutexGuard<'_, Shared> {
    builds.shared.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn pending_path() -> std::path::PathBuf {
    horizon_client::logging::private_home().join(PENDING_FILE)
}

fn save_pending(pending: Option<&Pending>) {
    let path = pending_path();
    match pending {
        Some(p) => {
            if let Ok(text) = serde_json::to_string_pretty(p) {
                let partial = path.with_extension("partial");
                if std::fs::write(&partial, text).is_ok() {
                    let _ = std::fs::rename(partial, path);
                }
            }
        }
        None => {
            let _ = std::fs::remove_file(path);
        }
    }
}

fn load_pending() -> Option<Pending> {
    let text = std::fs::read_to_string(pending_path()).ok()?;
    serde_json::from_str(&text).ok()
}

/// A part of the Mods page's top: a section's title, what it says, and what needs doing.
fn title(game: &mut NativeGame, words: &str) {
    game.parts.push(NativePart::Title(words.to_owned()));
}
fn text(game: &mut NativeGame, words: String) {
    game.parts.push(NativePart::Text(words));
}
fn warning(game: &mut NativeGame, words: String) {
    game.parts.push(NativePart::Warning(words));
}

/// The toast of a build ready to install.
fn ready_toast() -> (String, String) {
    (
        "Mods build ready".to_owned(),
        "Install it from Options > Mods".to_owned(),
    )
}

fn builds_dir() -> std::path::PathBuf {
    horizon_client::logging::private_home().join(BUILDS_DIR)
}

/// The downloaded builds go: once installed or dismissed, nothing of them stays.
fn forget_builds() {
    let _ = std::fs::remove_dir_all(builds_dir());
}

/// The mods this app was built with: (id, version), from the lists beside its program.
pub(super) fn built_mods() -> Vec<(String, String)> {
    let Ok(exe) = std::env::current_exe() else {
        return Vec::new();
    };
    let mut built = BTreeMap::new();
    for name in BUILD_INFO {
        let info: Value = std::fs::read_to_string(exe.with_file_name(name))
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        for m in info["mods"].as_array().into_iter().flatten() {
            if let Some(id) = m["id"].as_str() {
                built.insert(
                    id.to_owned(),
                    m["version"].as_str().unwrap_or("?").to_owned(),
                );
            }
        }
    }
    built.into_iter().collect()
}

/// The catalog mods compiled into this app, as a build is asked for with them (`id@version`).
/// The edition's own mods are not among them: every build of the edition has those.
fn built_catalog_mods() -> BTreeMap<String, String> {
    let info: Value = std::env::current_exe()
        .ok()
        .and_then(|exe| std::fs::read_to_string(exe.with_file_name(BUILD_INFO[1])).ok())
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    info["mods"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| {
            Some((
                m["id"].as_str()?.to_owned(),
                m["version"].as_str()?.to_owned(),
            ))
        })
        .collect()
}

/// Whether this app is a release of this port's own repository as it came: not built in a
/// player's fork, and with no mods beyond its edition's. Only such an app is updated by
/// installing the next release over it (game/starling/update_offer.rs): a release has no catalog
/// mods.
pub(super) fn official_build() -> bool {
    REPO == horizon_client::ios_releases::REPOSITORY && built_catalog_mods().is_empty()
}

/// The catalog mods in the `horizon-native-mods` folder that are turned on, by id with their
/// version, and the ids of those there that no build takes: the older bundles, and the catalog's
/// mods that are not on the list of those a build takes (`mod_builds::buildable`: they change the
/// game's own source, or don't compile against this engine). The catalog leaves those off its
/// shelf, and a file comes into the folder other ways too.
fn folder_mods_on(off: &BTreeSet<String>) -> (BTreeMap<String, String>, Vec<String>) {
    let folder = Game::game_dir().join(horizon_client::mod_list::NATIVE_DIR);
    let mut on = BTreeMap::new();
    let mut untaken = Vec::new();
    let files = std::fs::read_dir(folder)
        .map(|d| d.flatten().map(|e| e.path()).collect::<Vec<_>>())
        .unwrap_or_default();
    for path in files {
        if !path
            .extension()
            .is_some_and(|e| e == "hnmod" || e == "hmod")
        {
            continue;
        }
        let Ok(found) = Any::read(&path) else {
            continue;
        };
        if off.contains(found.id()) {
            continue;
        }
        match &found {
            Any::Flat(_)
                if mod_builds::names_a_mod(found.id(), found.version())
                    && mod_builds::buildable_known().contains(found.id()) =>
            {
                on.insert(found.id().to_owned(), found.version().to_owned());
            }
            _ => untaken.push(found.id().to_owned()),
        }
    }
    untaken.sort();
    (on, untaken)
}

/// What the service answered at `path`, or why not, in the player's words. `body`: a request
/// to start a build.
fn ask(service: &str, path: &str, body: Option<&str>) -> Result<(String, State), String> {
    let url = format!("{service}{path}");
    let request = match body {
        Some(_) => ureq::post(&url),
        None => ureq::get(&url),
    }
    .timeout(Duration::from_secs(30))
    .set("User-Agent", "HorizonPE");
    let response = match body {
        Some(body) => request
            .set("Content-Type", "application/json")
            .send_string(body),
        None => request.call(),
    };
    match response {
        Ok(response) => {
            let text = response.into_string().map_err(|e| e.to_string())?;
            mod_builds::read_state(&text)
        }
        Err(ureq::Error::Status(code, response)) => {
            let said = response
                .into_string()
                .ok()
                .and_then(|text| mod_builds::refusal(&text));
            Err(said.unwrap_or_else(|| format!("The build service answered {code}.")))
        }
        Err(e) => Err(format!("The build service can't be reached: {e}")),
    }
}

/// Whether a refusal is the service's last word (a request it will refuse again), not a
/// moment's trouble reaching it.
fn refused_for_good(why: &str) -> bool {
    !why.starts_with("The build service can't be reached")
        && !why.starts_with("The build service answered 5")
        && !why.starts_with("GitHub did not")
}

/// Waits [`POLL`] between two looks at a build, or less when the app asks for a look now
/// ([`Shared::nudge`]: it came back to the front, where the build may long be made). By the
/// clock on the wall, which runs on while the phone sleeps.
fn wait(shared: &Mutex<Shared>) {
    let until = now() + POLL.as_secs();
    for _ in 0..POLL.as_secs() * 2 {
        std::thread::sleep(Duration::from_millis(500));
        let mut s = shared.lock().unwrap_or_else(PoisonError::into_inner);
        if std::mem::take(&mut s.nudge) || now() >= until {
            return;
        }
    }
}

/// No thread follows the build any more.
fn ended(shared: &Mutex<Shared>) {
    let mut s = shared.lock().unwrap_or_else(PoisonError::into_inner);
    s.following = false;
    s.fetching = false;
    s.changes += 1;
}

/// [`follow`] on a thread of its own. Whatever ends that thread, [`Shared::following`] says so:
/// a build under way that nothing follows is followed again (`poll_native_builds`), where it
/// would else say "Building mods..." for good.
fn spawn_follow(shared: Arc<Mutex<Shared>>, service: &'static str, device: String) {
    let theirs = shared.clone();
    let run = move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            follow(theirs.clone(), service, device);
        }));
        ended(&theirs);
    };
    if std::thread::Builder::new()
        .name("mod build".into())
        .spawn(run)
        .is_err()
    {
        ended(&shared);
    }
}

/// Asks the service for the build, then follows it to its .ipa, downloaded, keeping [`Shared`]
/// (and [`PENDING_FILE`]) up to date. Runs on its own thread.
fn follow(shared: Arc<Mutex<Shared>>, service: &'static str, device: String) {
    let set = |f: &dyn Fn(&mut Shared)| {
        let mut s = shared.lock().unwrap_or_else(PoisonError::into_inner);
        f(&mut s);
        s.changes += 1;
        save_pending(s.pending.as_ref());
    };
    let pending = || {
        shared
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending
            .clone()
    };
    let fail = |why: String| {
        set(&|s| {
            if let Some(p) = &mut s.pending {
                p.failed = Some(why.clone());
            }
            s.following = false;
            s.announce = Some((
                "Mods build failed".to_owned(),
                "See why in Options > Mods".to_owned(),
            ));
        })
    };
    let keyed = |key: &str, progress: String| {
        set(&|s| {
            if let Some(p) = &mut s.pending {
                p.key = Some(key.to_owned());
            }
            s.progress = progress.clone();
        })
    };
    loop {
        let Some(p) = pending() else {
            return set(&|s| s.following = false);
        };
        if !p.under_way() {
            return set(&|s| s.following = false);
        }
        // Asked for until the service knows the build; then only read.
        let answer = match &p.key {
            None => {
                let body = mod_builds::request(&p.tag, &p.mods, &device);
                ask(service, "/v1/build", Some(&body))
            }
            Some(key) => ask(service, &format!("/v1/build/{key}"), None),
        };
        let minutes = now().saturating_sub(p.asked) / 60;
        match answer {
            Ok((key, State::Ready { size })) => {
                keyed(&key, "Built. Downloading it.".to_owned());
                // Said on the title screen too: the build is made, and its file takes a while.
                set(&|s| s.fetching = true);
                let said = |progress: String| set(&|s| s.progress = progress.clone());
                match download(service, &key, size, &said) {
                    Ok(ipa) => {
                        return set(&|s| {
                            if let Some(p) = &mut s.pending {
                                p.ipa = Some(ipa.to_string_lossy().into_owned());
                            }
                            s.fetching = false;
                            s.following = false;
                            s.announce = Some(ready_toast());
                        });
                    }
                    Err(why) => set(&|s| {
                        s.fetching = false;
                        s.progress = format!("The download stopped ({why}): trying again.");
                    }),
                }
            }
            Ok((key, State::Queued)) => keyed(
                &key,
                format!("Waiting for one of GitHub's Macs ({minutes} min so far)."),
            ),
            Ok((key, State::Building { .. })) => keyed(
                &key,
                format!("Building: {minutes} min so far, usually 30 to 60."),
            ),
            Ok((_, State::Failed { reason })) => {
                return fail(format!(
                    "{reason} One of the mods may not fit this game's version: turn it off and \
                     build again."
                ));
            }
            // Known by its key and gone: asked for again, unless it never starts.
            Ok((_, State::None)) if now().saturating_sub(p.asked) > NEVER_STARTED.as_secs() => {
                return fail(
                    "The build was asked for but never started. Try again later.".to_owned(),
                );
            }
            Ok((_, State::None)) => set(&|s| {
                if let Some(p) = &mut s.pending {
                    p.key = None;
                }
                s.progress = "Asking for the build...".to_owned();
            }),
            Err(why) if refused_for_good(&why) => return fail(why),
            Err(why) => set(&|s| s.progress = format!("{why} Trying again.")),
        }
        wait(&shared);
    }
}

/// The build's artifact (`size` bytes, a zip holding the .ipa) downloaded through the service,
/// which answers with GitHub's own short-lived link, and its .ipa taken out into [`builds_dir`]
/// as `HorizonPE-mods-<key>.ipa`.
fn download(
    service: &str,
    key: &str,
    size: u64,
    said: &dyn Fn(String),
) -> Result<std::path::PathBuf, String> {
    use std::io::{Read, Write};
    if size > LARGEST_ARTIFACT {
        return Err(format!("{} MB is more than a build", size >> 20));
    }
    let dir = builds_dir();
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(30))
        .timeout_read(Duration::from_secs(60))
        .build();
    let response = agent
        .get(&format!("{service}/v1/build/{key}/ipa"))
        .set("User-Agent", "HorizonPE")
        .call()
        .map_err(|e| e.to_string())?;
    let zip_path = dir.join("build.zip");
    let mut file = std::fs::File::create(&zip_path).map_err(|e| e.to_string())?;
    let mut reader = response.into_reader().take(LARGEST_ARTIFACT + 1);
    let mut buffer = vec![0; 1 << 16];
    let (mut done, mut shown) = (0u64, 0u64);
    loop {
        let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        if done >> 20 != shown {
            shown = done >> 20;
            said(format!(
                "Built. Downloading it: {shown} of {} MB.",
                size.div_ceil(1 << 20).max(shown)
            ));
        }
    }
    file.sync_all().map_err(|e| e.to_string())?;
    drop(file);
    if done > LARGEST_ARTIFACT {
        return Err("more than a build".to_owned());
    }
    let ipa = dir.join(format!("HorizonPE-mods-{key}.ipa"));
    let archive = std::fs::File::open(&zip_path).map_err(|e| e.to_string())?;
    let mut archive = zip::ZipArchive::new(archive).map_err(|e| e.to_string())?;
    let mut found = None;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // A name that is not text is no .ipa of ours.
        let Ok(name) = entry.name() else { continue };
        if name.ends_with(".ipa") && !name.contains('/') {
            found = Some(i);
            break;
        }
    }
    let index = found.ok_or("the build holds no .ipa")?;
    let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
    let mut out = std::fs::File::create(&ipa).map_err(|e| e.to_string())?;
    std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
    out.sync_all().map_err(|e| e.to_string())?;
    let _ = std::fs::remove_file(&zip_path);
    Ok(ipa)
}

impl Game<'_> {
    /// The catalog mods turned on in the folder that this app doesn't have yet are chosen for the
    /// next build (a box on their rows), and the page's top says what the app has and offers it.
    pub(super) fn offer_compile(&mut self) {
        for row in &mut self.settings.mods.installed {
            if row.problem.as_deref() == Some(NOT_BUILT) {
                row.native = Some(NativePage::default());
            }
        }
        self.settings.mods.native_game = self.native_game();
    }

    /// What a build would be made with: the catalog mods turned on in the folder (`wanted`), and
    /// with them the ones this app has that are still on (`build`, each `id@version`); and the
    /// mods in the folder no build takes.
    fn wanted_mods(&self) -> (BTreeSet<String>, Vec<String>, Vec<String>) {
        let off = &self.settings.mods.disabled;
        let (folder, untaken) = folder_mods_on(off);
        let wanted: BTreeSet<String> = folder.keys().cloned().collect();
        // The folder's version of a mod is the one asked for: it is the newer.
        let mut build = built_catalog_mods();
        build.retain(|id, _| !off.contains(id));
        build.extend(folder);
        let build = build
            .iter()
            .map(|(id, version)| mod_builds::named(id, version))
            .collect();
        (wanted, build, untaken)
    }

    /// The release a build is made of: the newest of this edition the update check found, else
    /// this app's own. A build with the mods is how an app that has some is updated.
    fn build_tag(&self) -> Option<String> {
        let version = self
            .newer_release_version()
            .or_else(|| crate::android::app_version().map(|(version, _)| version))?;
        Some(mod_builds::tag(
            horizon_client::ios_releases::EDITION,
            &version,
        ))
    }

    /// The Mods page's top: what this app was built with, then the build of the mods turned on
    /// that it lacks, from asking for it to the install.
    fn native_game(&self) -> Option<NativeGame> {
        let built = built_mods();
        let catalog = built_catalog_mods();
        let (wanted, build, untaken) = self.wanted_mods();
        let update = self.newer_release_version();
        let state = shared(&self.native_builds);
        let pending = state.pending.clone();
        if built.is_empty() && wanted.is_empty() && untaken.is_empty() && pending.is_none() {
            return None;
        }
        let mut game = NativeGame::default();
        let name = horizon_client::ios_releases::EDITION.name();
        let version = crate::android::app_version().map_or_else(String::new, |(v, _)| v);
        // Mods by the names the list below gives them.
        let listed = |ids: &[String]| {
            let titles: Vec<String> = ids
                .iter()
                .map(|id| {
                    let found = self.settings.mods.installed.iter().find(|m| m.id == *id);
                    found.map_or_else(|| id.clone(), |m| m.title.clone())
                })
                .collect();
            mods_label(&titles)
        };
        title(&mut game, "This app");
        text(
            &mut game,
            match built.len() {
                0 => format!("{name} {version}. No mods are built into it."),
                1 => format!("{name} {version}. 1 mod is built into it."),
                n => format!("{name} {version}. {n} mods are built into it."),
            },
        );
        if let Some(p) = &pending {
            let ids: Vec<String> = p
                .mods
                .iter()
                .map(|m| mod_builds::id_of(m).to_owned())
                .collect();
            let mods = listed(&ids);
            let of = p.tag.trim_start_matches('v');
            if let Some(why) = &p.failed {
                title(&mut game, "The build failed");
                warning(&mut game, why.clone());
                text(&mut game, format!("It was a build with {mods}."));
                game.buttons
                    .push(("Dismiss".to_owned(), NativeGameAct::PlayWithout));
            } else if p.ipa.is_some() {
                title(&mut game, "Your build is ready");
                text(&mut game, format!("{name} {of} with {mods}."));
                text(
                    &mut game,
                    "Tap Install... and pick the app you sideload with (SideStore, AltStore, \
                     LiveContainer). It installs over this app: your worlds, settings and \
                     accounts stay."
                        .to_owned(),
                );
                game.buttons
                    .push(("Install...".to_owned(), NativeGameAct::Install));
                game.buttons
                    .push(("Save to Files".to_owned(), NativeGameAct::SaveIpa));
                game.buttons
                    .push(("Dismiss".to_owned(), NativeGameAct::PlayWithout));
            } else {
                title(&mut game, "Building");
                text(&mut game, format!("{name} {of} with {mods}."));
                if !state.progress.is_empty() {
                    text(&mut game, state.progress.clone());
                }
                text(
                    &mut game,
                    "You don't have to wait here. Keep playing, or close the app: the build goes \
                     on without it, and the game tells you when it is ready."
                        .to_owned(),
                );
                game.buttons
                    .push(("Stop following".to_owned(), NativeGameAct::PlayWithout));
            }
        }
        let building = pending.as_ref().is_some_and(Pending::under_way);
        let have: BTreeSet<String> = catalog.keys().cloned().collect();
        let asked: BTreeSet<&str> = build.iter().map(|m| mod_builds::id_of(m)).collect();
        // A mod the app lacks, or has in another version than the folder's, is one to build.
        let to_build = |m: &&String| {
            let id = mod_builds::id_of(m);
            catalog
                .get(id)
                .is_none_or(|version| mod_builds::named(id, version) != **m)
        };
        let fresh: Vec<String> = build
            .iter()
            .filter(to_build)
            .map(|m| mod_builds::id_of(m).to_owned())
            .collect();
        let missing = !wanted.is_subset(&have) || !fresh.is_empty();
        let leftover: Vec<String> = have
            .iter()
            .filter(|id| !asked.contains(id.as_str()))
            .cloned()
            .collect();
        let ids: Vec<String> = asked.iter().map(|id| (*id).to_owned()).collect();
        if !untaken.is_empty() {
            let one = untaken.len() == 1;
            title(&mut game, "Can't be built");
            warning(
                &mut game,
                format!(
                    "{} can't be built into the app: {} in an older format, or {} the \
                     game's own code. Turn {} off, or use the catalog's version if it has one.",
                    listed(&untaken),
                    if one { "it is" } else { "they are" },
                    if one { "changes" } else { "change" },
                    if one { "it" } else { "them" }
                ),
            );
        }
        let to_do = missing || !leftover.is_empty();
        if building {
            // Said above.
        } else if to_do && build.is_empty() {
            title(&mut game, "Nothing to build");
            text(
                &mut game,
                format!(
                    "Every added mod is off, so none of them runs. To take them out of the app, \
                     install the normal {name} release."
                ),
            );
        } else if build.len() > mod_builds::MOST_MODS {
            title(&mut game, "Too many mods");
            warning(
                &mut game,
                format!(
                    "A build takes at most {} mods, and {} are turned on. Turn some off.",
                    mod_builds::MOST_MODS,
                    build.len()
                ),
            );
        } else if to_do || (update.is_some() && !build.is_empty()) {
            match (&update, missing) {
                (Some(new), _) => {
                    title(&mut game, "Update available");
                    text(
                        &mut game,
                        format!(
                            "{name} {new} is out. This app has mods built in, so it updates \
                             with a build: {new} with {}.",
                            listed(&ids)
                        ),
                    );
                }
                (None, true) => {
                    title(&mut game, "Build needed");
                    text(
                        &mut game,
                        format!("Not in this app yet: {}.", listed(&fresh)),
                    );
                    text(
                        &mut game,
                        "On iPhone and iPad a mod with code is built into the app. Tap Build. \
                         It takes 30 to 60 minutes, or no time if someone built the same mods \
                         before. You don't have to wait: keep playing, or close the app."
                            .to_owned(),
                    );
                }
                (None, false) => {
                    title(&mut game, "Build needed");
                    let (verb, them) = if leftover.len() == 1 {
                        ("is", "it")
                    } else {
                        ("are", "them")
                    };
                    text(
                        &mut game,
                        format!(
                            "{} {verb} off but still in this app. Tap Build to take {them} out.",
                            listed(&leftover)
                        ),
                    );
                }
            }
            if mod_builds::service().is_some() {
                game.buttons
                    .push(("Build".to_owned(), NativeGameAct::Compile));
            } else {
                warning(
                    &mut game,
                    "This copy of the app has no build service, so it can't build mods here. On \
                     a computer, the repository's mod-build workflow can."
                        .to_owned(),
                );
            }
        }
        if let Some((said, red)) = &state.message {
            if *red {
                warning(&mut game, said.clone());
            } else {
                text(&mut game, said.clone());
            }
        }
        Some(game)
    }

    fn refresh_native_game(&mut self) {
        self.settings.mods.native_game = self.native_game();
    }

    /// A catalog mod was turned on or off: the build it needs may be another.
    pub(super) fn native_mod_toggled(&mut self, i: usize) {
        if self
            .settings
            .mods
            .installed
            .get(i)
            .is_some_and(|m| m.native.is_some())
        {
            self.refresh_native_game();
        }
    }

    /// A button of the Mods page's top.
    pub(super) fn native_game_act(&mut self, act: NativeGameAct) {
        let ipa = shared(&self.native_builds)
            .pending
            .as_ref()
            .and_then(|p| p.ipa.clone())
            .map(std::path::PathBuf::from)
            .filter(|path| path.is_file());
        let mut message = None;
        match act {
            NativeGameAct::Compile => message = self.start_build(),
            NativeGameAct::PlayWithout => {
                let mut state = shared(&self.native_builds);
                state.pending = None;
                state.progress.clear();
                // The thread that follows it ends now, so another build can be asked for at once.
                state.nudge = true;
                state.changes += 1;
                save_pending(None);
                forget_builds();
            }
            NativeGameAct::Install | NativeGameAct::SaveIpa => {
                let opened = ipa.as_deref().is_some_and(|path| {
                    if act == NativeGameAct::Install {
                        horizon_gamecontroller::share(path)
                    } else {
                        horizon_gamecontroller::export(&[path.to_owned()])
                    }
                });
                if !opened {
                    message = Some((
                        "The .ipa couldn't be opened here: press Dismiss and build again."
                            .to_owned(),
                        true,
                    ));
                }
            }
            _ => {}
        }
        {
            let mut state = shared(&self.native_builds);
            state.message = message;
            state.changes += 1;
        }
        self.refresh_native_game();
    }

    /// Asks the service to build the app with the mods turned on, and follows the build. What
    /// to tell the player when it can't be asked.
    fn start_build(&mut self) -> Option<(String, bool)> {
        let (_, build, _) = self.wanted_mods();
        if build.is_empty() || build.len() > mod_builds::MOST_MODS {
            return None;
        }
        let Some(service) = mod_builds::service() else {
            return Some((
                "This build of the app has no build service to ask.".to_owned(),
                true,
            ));
        };
        let Some(tag) = self.build_tag() else {
            return Some((
                "The app can't tell its own version, so it can't ask for a build of it.".to_owned(),
                true,
            ));
        };
        let device = mod_builds::device(&horizon_client::logging::private_home().join(DEVICE_FILE));
        let shared_state = self.native_builds.shared.clone();
        {
            let mut state = shared(&self.native_builds);
            if state.following {
                return None;
            }
            state.pending = Some(Pending {
                tag,
                mods: build,
                asked: now(),
                ..Pending::default()
            });
            state.progress = "Asking for the build...".to_owned();
            state.message = None;
            state.following = true;
            state.changes += 1;
            save_pending(state.pending.as_ref());
        }
        forget_builds();
        spawn_follow(shared_state, service, device);
        None
    }

    /// The title screen's line about a build, and where it is, on the title's main page: along the
    /// top, between the account button (4..124) and the touch layout's Keys, Mouse and Back
    /// (from 171 short of the right edge); on a screen too narrow for that, under the touch pad's
    /// row (which the account button pushes down to 27..47). None without a build to tell about.
    fn build_badge(&self) -> Option<(Text, [f32; 4])> {
        let on_title = self
            .title
            .as_ref()
            .is_some_and(|t| matches!(t.mode, horizon_client::title::Mode::Main))
            && !matches!(self.screen, super::Screen::Options | super::Screen::Win);
        if !on_title {
            return None;
        }
        let state = shared(&self.native_builds);
        let p = state.pending.as_ref()?;
        let text = if p.failed.is_some() {
            "Mods build failed"
        } else if p.ipa.is_some() {
            "Mods ready: install"
        } else if state.fetching {
            "Downloading mods..."
        } else {
            "Building mods..."
        };
        let width = (HudBatch::text_width(&self.gui.atlas, text) + 16.0).ceil();
        let (gui_w, _) = self.gui_size();
        let x = gui_w - 175.0 - width;
        let rect = if x >= 128.0 {
            [x, 3.0, width, 20.0]
        } else {
            [4.0, 51.0, width, 20.0]
        };
        Some((Text::literal(text.to_owned()), rect))
    }

    pub(super) fn draw_build_badge(&self, batch: &mut HudBatch) {
        let Some((label, [x, y, w, h])) = self.build_badge() else {
            return;
        };
        let hover = ui::hovered(self.cursor, x, y, w, h);
        ui::button(batch, &self.gui.atlas, x, y, w, h, &label, true, hover);
    }

    pub(super) fn build_badge_at(&self, cursor: (f32, f32)) -> bool {
        self.build_badge()
            .is_some_and(|(_, [x, y, w, h])| ui::hovered(cursor, x, y, w, h))
    }

    /// The badge opens the Mods page, whose top has the build.
    pub(super) fn press_build_badge(&mut self) {
        self.audio.play_button_click();
        shared(&self.native_builds).nudge = true;
        self.open_options_at(true, Page::Mods);
    }

    /// Every frame: the build asked for before the app last closed is followed again, and the
    /// page's top is redrawn when the build moved on.
    pub(super) fn poll_native_builds(&mut self) {
        if !self.native_builds.loaded {
            self.native_builds.loaded = true;
            let mut pending = load_pending();
            // The app now running is that build's: done.
            let have: Vec<String> = built_catalog_mods()
                .iter()
                .map(|(id, version)| mod_builds::named(id, version))
                .collect();
            let this = crate::android::app_version().map(|(version, _)| {
                mod_builds::tag(horizon_client::ios_releases::EDITION, &version)
            });
            if pending
                .as_ref()
                .is_some_and(|p| p.mods == have && Some(&p.tag) == this.as_ref())
            {
                pending = None;
                save_pending(None);
                forget_builds();
            }
            // A downloaded .ipa that went (iOS clears caches; it is not one) comes again.
            if let Some(p) = &mut pending
                && p.ipa
                    .as_ref()
                    .is_some_and(|f| !std::path::Path::new(f).is_file())
            {
                p.ipa = None;
            }
            // A build that waits to be installed says so again at each start. One under way is
            // followed again just below, as any that nothing follows.
            let waiting = pending.as_ref().is_some_and(|p| p.ipa.is_some());
            let mut state = shared(&self.native_builds);
            state.pending = pending;
            state.following = false;
            state.announce = waiting.then(ready_toast);
            state.changes += 1;
        }
        // The app was away: a build under way is looked at now. And one that nothing follows
        // (the app just started, or its thread ended some way it should not have) is followed.
        let at = now();
        let away = at.saturating_sub(self.native_builds.frame) >= AWAY_SECS;
        self.native_builds.frame = at;
        let lost = {
            let mut state = shared(&self.native_builds);
            if away && state.following {
                state.nudge = true;
            }
            !state.following && state.pending.as_ref().is_some_and(Pending::under_way)
        };
        if lost
            && at.saturating_sub(self.native_builds.revived) >= REVIVE_SECS
            && let Some(service) = mod_builds::service()
        {
            self.native_builds.revived = at;
            {
                let mut state = shared(&self.native_builds);
                state.following = true;
                state.nudge = false;
                state.changes += 1;
            }
            let device =
                mod_builds::device(&horizon_client::logging::private_home().join(DEVICE_FILE));
            spawn_follow(self.native_builds.shared.clone(), service, device);
        }
        let announce = shared(&self.native_builds).announce.take();
        if let Some((title, line)) = announce {
            self.server_packs.notice =
                Some(SystemToast::new(title.into(), Some(line.into())).with_display_time(10_000.0));
        }
        let changes = shared(&self.native_builds).changes;
        if changes != self.native_builds.shown {
            self.native_builds.shown = changes;
            if self.settings.mods.native_game.is_some() || self.native_game().is_some() {
                self.refresh_native_game();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_refusal_ends_a_build() {
        assert!(refused_for_good("This device has started 6 builds today."));
        assert!(refused_for_good(
            "This app's version has no builds with mods."
        ));
        assert!(!refused_for_good(
            "The build service can't be reached: timed out"
        ));
        assert!(!refused_for_good("The build service answered 502."));
        assert!(!refused_for_good("GitHub did not start the build (500)."));
    }
}
