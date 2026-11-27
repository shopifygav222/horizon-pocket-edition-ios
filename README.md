<h1 align="center">Horizon Pocket Edition</h1>

<p align="center"><b>Minecraft: Java Edition 26.3 on iPhone and iPad.</b><br>Native. No Java, no emulation.</p>

<p align="center">
  <img src="https://img.shields.io/badge/Minecraft-Java%2026.3-5D9C3F" alt="Minecraft Java 26.3">
  <img src="https://img.shields.io/badge/iOS%20%26%20iPadOS-16%2B-1C1C1E?logo=apple&logoColor=white" alt="iOS and iPadOS 16 or newer">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-3B2F7E" alt="MIT license"></a>
</p>

<table>
  <tr>
    <td><img src="docs/media/sunset.jpg" alt="A sunset over a savanna"></td>
    <td><img src="docs/media/village.jpg" alt="A savanna village"></td>
    <td><img src="docs/media/underwater.jpg" alt="Under the sea"></td>
  </tr>
</table>

- **The real game.** Java Edition worlds, servers, LAN and redstone, as on a computer.
- **Made for touch.** Controls you can move and resize. Controllers, keyboards and mice work too.
- **Mods, shaders and resource packs** from a catalog inside the game.
- **Shared worlds and voice chat** with your friends, with no server to rent.

Horizon PE is a port of [Horizon](https://github.com/ludovic111/horizon), Minecraft's Java game
rewritten in Rust, based on [Starling](https://github.com/starling-client/starling), Horizon's
Android port.

> **Not an official Minecraft product.** Not approved by or associated with Mojang or Microsoft.
> The app contains no Minecraft code or assets; it downloads the game's files from Mojang on your
> device. Playing online needs your own copy of Minecraft: Java Edition.

## Contents

1. [Download](#1-download)
2. [Install](#2-install)
3. [Controls](#3-controls)
4. [Mods](#4-mods)
5. [Shader packs](#5-shader-packs)
6. [Shared worlds and voice chat](#6-shared-worlds-and-voice-chat)
7. [Help](#7-help)
8. [For developers](#8-for-developers)
9. [Credits and license](#9-credits-and-license)

## 1. Download

Pick one. They are the same game; each adds to the one before it.

| Edition | For you if | Download |
| --- | --- | --- |
| **Horizon PE** | You play alone, on servers, or with friends by invite code | [**1.1.0**](https://github.com/shopifygav222/horizon-pocket-edition-ios/releases/download/v1.1.0/HorizonPE-1.1.0.ipa) |
| **Shared Worlds** | Your group wants one world any of you can open, and voice chat in it | [**1.1.0**](https://github.com/shopifygav222/horizon-pocket-edition-ios/releases/download/v1.1.0-shared-worlds/HorizonPE-1.1.0-SharedWorlds.ipa) |
| **Shared Worlds + Mods** | You want that, plus 15 mods ready from the first start | [**1.1.0**](https://github.com/shopifygav222/horizon-pocket-edition-ios/releases/download/v1.1.0-shared-worlds-mods/HorizonPE-1.1.0-SharedWorlds-Mods.ipa) |

You can switch later: install another edition over the app and your worlds, settings and
accounts stay.

## 2. Install

**You need:** iOS 16 or newer, about 2 GB free, and a sideloading app
([SideStore](https://sidestore.io), [AltStore](https://altstore.io),
[Sideloadly](https://sideloadly.io) or
[LiveContainer](https://github.com/LiveContainer/LiveContainer)).

1. Download the `.ipa` from the table above.
2. Install it with your sideloading app.
3. Open it on Wi-Fi. The first start downloads the game's files.
4. Sign in with Microsoft or Ely.by, or play offline.

| | |
| --- | --- |
| **Updates** | The title screen shows a card when a new version is out. Tap **Update**. Nothing is lost |
| **Free Apple ID** | The app must be refreshed every 7 days. SideStore and AltStore do it for you |
| **Your files** | Files app > **On My iPhone > Horizon PE**: worlds are in `saves` |
| **Add a world, pack or skin** | Open its file with Horizon PE from Files or Safari |

## 3. Controls

<p align="center"><img src="docs/media/controls-mcpe.jpg" alt="The touch controls in a world"></p>

| Control | What it does |
| --- | --- |
| **Pad**, bottom left | Move |
| **Drag** the screen | Look around |
| **Sword** | Attack and break |
| **Crosshair button** | Use and place |
| **Arrows** | Jump, sneak, sprint. Sneak and sprint are tap on, tap off |
| **Hotbar** | Tap a slot, or slide along it and lift |
| **...** beside the hotbar | Inventory |
| **Top row** | Chat, pause, options, camera view, debug screen |
| **H**, top left | The Control Hub: every touch setting |

**Holding a button does more:**

- **Hold the sword** to keep mining, and to hit a monster again each time your weapon is ready.
- **Hold the crosshair button** with blocks to keep building. Every block goes on the same side
  as the first, so a bridge, wall or pillar stays straight.

**In the Control Hub** you can move, resize, recolour or hide any button, and change the button
size, the hotbar size, the look sensitivity and the left-handed layout.

**Hitbox size** (Options > Touch, from 1.1.1) makes fighting with a thumb easier: above 100%, an
attack that just misses a player or a creature still hits it. It is off until you raise it, and
your reach stays the same. It works in your own worlds. On a server it works only if you also turn
on **Wider hitboxes on servers**: a server can treat it as cheating and kick or ban you, and one
with anti-cheat may not count those hits at all. That risk is yours to take.

A controller, keyboard or mouse works as soon as you use it; the touch controls step aside.

## 4. Mods

Tap **Catalog** on the title screen. It lists mods, shader packs and resource packs. Tap a row to
read about it.

Every mod is one of two kinds:

| Kind | What it is | To add it |
| --- | --- | --- |
| **CONTENT** | New blocks, items and recipes | Tap **Install**. Done |
| **NATIVE** | A mod with its own code | Tap **Install**, then **Build** (below) |

### Building a native mod into the app

iOS only runs code that is part of an app, so the game rebuilds itself with your mods in it. You
need no computer and no account.

1. Install the mods you want, then close the catalog. The Mods page opens.
2. Tap **Build**. It takes 30 to 60 minutes, or no time if someone built the same mods before.
   You don't have to wait: keep playing, or close the app. The build goes on without it.
3. The game tells you when the build is ready, the next time you open it if it was closed. Open
   **Options > Mods** and tap **Install...**.
4. Pick your sideloading app. It installs over your app. Nothing is lost.

| | |
| --- | --- |
| **Turn a mod off** | **Options > Mods**, tap the mod. It stops at once |
| **Remove it from the app** | Turn it off, then **Build** again |
| **Update the app** | The Mods page says when a new version is out. Tap **Build** |
| **Limit** | 6 new builds a day for one device |

### The 15 mods of the Shared Worlds + Mods edition

They work from the first start. Change or turn off any of them in **Options > Mods**.

| Mod | What it does |
| --- | --- |
| **Waymap** | Waypoints and a minimap |
| **Zoom** | Zoom in |
| **Jade** | Says what you are looking at |
| **AppleSkin** | Shows how much hunger a food gives |
| **Armor Status** | Your armour and its durability |
| **Status Effect HUD** | Your potion effects and their time left |
| **Coordinates HUD** | Your position and the way you face |
| **TreeFall** | Cut the bottom log and the whole tree falls |
| **RightClickHarvest** | Tap a grown crop to harvest and replant it |
| **ShulkerPeek** | See inside a shulker box |
| **Enchantment Hints** | Says what each enchantment does |
| **BiomeCompass** | A compass that finds a biome |
| **SmartF3** | A tidier debug screen |
| **ContinuityHorizon** | Connected textures |
| **Better Dynamic Lights** | A held torch lights the way |

This edition adds three buttons: **zoom**, **waypoint list** and **add waypoint**.
[More about these mods](docs/shared-worlds-mods.md)

## 5. Shader packs

Iris and OptiFine shader packs work, such as Complementary, BSL, MakeUp and Photon.

1. Install one from the catalog, or put its `.zip` in `shaderpacks` in the Files app.
2. Turn it on from the **Shader Packs** screen in Options. This takes a few seconds.

**Starling Visuals** comes with the app and is the lightest. Big packs lower the frame rate and
use more battery.

## 6. Shared worlds and voice chat

*In the Shared Worlds editions.*

A shared world belongs to your whole group. Whoever opens it first hosts it on their own device.
When they leave, it moves to someone still playing. Up to 8 players.

**Set it up once.** One person does this, in about 10 minutes, on a phone:

1. Make a free [Cloudflare](https://dash.cloudflare.com) account and create an **R2** storage
   bucket. This is where the world's copy lives.
2. Create an **API token** that can read and write that bucket.
3. In the game, tap **Shared Worlds > Share a World...** and paste the bucket's details.
4. Tap **Copy World Invite** and send it to your friends. They tap **Add with Invite...**.

Only send the invite to friends you trust: it holds the key to the world.

| To | Do this |
| --- | --- |
| **Play** | Tap **Start** if nobody is playing, **Join** if someone is |
| **Stop hosting** | Save and Quit, and keep the app open until the upload finishes |
| **Load a backup** | Shared Worlds > the world > **Backups...** |
| **Talk** | Game Menu > **Friends** > **Voice Chat** > **On**, then tap the microphone button |

Voice chat and your microphone are both off until you turn them on.
[The full guide](docs/shared-worlds.md)

## 7. Help

| Problem | Fix |
| --- | --- |
| The `.ipa` is "corrupt" | Download it again in Safari. If its name ends in `.zip`, rename it to `.ipa`; don't unzip it |
| The app stops opening after a week | Refresh it in your sideloading app. A free Apple ID lasts 7 days |
| Low frame rate, or the device gets hot | Lower the render distance and turn the shader pack off |
| The touch buttons are a mess | Control Hub > Controls > presets > **Default** |
| A build of mods failed | One mod did not fit this version. Turn it off and build again |
| Something else | Options > Diagnostics > **Export diagnostic ZIP**, then [open an issue](https://github.com/shopifygav222/horizon-pocket-edition-ios/issues/new/choose) |

## 8. For developers

GitHub builds the app; no Mac is needed. Run the [`ios`](.github/workflows/ios.yml) workflow with
**Make and keep the unsigned .ipa**.

| Branch | Edition |
| --- | --- |
| [`main`](https://github.com/shopifygav222/horizon-pocket-edition-ios/tree/main) | Horizon PE |
| [`shared-worlds`](https://github.com/shopifygav222/horizon-pocket-edition-ios/tree/shared-worlds) | Shared Worlds |
| [`shared-worlds-mods`](https://github.com/shopifygav222/horizon-pocket-edition-ios/tree/shared-worlds-mods) | Shared Worlds + Mods |

- [docs/ios.md](docs/ios.md): how the iOS app works.
- [IOS-SYNC.md](IOS-SYNC.md): every change made to Starling, and how to take in a new Starling
  release.
- [packaging/mod-builds-worker](packaging/mod-builds-worker/README.md): the mod build service,
  for a fork of your own.

## 9. Credits and license

- **[Horizon](https://github.com/ludovic111/horizon)** by Ludovic and contributors: the game.
- **[Starling](https://github.com/starling-client/starling)** by
  [JuanLuci](https://github.com/JuanLuci) and [Vasysik](https://github.com/Vasysik): the Android
  port this app is based on. The world screenshots are Starling's.
- **[horizon-iOS](https://github.com/meehandaniel4/horizon-iOS)**: the first native mod builds
  for iOS.
- **[Opus](https://opus-codec.org)** and **[RNNoise](https://github.com/xiph/rnnoise)**: voice
  chat's codec and noise filter.
- **The built-in mods' authors:**
  [names, sources and licences](docs/shared-worlds-mods.md#the-mods-their-authors-and-licences).

MIT licensed, as Horizon and Starling are: [LICENSE](LICENSE) and
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
