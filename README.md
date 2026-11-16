<!-- Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): written for RustyShelf
     from Boxshelf's README.md, which documents the features RustyShelf carries over. -->
<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="renderer/brand/rustyshelf-logo-dark.svg">
  <img src="renderer/brand/rustyshelf-logo-light.svg" alt="RustyShelf" width="520">
</picture>

**Your game library as a shelf you browse from the couch.**

**English** · [Русский](README.ru.md) · [Português](README.pt-BR.md) · [Français](README.fr.md) · [Türkçe](README.tr.md)

[![Licence: GPL v3](https://img.shields.io/badge/licence-GPL--3.0-blue)](LICENSE)
[![Platforms: Windows, Linux, Android](https://img.shields.io/badge/platforms-Windows%20%C2%B7%20Linux%20%C2%B7%20Android-informational)](#install)
[![Build/release check](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml/badge.svg)](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml)
[![Electron](https://img.shields.io/badge/app-Electron-47848F?logo=electron&logoColor=white)](docs/MANUAL.md#run-from-source)

</div>

Emulated console games, Steam and other PC games, and streaming apps all stand side by side as real boxes on a 3D shelf.
Pull one out, open the case, read the manual, watch the disc go in, and play, all with one controller.

## What it does

- **A 3D shelf of real boxes.** Every game stands in its console's own case, with front, spine and back. Flip it, lift it
  out, open it, take the disc out, read the manual.
- **One controller for everything.** Browse, start a game, pause it, save and load states, take a screenshot and come back
  to the shelf without touching a keyboard.
- **Console games through your emulators.** Dolphin, Cemu, Eden, PPSSPP, PCSX2, RPCS3, shadPS4, Vita3K, xemu, Xenia, Azahar,
  and RetroArch for the retro systems. RustyShelf finds them, or you point it at them.
- **PC games on the same shelf.** Steam, Epic Games, GOG Galaxy, Ubisoft Connect, the EA app, Xbox / Game Pass, and a folder
  where every sub-folder is a game. On Linux, Windows games run through Wine or Proton without Steam.
- **Box art and manuals found for you.** Fronts, backs, spines, disc labels, descriptions and manuals come from open
  databases. The Art Studio and the Manual Studio let you choose between versions, part by part.
- **A museum of your collection.** Every system on a wall of shelves, each case at its real size, with four kinds of
  lighting.
- **Extras.** RetroAchievements and Steam achievements, a phone companion that works as a remote, eighteen colour themes
  and your own, attract mode, menu music, and your own 3D console models.
- **Private.** No accounts, no ads, no telemetry. Everything it saves stays in its own folder.

The interface is in English, Brazilian Portuguese, French and Turkish.

## Install

Get the newest build from the [releases page](https://github.com/zearkreast/rustyshelf/releases), once a release is out.
Until then, [run it from source](docs/MANUAL.md#run-from-source).

| Platform | File | How |
|---|---|---|
| **Windows** | `RustyShelf-<version>-windows-portable.exe` | Put it in a folder of its own and double-click it. If Windows warns about an unknown publisher, choose **More info → Run anyway**. |
| **Linux** | `RustyShelf-<version>-linux.AppImage` | `chmod +x` it once, then start it. Settings are kept in `~/RustyShelf`. |
| **Android** | `RustyShelf-<version>-android.apk` | Open the file on the device and allow the install when Android asks. |

On first start, the setup wizard offers **Find automatically**: it looks for your emulators and game folders, you look over
each system, then choose **Open RustyShelf**.

## Controls

| Action | Keyboard | Xbox / PlayStation pad |
|---|---|---|
| Move along the shelf | ← → | D-pad / left stick |
| Switch shelf | ↑ ↓ | LT / RT |
| Play / open | Enter | A / Cross |
| Back | Esc | B / Circle |
| Flip the box | F | Y / Triangle |
| Details | I | X / Square |
| Inspect | V | Right stick click |
| Museum | M | Hold View / Back |
| Settings | S | Start |
| Home menu while a game runs | | Guide, or hold View / Back |

The [manual](docs/MANUAL.md#controls) has the full list.


## Documentation

The [manual](docs/MANUAL.md) covers everything in detail:

- [Your folders](docs/MANUAL.md#your-folders) and how games are found, with notes per system
- [On the shelf](docs/MANUAL.md#on-the-shelf): Inspect, Details, Home, sorting, the museum, attract mode
- [While a game runs](docs/MANUAL.md#while-a-game-runs): the Home menu, save states, each emulator's hotkeys
- [Box art and manuals](docs/MANUAL.md#box-art-and-manuals) and [PC games](docs/MANUAL.md#pc-games)
- [Settings](docs/MANUAL.md#settings) and [Extras](docs/MANUAL.md#extras): phone companion, achievements, your own 3D models
- [Privacy](docs/MANUAL.md#privacy): every service it contacts, when, and what it sends
- [For developers](docs/MANUAL.md#for-developers): tests and release builds
- [Troubleshooting](docs/MANUAL.md#troubleshooting)

Steam Deck notes are in [docs/STEAM-DECK.md](docs/STEAM-DECK.md).

## Where RustyShelf comes from

RustyShelf is a renamed fork of [(T)rustyShelf](https://github.com/GGStonks/t-rustyshelf) by GGStonks, which is in turn a
fork of [Boxshelf](https://github.com/sassiestrhihno/boxshelf-beta) by sassiestrhihno. Most of what RustyShelf does is
Boxshelf's work, by its author. What the forks added is tagged **[fork]** in the manual.

> **Want the original Boxshelf?** Download it from the [Boxshelf beta page](https://github.com/sassiestrhihno/boxshelf-beta/releases)
> and report its bugs and ideas in [that project's issues](https://github.com/sassiestrhihno/boxshelf-beta/issues/new/choose).

## Credits

**Box art and metadata:** [libretro thumbnails](https://thumbnails.libretro.com),
[LaunchBox Games Database](https://gamesdb.launchbox-app.com), [GameTDB](https://www.gametdb.com),
[Super Mario Wiki](https://www.mariowiki.com).
**Manuals:** [Internet Archive](https://archive.org) collections and [GamesDatabase.org](https://www.gamesdatabase.org).
Box art and manuals are © their publishers.
**Fonts:** SIL Open Font License (see `renderer\fonts\LICENSES`).
**PC games:** [legendary](https://github.com/derrod/legendary) and [umu-launcher](https://github.com/Open-Wine-Components/umu-launcher).
**Bundled libraries** and their licences: [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

**Contributors:**

- [TrapEmAll](https://github.com/TrapEmAll) made the Museum's left-and-right look-around with the side displays reachable
  from the wide view ([boxshelf-source pull request #4](https://github.com/sassiestrhihno/boxshelf-source/pull/4)), and the
  Retro TV's live progress for the commercial search and ad downloads
  ([pull request #5](https://github.com/sassiestrhihno/boxshelf-source/pull/5)). Both came into RustyShelf unchanged.
- [supalii](https://github.com/supalii) made, in [their Boxshelf fork](https://github.com/supalii/boxshelf-source), custom
  games' own settings pages with a Platform (Custom, PC or any console's shelf) and launch arguments, the Xbox One shelf for
  PC ports, automatic SteamGridDB posters for PC games without a cover, hiding the Open emulator and Surprise me tiles, and
  `docs/LOCALIZATION.md`. These came into RustyShelf on top of Boxshelf beta.10.
- The Linux start script's idea of finishing Electron's unpacking comes from wouldja's boxshelf-linux.

<!-- fork-survey credits: made by `node scripts/fork-survey.js --credits` from the commit history; edits here are replaced -->

Brought into RustyShelf from other people's forks (they are the authors or co-authors of these commits):

- [Gilang](https://github.com/gilangbros-hub): the Famicom Disk System shelf ([original](https://github.com/gilangbros-hub/boxshelf-source/commit/3eb635ba13bbe00ee53d71db4ee76e0cb71dc6cd))
- [Thanks2111](https://github.com/Thanks2111): Português (Brasil): the language is a choice now
- [gfadeyi](https://github.com/gfadeyi): the PowerA Switch pad, the cover loop when a game window vanishes, manual passes in the background, Steam's hashed store covers, and the Trophies screen with Steam achievements
- [Haseo](https://github.com/Haseosama): Français (Settings → Display → Language, in both apps), games folders found in a folder, the drives to search, and the museum with huge libraries
- [supalii](https://github.com/supalii): Custom games on any shelf, the Xbox One shelf, PC posters and hidden tiles (from supalii's fork)
- [TrapEmAll](https://github.com/TrapEmAll): Retro TV guide: the progress line no longer stops the guide drawing; Retro TV: live progress for the commercial search and ad downloads; Credit TrapEmAll for the Museum's look-around; Museum: look left and right, and the side displays reachable from the wide view

<!-- /fork-survey credits -->

## Licence

RustyShelf is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License
as published by the Free Software Foundation, version 3 ([LICENSE](LICENSE)). It comes with no warranty.

**Name and logo.** Boxshelf's author asks that forks use their own name and logo: the GPL covers the code, not the Boxshelf
name or logo, which remain Boxshelf's. That's why (T)rustyShelf and RustyShelf each have their own name, and RustyShelf
its own logo. If you release a fork or modified version of RustyShelf, please give it a different name and logo as well,
so people can tell them apart. Saying it's "based on RustyShelf" is welcome.
