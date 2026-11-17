<!-- Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): written for RustyShelf
     from Boxshelf's README.md, which documents the features RustyShelf carries over. -->
<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/brand/rustyshelf-logo-dark.svg">
  <img src="docs/brand/rustyshelf-logo-light.svg" alt="RustyShelf" width="520">
</picture>

**Your game library as a shelf you browse from the couch.**

**English** · [Русский](README.ru.md) · [Português](README.pt-BR.md) · [Français](README.fr.md) · [Türkçe](README.tr.md)

[![Licence: GPL v3](https://img.shields.io/badge/licence-GPL--3.0-blue)](LICENSE)
[![Platforms: Windows, Linux, Android](https://img.shields.io/badge/platforms-Windows%20%C2%B7%20Linux%20%C2%B7%20Android-informational)](#install)
[![Build/release check](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml/badge.svg)](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml)
[![Rust + Bevy](https://img.shields.io/badge/app-Rust%20%2B%20Bevy-CE422B?logo=rust&logoColor=white)](docs/MANUAL.md#run-from-source)

</div>

Emulated console games, Steam and other PC games all stand side by side as real boxes on a shelf, played with one
controller.

> **This branch is the native app, in Rust.** It's an alpha, not complete yet. The Electron app it is rewritten from (and
> its Android app, made with Capacitor) is on the [`archive/electron`](https://github.com/zearkreast/rustyshelf/tree/archive/electron)
> branch. Both read the same settings and data, so a setup made in either works in both. The Museum stays Electron-only.

## What it does

- **A shelf of real boxes.** Every game stands in its console's own case, with front, spine and back. Turn it round or
  hold it up and turn it freely.
- **One controller for everything.** Browse, start a game, open the Home menu over it and come back to the shelf without
  touching a keyboard.
- **Console games through your emulators.** RustyShelf finds them, or you point it at them.
- **PC games on the same shelf.** Steam, Epic Games, GOG Galaxy, Ubisoft Connect, the EA app, Xbox / Game Pass, and a folder
  where every sub-folder is a game.
- **Box art and manuals found for you**, with the Art Studio and the Manual Studio to choose between versions.
- **Extras.** RetroAchievements and Steam achievements, a phone companion that works as a remote, colour themes and your
  own, attract mode and menu music.
- **Private.** No accounts, no ads, no telemetry. Everything it saves stays in its own folder.

The manual lists [what works so far](docs/MANUAL.md#what-works) and what isn't in it yet.

## Install

Alphas for Windows and Android are on the [releases page](https://github.com/zearkreast/rustyshelf/releases). Or run it
from source:

```bash
cargo run -p rustyshelf --release -- --windowed
```

The [manual](docs/MANUAL.md#run-from-source) has the Android build and the [controls](docs/MANUAL.md#controls).

## Documentation

The [manual](docs/MANUAL.md) covers what works, how to run it from source, the controls, the tests and the release
builds.

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
**Fonts:** SIL Open Font License (see `crates/rustyshelf-core/assets/fonts`).
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
  the Electron app's localization guide. These came into RustyShelf on top of Boxshelf beta.10.
- [Zak](https://github.com/ZakAuMiel) made, in [their Boxshelf fork](https://github.com/ZakAuMiel/boxshelf-source), the
  shelf of games streamed from another PC through Moonlight with its Settings page, the Pastel Pop theme, a French
  translation, recomps on their console's shelf, and PC games' backs of the box from LaunchBox. These came into the
  native app on top of Boxshelf beta.14: Game streaming, Pastel Pop, their French merged into ours, Art Studio and
  manuals for a PC game on a console's shelf (it stays a PC game here), and backs of the box.
- The Electron app's Linux start script's idea of finishing Electron's unpacking comes from wouldja's boxshelf-linux.

<!-- fork-survey credits: made by `node scripts/fork-survey.js --credits` from the commit history; edits here are replaced -->

Brought into RustyShelf from other people's forks (they are the authors or co-authors of these commits):

- [Gilang](https://github.com/gilangbros-hub): the Famicom Disk System shelf ([original](https://github.com/gilangbros-hub/boxshelf-source/commit/3eb635ba13bbe00ee53d71db4ee76e0cb71dc6cd))
- [Thanks2111](https://github.com/Thanks2111): Português (Brasil): the language is a choice now
- [gfadeyi](https://github.com/gfadeyi): the PowerA Switch pad, the cover loop when a game window vanishes, manual passes in the background, Steam's hashed store covers, and the Trophies screen with Steam achievements
- [Haseo](https://github.com/Haseosama): Français (Settings → Display → Language, in both apps), games folders found in a folder, the drives to search, and the museum with huge libraries
- [supalii](https://github.com/supalii): Custom games on any shelf, the Xbox One shelf, PC posters and hidden tiles (from supalii's fork)
- [TrapEmAll](https://github.com/TrapEmAll): Retro TV guide: the progress line no longer stops the guide drawing; Retro TV: live progress for the commercial search and ad downloads; Credit TrapEmAll for the Museum's look-around; Museum: look left and right, and the side displays reachable from the wide view
- [ZakAuMiel](https://github.com/ZakAuMiel): Recomps go to their console's shelf by themselves; Backs of the box for PC games, from LaunchBox's database; PC games can have a back of the box; Recomps on a console's shelf get the full case, Art Studio and manuals; French translation, with a language setting; Theme: Pastel Pop; Settings: a Streaming page in Library; Streaming shelf: play games from another PC through Moonlight

<!-- /fork-survey credits -->

## Licence

RustyShelf is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License
as published by the Free Software Foundation, version 3 ([LICENSE](LICENSE)). It comes with no warranty.

**Name and logo.** Boxshelf's author asks that forks use their own name and logo: the GPL covers the code, not the Boxshelf
name or logo, which remain Boxshelf's. That's why (T)rustyShelf and RustyShelf each have their own name, and RustyShelf
its own logo. If you release a fork or modified version of RustyShelf, please give it a different name and logo as well,
so people can tell them apart. Saying it's "based on RustyShelf" is welcome.
