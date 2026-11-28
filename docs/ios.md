# Horizon Pocket Edition (iOS)

Horizon Pocket Edition ("Horizon PE" under its icon) is an unofficial iOS port of Horizon, Minecraft:
Java Edition rewritten in Rust, based on Starling 0.8.0, Horizon's Android port (see `IOS-SYNC.md` for
how it follows Starling). It has Starling's phone features: the same setup, title screen, control system, accounts, skins, catalog and
Diagnostics as the Android app. GitHub Actions builds it (`.github/workflows/ios.yml`); the IPA is
unsigned, so install it with a sideloading tool that signs it for your device.

It comes in two editions, both listed with their downloads in the README: **Horizon PE** (branch
`main`) and **Horizon PE: Shared Worlds** (branch `shared-worlds`), which adds shared worlds and
voice chat ([shared-worlds.md](shared-worlds.md)). Everything on this page is true of both.

What differs from Android, because iOS does not allow it or does it another way:

- **Native mods** can't be downloaded into the app and loaded later: iOS runs no code the app
  did not ship with. Instead they are compiled into the app itself (below).
- **Game files** are downloaded from Mojang and checked as on Android, then laid out from tables
  the app carries instead of being prepared with Java.
- **Your game folder** is the app's Documents: "Horizon PE" under On My iPhone in the Files app.
- **Updates** come as a new IPA. When a newer release of the app's own edition is out, a card
  over the title screen offers it (Update, Tomorrow, No): Update downloads the release's IPA,
  checks it against the SHA-256 GitHub gives, and opens the share sheet on it for the sideloading
  app, since iOS lets no app install another by itself. An app built in a fork, or with native
  mods, is told how to update there instead. There is no separate recovery window before the
  game starts.

## Catalog mods

A mod with code of its own changes what the game does, and iOS runs no code its app did not ship
with. So on iOS such a mod is compiled into the app: a build of the app with the mods you picked,
which you install over the one you have. Your worlds, settings and accounts stay.

From 1.1.0 the game asks for that build itself. You need no GitHub account, no fork and no token.

**In the game:**

1. Options > Mods > **Catalog**: the mods with code are listed with the content mods, marked
   "(native mod)". Install the ones you want. The catalog lists only the ones this version of the
   game is known to build (see "Which mods" below).
2. Back on the Mods page, its top says which mods this app has, and that the ones turned on aren't
   all in it. Press **Build**.
3. Keep playing. If someone already built the same mods for the same version of the game, the
   build is ready at once. If not, GitHub builds it (about 30 to 60 minutes) and the game follows
   it. When it is ready, or if it failed, a toast says so wherever you are, and the title screen
   shows a line under the account button until you install or dismiss it; tapping it opens the
   Mods page. If the app was closed when the build finished, it catches up the next time it starts.
4. Press **Install...**: the share sheet opens with the .ipa; pick **SideStore**, **AltStore**,
   **LiveContainer** or the app you sideload with (**Save to Files** keeps a copy instead). It
   installs over Horizon PE: the same app. Options > Mods lists the mods that are in it; each
   turns off there like any other, without building again. The downloaded .ipa is deleted once
   the new app runs, or when you press Dismiss.

A build has every catalog mod turned on in the folder, and keeps the mods this app already has
that are still on. A mod turned off stops at once; the Mods page then offers a build without it,
which takes it out of the app. With every catalog mod off, the normal release has none.

**Updating an app that has mods.** A release has no catalog mods, so it does not install over an
app that has some without taking them out. When a newer version is out, the Mods page says so and
**Build** makes the newer version with your mods.

**Who builds it.** The game asks the mod builds service (a small program the app's maker runs on
Cloudflare, `packaging/mod-builds-worker`), which starts this repository's `mod-build` workflow on
GitHub and hands the finished .ipa back. A build is named by the game's version and its mods, so
the same request from anyone is the same build, made once. The service is sent the version, the
mods and a random number your app made up once (it counts how many builds a device started today,
six at most); it keeps no account, no address and no list of who asked for what. GitHub keeps each
build for 90 days; after that, asking again builds it again. Builds are never published as
releases, but anyone signed in to GitHub can download a public repository's build files: they hold
the game and the mods picked, nothing else.

**Which mods.** A build takes the catalog's native mods that use only the game's mod API. A mod
that changes the game's own source (Horizon's older "mixin" format, and the newer mods with
`[native.mixin]`, `[native.bridge]` or `[native.module]` in their manifest) is not built from the
catalog, nor is one with a build script or a dependency that is not a public Rust crate. Of the
rest, the `catalog-mods-check` workflow compiles each against this version's mod API, and the
catalog on the phone lists only the ones that compile, and the Mods page and the build service
both refuse a mod that is not among them (`packaging/ios/catalog-buildable.json`). A content mod
is listed only if its package is content alone (`packaging/ios/catalog-content.json`, from the
`catalog-content-check` workflow): one that carries WebAssembly can't run in an iPhone app. So:
the catalog's mods are written for Horizon's newest version, and this port is on Starling 0.9.0's.
The Shared Worlds + Mods edition's fifteen built-in mods are in the older format and are part of
that edition's source (`native-mods/`): every build of it has them.

**What the build does.** It checks the request first (`tools/mod_build_key.py`: every mod is a
catalog mod's `id@version`), downloads each mod's package from the catalog's own GitHub releases
and checks its size and SHA-256, makes it a crate of the app's program
(`tools/ios_native_sdk.py`), compiles the app with it and puts the mod's package beside the
program, where the engine reads it. The build has no secret and may only read the repository.

**The mod builder.** Compiling the whole game for every build took 25 to 45 minutes of a Mac,
and GitHub lends this repository five Macs at a time: after a release, builds waited hours for
one. The `mod-builder` workflow compiles a release's game once and keeps what Cargo made of it
(the "compiled game": Cargo's target folder, the registry crates it read, the app bundle without
mods) as the artifact `game-base-<commit>-<recipe>`, for 90 days. Every later build of that
release takes it back and compiles the mods and the program that names them. It runs on a branch
and is told the release: the release's own source and tools are checked out at its tag, so a
release that is already out is built this way too.

- *What counts as unchanged.* Cargo goes by the dates of the files it read, and a fresh checkout
  dates every file now. The builder gives a file a fixed, old date only if its bytes are the ones
  the kept game was compiled from (`tools/ci/mod_builder.py` `snapshot`, `restore`); anything else
  keeps its own date and Cargo compiles again whatever read it, as in a build from nothing.
- *Which kept game is taken.* Only one this repository's own run of `mod-builder.yml` made,
  started by hand or by the service (never a pull request's), with the same compiler, Xcode and
  build settings. Without one, or with any doubt, the game is compiled from nothing and kept.
- *The one thing done differently.* `horizon-world`'s build script hashes `Cargo.lock` into the
  key of the snapshots the game keeps on the phone. Mods add their own packages to that file. When
  they change none of the game's own, the file is treated as the game's and that key stays the
  release's own (a build from nothing gets another key, and the phone makes its snapshots again);
  when a mod does change a package of the game's, Cargo is left to do everything a build from
  nothing does (`mod_builder.py` `lock`).
- *Its test.* `mod-builder-check` does it on Linux with the game's library: the same build from a
  kept game compiles nothing, with a mod it compiles the mod only, and with one line of the game
  changed it compiles the game again.
- *Which releases it builds* is `packaging/ios/mod-builder.json` on main, which the service reads:
  `{"ref": "main", "tags": ["v1.1.1", "v1.1.1-shared-worlds"]}`. Without the file, or for a
  release it does not name, a build is `mod-build`'s, at the release's tag, as before. Removing
  a tag (or the file) turns the builder off for it within minutes.
- *A trial.* **Actions > mod-builder > Run workflow** with a release's tag and mods builds
  `mod-trial-<key>`, which nothing hands to a player. Only the service asks for `serve`, which
  names the file `mod-build-<key>`, as it finds a build.

**What can go wrong.** "The build failed at: Build the game" (from the mod builder: "Build the
game with the mods") means a mod did not compile after
all (a new version of it since the list was made): turn it off and build again. "This device has
started 6 builds today": tomorrow there are more. "This build of the app has no build service to
ask": the app was built without one (a fork that did not set it up, below).

**Without the service** (a fork, or by hand): on GitHub, **Actions > mod-build > Run workflow**,
choose the release's tag under **Use workflow from**, and type the mods as
`id@version,id@version`. The .ipa is the run's artifact `mod-build-<key>`. A fork that wants the
in-game Build runs its own service: `packaging/mod-builds-worker/README.md` (ten minutes, free).

Catalog mods are not sandboxed: their code runs with the app's full access. Only the catalog's
reviewed mods can be built in.

## Installed packs and mods

Each catalog shelf (Mods, Resource packs, Shader packs) has an **Installed** button: it lists
everything in that shelf's folders, whether it came from the catalog, the Files app or an import,
with its size and where it came from, and the search box narrows it. **Remove** asks first, then
deletes it (through the catalog when the catalog installed it, so its list stays right). A
removed resource pack or shader pack leaves its page at once; a removed content mod leaves when
the game next starts. A native mod compiled into the app is different: removing it turns it off at
once, but it stays in the app until your next build (Options > Mods > **Build on GitHub**); the
question before removing it says so, and if you'd rather not rebuild, turn it off in Options > Mods
instead. One compiled in with no file left has **Turn off** instead of Remove. Each row shows the
project's icon (a resource pack added by hand shows its own `pack.png`).

## Device checklist

Install the IPA beside any other build, then, in order:

1. **First start:** the setup appears. Sign in (Microsoft, Ely.by, or an offline profile), let it
   download and lay out the game files, and reach the title screen.
2. **Second start:** the title screen comes straight up, with the account still picked.
3. **Single player:** create a world, walk, break and place blocks, open the inventory, leave and
   re-enter the world.
4. **Controls:** open the control hub (H), switch between MCPE and GoyControl, move a button in
   the editor and save, try the virtual mouse in a menu, and the gyroscope if you use it.
5. **Keyboard:** tap the chat, a sign and a search box: the keyboard opens and types; a tap beside
   the field puts it away. Name a control preset.
6. **Multiplayer:** join an online-mode server with a Microsoft account, and an Ely.by server
   with an Ely.by account.
7. **Skins:** Skin Customization shows the profile's skin; change it.
8. **Catalog:** from Resource Packs and Shader Packs, install one of each and turn it on.
9. **Files:** the Horizon PE folder shows in the Files app; open a resource pack zip with Horizon PE
   from Files.
10. **Diagnostics:** Options > Diagnostics, export a ZIP and share it: the Files sheet opens.
11. **Hardware:** a controller, and a keyboard and mouse if you have them.
12. **Endurance:** play 15 minutes in one world, send the app to the background and bring it back.

If anything fails, send the diagnostic ZIP (step 10) or `Horizon PE/logs` from the Files app.
