//! The app's start, and the event loop's handler: the game, while one runs.
use std::path::{Path, PathBuf};
use winit::application::ApplicationHandler;
use winit::event::{DeviceEvent, DeviceId, StartCause, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::WindowId;

/// The stack every event-loop callback runs on, and how much of it must be left before one.
/// The desktop's and Android's main threads reserve 136 MiB from 0.9.0 (horizon-guest-stack, for
/// wasm mods, which iOS does not run); 64 MiB is address space only, used as far as the game goes
/// (built-in mods run on it), and the same as the integrated server's thread here.
const GAME_STACK: usize = 64 << 20;
const GAME_STACK_LEFT: usize = 8 << 20;

/// The mark that says this program is the game, kept in it as the desktop's main.rs keeps it
/// (horizon-official).
static GAME_IDENTITY: [u8; horizon_mod_package::game_identity::MARK.len()] =
    horizon_mod_package::game_identity::MARK;

/// What the desktop's mod host installs before the engine starts (horizon-mod-host's
/// `main_with`), in its order, as Starling's Android library does (android_lib.rs
/// `install_platform`): the engine's seal, the mods' instrumentation, the engine platform socket
/// (packages, registration data, parts, profiler) and the render points. Without these the game
/// stops at its first mod guard ("engine platform socket is not installed").
fn install_platform() -> Result<(), String> {
    horizon_engine_slots::install_seal(mod_points::__private::seal)
        .map_err(|error| format!("engine seal: {error}"))?;
    mod_platform::__private::initialize()
        .map_err(|error| format!("mod instrumentation is unavailable: {error}"))?;
    horizon_engine_slots::install_platform(horizon_engine_slots::Platform {
        install_packages: mod_platform::__private::install_packages,
        install_registration_data: mod_platform::__private::install_registration_data,
        install_parts: mod_platform::__private::install_parts,
        profiler_is_enabled: mod_platform::__private::profiler::is_enabled,
        profiler_start: mod_platform::__private::profiler::start,
        profiler_stop: mod_platform::__private::profiler::stop,
        profiler_summary: mod_platform::__private::profiler::summary,
        profiler_firefox_profile: mod_platform::__private::profiler::firefox_profile,
    })
    .map_err(|error| error.to_string())?;
    mod_render::render::install();
    Ok(())
}

fn on_game_stack<R>(callback: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(GAME_STACK_LEFT, GAME_STACK, callback)
}

/// The game folder (Documents) and the private one (Library/Application Support/Starling).
fn folders() -> (PathBuf, PathBuf) {
    let home = std::env::var_os("HOME").map_or_else(std::env::temp_dir, PathBuf::from);
    (
        home.join("Documents"),
        home.join("Library/Application Support/Starling"),
    )
}

/// Standard error, which iOS sends nowhere a player can read, into the game folder's
/// `logs/stderr.log` (the last run's kept as `previous-stderr.log`): what a crash leaves there (a
/// panic, a native abort's message) reaches a bug report.
fn capture_stderr(home: &Path) {
    use std::os::fd::AsRawFd;
    let dir = home.join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = std::fs::rename(dir.join("stderr.log"), dir.join("previous-stderr.log"));
    if let Ok(file) = std::fs::File::create(dir.join("stderr.log")) {
        // SAFETY: both descriptors are open; dup2 points standard error at the file, which stays
        // open through it once `file` closes its own descriptor.
        unsafe { libc::dup2(file.as_raw_fd(), libc::STDERR_FILENO) };
    }
}

/// The game's command line and environment, as Android's (android.rs `command_line`):
/// `--resources` in the private folder, then `horizon-args.txt` there, which may override it: one
/// argument per line, or `NAME=value` for an environment variable.
fn command_line(private: &Path) -> (Vec<String>, Vec<(String, String)>) {
    let mut args = vec![
        "--resources".to_owned(),
        private.join("resources").display().to_string(),
    ];
    let mut env = Vec::new();
    let text = std::fs::read_to_string(private.join("horizon-args.txt")).unwrap_or_default();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        match line.split_once('=') {
            Some((name, value))
                if !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') =>
            {
                env.push((name.to_owned(), value.to_owned()))
            }
            _ => args.push(line.to_owned()),
        }
    }
    (args, env)
}

/// The RAM limit Starling's governor keeps the game under (ram_limit.rs: far terrain off, then a
/// lower render distance, before the game would hold more), in MiB: 85% of what iOS lets this
/// app hold, which iOS enforces by ending the app without a word. Empty when iOS doesn't say.
fn ram_limit_mb() -> String {
    // What the app holds now (its start: little), and what iOS lets it take beyond that.
    let held = 64 << 20;
    horizon_gamecontroller::available_memory()
        .filter(|&available| available > 0)
        .map_or_else(String::new, |available| {
            (((available + held) / 100 * 85) >> 20).to_string()
        })
}

pub fn main() {
    std::hint::black_box(&GAME_IDENTITY);
    let (home, private) = folders();
    let _ = std::fs::create_dir_all(&home);
    let _ = std::fs::create_dir_all(&private);
    capture_stderr(&home);
    let (args, env) = command_line(&private);
    let ram_limit = ram_limit_mb();
    // SAFETY: no other thread of this program runs yet; the game reads these from its threads
    // afterwards.
    unsafe {
        std::env::set_var("HORIZON_HOME", &home);
        std::env::set_var("HORIZON_PRIVATE", &private);
        for (name, value) in &env {
            std::env::set_var(name, value);
        }
        // As Starling's Android app: no telemetry unless the test arguments ask, and no Discord
        // on a phone.
        for (name, value) in [
            ("HORIZON_TELEMETRY_URL", "off"),
            ("HORIZON_NO_DISCORD", "1"),
            ("HORIZON_MEMORY_LIMIT_MB", ram_limit.as_str()),
        ] {
            if std::env::var_os(name).is_none() && !value.is_empty() {
                std::env::set_var(name, value);
            }
        }
    }
    if let Err(e) = std::env::set_current_dir(&home) {
        eprintln!("Horizon: cannot enter {}: {e}", home.display());
    }
    // Before the game makes a sound: music from another app plays on beside it.
    if !horizon_gamecontroller::audio_session::play_beside_others() {
        eprintln!("Horizon: the audio session is not shared: other apps' sound will stop");
    }
    horizon_client::ios::install();
    horizon_protocol::install_system_srv_lookup(crate::dns::query_srv);
    horizon_client::ios::set_args(args);
    // Options > Diagnostics, as Android's: the session's checkpoints, from the start.
    if let Err(error) = horizon_client::diagnostics::initialize(
        &private,
        serde_json::json!({"abi": std::env::consts::ARCH, "system": "ios"}),
    ) {
        eprintln!("Horizon: could not initialize diagnostics: {error}");
    }
    // The engine's seal, the mods' instrumentation and platform socket, before anything
    // registers: once a process.
    if let Err(error) = install_platform() {
        eprintln!("Horizon: {error}");
        return;
    }
    // Play with Friends' joining side: without it no invite code joins, and the title's Friends
    // button stays off. The desktop's program registers it too (horizon-official).
    if let Err(error) = horizon_hooks::register(&horizon_friends::game::HOOKS) {
        eprintln!("Horizon: Play with Friends did not register: {error}");
    }
    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(e) => {
            eprintln!("Horizon: no event loop: {e}");
            return;
        }
    };
    let mut app = App {
        home,
        private,
        game: None,
        started: false,
        closing: false,
    };
    if let Err(e) = event_loop.run_app(&mut app) {
        eprintln!("Horizon: the event loop failed: {e}");
    }
}

/// The event loop's handler.
struct App {
    home: PathBuf,
    private: PathBuf,
    /// The game Starling's start made, while it runs.
    game: Option<Box<dyn ApplicationHandler>>,
    /// The first start happened (at the first `resumed`).
    started: bool,
    /// The game ended to start again, which takes a new process: the app closes once the
    /// notice that says so is answered.
    closing: bool,
}

impl App {
    /// Starling's start, as the desktop binary's `main` makes it, up to the game it hands over.
    fn start_game(&mut self, event_loop: &ActiveEventLoop) {
        let (home, private) = (self.home.clone(), self.private.clone());
        let game = on_game_stack(|| {
            horizon_client::entry::set_after_log(move || {
                log::info!(
                    "Horizon: iOS, game folder {}, private files {}, arguments {:?}",
                    home.display(),
                    private.display(),
                    horizon_client::ios::args()
                );
                // The Simulator's check of voice chat's calls into the system
                // (tools/ci/ios_simulator_smoke.sh sets this; no player's phone does).
                if std::env::var_os("HORIZON_VOICE_SELFTEST").is_some() {
                    for line in horizon_gamecontroller::voice::selftest() {
                        log::info!("Horizon voice self-test: {line}");
                    }
                }
                // Read from the app's bundle (`assets/`), as Android's from the APK.
                horizon_client::preparer::put_shader_pack(&home);
                // The saved Microsoft sign-in renews while the game loads.
                horizon_client::accounts::resume();
            });
            // The code mods are the ones this app was built with (iOS runs no code it did not
            // ship with), loaded as the desktop's mod host loads the ones beside its program.
            let (entries, slots, native) = crate::built_mods::built();
            horizon_client::entry::start(
                native,
                move |native, launch| horizon_mod_host::load_code(entries, &slots, native, launch),
                horizon_mod_host::code_runtime(),
                horizon_client::entry::prepare(),
            );
            horizon_client::ios::started_game()
        });
        let Some(mut game) = game else {
            let why = horizon_client::ios::take_failure()
                .unwrap_or_else(|| "the game ended before it started".to_owned());
            log::error!("Horizon: {why}");
            return;
        };
        on_game_stack(|| {
            game.new_events(event_loop, StartCause::Init);
            game.resumed(event_loop);
        });
        self.game = Some(game);
    }

    /// After each callback: a game that asked to end (Quit) or to start again ends here, saving
    /// what it holds, as the desktop's does when its loop returns. One that asked to end closes
    /// the app with it.
    fn after_callback(&mut self, event_loop: &ActiveEventLoop) {
        let exit = winit::platform::ios::take_exit_request();
        let restart = horizon_client::ios::take_restart();
        if !exit && !restart {
            return;
        }
        if let Some(mut game) = self.game.take() {
            on_game_stack(|| {
                game.exiting(event_loop);
                drop(game);
            });
        }
        // The game ended as it meant to: its hooks hear it, the recording stops and the
        // session is marked clean.
        horizon_client::ios::game_ended();
        horizon_client::performance::stop();
        if !restart {
            horizon_client::diagnostics::finish();
        }
        if restart {
            // The engine registers its mods and seals its tables once a process, so starting
            // again takes a new one, as Android's restart of its activity is. iOS starts no app
            // for another: the player opens it again, and the game goes on from what it left
            // for its next start (the world it was opening, game/starling/world_content.rs).
            log::info!("Horizon: the game ended to start again: asking to reopen the app");
            log::logger().flush();
            let name = horizon_client::APP_NAME;
            let shown = horizon_gamecontroller::notice(
                &horizon_client::lang::get_or("pocket.restart.title", "Open %s again")
                    .replace("%s", name),
                &horizon_client::lang::get_or(
                    "pocket.restart.message",
                    "Your game is saved. %s has to start again to go on, and iOS lets no app \
                     open itself: tap OK, then open %s again.",
                )
                .replace("%s", name),
                &horizon_client::lang::get_or("gui.ok", "OK"),
            );
            if !shown {
                std::process::exit(0);
            }
            self.closing = true;
        } else {
            // Quit Game: the game is over and saved, and iOS has no way for an app to ask to
            // be closed, so the process ends here. Without this the app stayed open on an
            // empty screen until it was swiped away.
            log::info!("Horizon: the game ended (Quit Game): closing the app");
            log::logger().flush();
            std::process::exit(0);
        }
    }

    /// `callback` with the game on its stack, then what it asked for.
    fn with_game(
        &mut self,
        event_loop: &ActiveEventLoop,
        callback: impl FnOnce(&mut dyn ApplicationHandler),
    ) {
        if let Some(game) = self.game.as_deref_mut() {
            on_game_stack(|| callback(game));
            self.after_callback(event_loop);
        }
    }
}

impl ApplicationHandler for App {
    fn new_events(&mut self, event_loop: &ActiveEventLoop, cause: StartCause) {
        self.with_game(event_loop, |game| game.new_events(event_loop, cause));
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if !self.started {
            self.started = true;
            // A 120 Hz screen runs an app at 60 unless the app asks for more.
            if let Some(rate) = horizon_gamecontroller::request_high_frame_rate() {
                log::info!("Horizon: asked iOS for {rate} frames a second");
            }
            self.start_game(event_loop);
            self.after_callback(event_loop);
            return;
        }
        self.with_game(event_loop, |game| game.resumed(event_loop));
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ()) {
        self.with_game(event_loop, |game| game.user_event(event_loop, event));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.with_game(event_loop, |game| game.window_event(event_loop, id, event));
    }

    fn device_event(&mut self, event_loop: &ActiveEventLoop, id: DeviceId, event: DeviceEvent) {
        self.with_game(event_loop, |game| game.device_event(event_loop, id, event));
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Files other apps handed this one ("Open in", the share sheet): into the import inbox,
        // which the game installs from.
        for file in winit::platform::ios::take_opened_files() {
            horizon_client::ios::put_in_inbox(&file.path);
        }
        if self.closing {
            // The notice was answered: the app closes, and its next start goes on.
            if !horizon_gamecontroller::presenting() {
                std::process::exit(0);
            }
            event_loop.set_control_flow(winit::event_loop::ControlFlow::WaitUntil(
                std::time::Instant::now() + std::time::Duration::from_millis(100),
            ));
            return;
        }
        self.with_game(event_loop, |game| game.about_to_wait(event_loop));
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.with_game(event_loop, |game| game.suspended(event_loop));
    }

    fn exiting(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(mut game) = self.game.take() {
            on_game_stack(|| game.exiting(event_loop));
        }
    }

    /// iOS is short of memory: the allocators give back every free page now, before iOS ends
    /// the app for what they keep.
    fn memory_warning(&mut self, event_loop: &ActiveEventLoop) {
        log::warn!("Horizon: iOS is short of memory: freeing what the game does not use");
        horizon_threads::release_free_memory();
        self.with_game(event_loop, |game| game.memory_warning(event_loop));
    }
}
