<!-- Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): this file is the same as Boxshelf's
     docs/STEAM-DECK.md apart from the app's name (RustyShelf). -->
# RustyShelf on the Steam Deck

RustyShelf's Linux build runs on the Steam Deck. It's experimental: tell us how it goes.

## Setup (once, in Desktop Mode)

1. Download `RustyShelf-…-linux.AppImage` and move it to `/home/deck/Applications`.
2. Right-click it, then Properties → Permissions → tick "Is executable".
3. Start it. RustyShelf finds EmuDeck's (and RetroDECK's) emulators and your ROMs by itself.
4. RustyShelf offers to **add itself to Steam**. Say yes: Steam closes and opens again while it's added. If you said "Not now", do it later in Settings → System → Add RustyShelf to Steam.

## Playing (Game Mode)

Start RustyShelf from your Library, under **Non-Steam**. Started from Steam, your controller works as a gamepad in every game RustyShelf starts.

- **Leaving a game:** hold **Select + Start** for a moment. RustyShelf closes the game, whatever the emulator, and you're back on the shelf. (Your emulator's own quit hotkey works too. RustyShelf's Home menu on the Guide button is Windows-only for now.)
- **Why not Desktop Mode?** There, programs not started by Steam get the Deck's keyboard-and-mouse layout: **B is the Esc key**, and most emulators quit their game on Esc.

## If RustyShelf doesn't start in Game Mode

RustyShelf adds the options Steam needs by itself (`--no-sandbox`, and X11 in Game Mode). If it still shows a black screen or closes at once:

1. In Game Mode: RustyShelf's page → ⚙ → Properties → Launch Options. Enter: `--no-sandbox --ozone-platform=x11`
2. Still nothing? Send us `~/RustyShelf/data/rustyshelf.log` (its "Linux start:" line shows what RustyShelf saw).

## Emulators RustyShelf looks for

- RetroArch: native, Flatpak, AppImage or Steam's.
- Dolphin, Cemu, PCSX2, RPCS3, DuckStation, PPSSPP, Azahar / Lime3DS, melonDS, RMG, mGBA, Flycast, shadPS4.
- Switch: Eden, Citron or Ryujinx.

They're searched for in `~/Applications`, Flatpak and the usual system folders. ROMs: `~/Emulation/roms`, `~/retrodeck/roms` and SD cards.
