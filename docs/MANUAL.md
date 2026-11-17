<!-- Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): written for RustyShelf
     from Boxshelf's README.md, which documents the features RustyShelf carries over. -->
<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="brand/rustyshelf-logo-dark.svg">
    <img src="brand/rustyshelf-logo-light.svg" alt="RustyShelf" width="520">
  </picture>
</h1>

This is the full manual. For a short overview, see the [README](../README.md).

**RustyShelf turns your game library into a shelf you browse from the couch.** Emulated console games, Steam and other
PC games all stand side by side as real boxes on a shelf, and play with one controller.

| | |
|---|---|
| **Runs on** | Windows, Linux (Steam Deck included), Android (in progress) |
| **Licence** | [GNU GPL v3.0](../LICENSE) |
| **Downloads** | Windows, Linux and Android builds on the [releases page](https://github.com/zearkreast/rustyshelf/releases) |
| **Status** | Alpha: a native rewrite in Rust of the Electron app, which is kept on the [`archive/electron`](https://github.com/zearkreast/rustyshelf/tree/archive/electron) branch |

## Where RustyShelf comes from

RustyShelf is a renamed fork of [(T)rustyShelf](https://github.com/GGStonks/t-rustyshelf) by GGStonks. (T)rustyShelf is
in turn a fork of [Boxshelf](https://github.com/sassiestrhihno/boxshelf-beta) by sassiestrhihno. Most of what this manual
describes is Boxshelf's work, by its author, carried over from the Electron app.

> **Want the original Boxshelf?** Download it from the [Boxshelf beta page](https://github.com/sassiestrhihno/boxshelf-beta/releases)
> and report its bugs and ideas in [that project's issues](https://github.com/sassiestrhihno/boxshelf-beta/issues/new/choose),
> not ones about RustyShelf.

The Electron app (`main.js`, `renderer/`, the Capacitor Android app) and its own manual are on the
[`archive/electron`](https://github.com/zearkreast/rustyshelf/tree/archive/electron) branch. The native app reads the same
settings and data, so a setup made in either works in both. The Museum stays Electron-only.

## What works

RustyShelf is a native app in Rust, in `crates/` (a Cargo workspace built with [Bevy](https://bevy.org/)). It isn't
complete yet.

[Bevy](https://bevy.org/)). It isn't a replacement yet; install and use the Electron app above.

**What works so far:**

- Reads the same settings as the Electron app (`data\config.json`), so a setup made in either works in both.
- Finds every console's games the same way the Electron app does: one box per game, discs grouped, updates and DLC left out.
  That includes Xbox 360 games named by their title id (disc images, extracted games, Games on Demand and Arcade
  packages), unpacked Switch 2 games, PS4 `.pkg` packages shown as not installed, PS5 games, arcade sets named from the
  FinalBurn Neo list, and the names and pictures in a Batocera / RetroBat `gamelist.xml`.
- Shows them with their box art on the shelf as the Electron app lays it out, one console at a time: the chosen
  game's case forward in the middle, the others in two angled stacks beside it with their spines out, each in its
  console's own case (the Electron app's table of case shapes and colours), under the same plastic shine and glow.
  Behind them is the colour theme's backdrop (Settings → Display → Theme; all eighteen, with what moves in them:
  Spectrum's ribbons and dust, Arcade's stars and mountains, Aurora's curtains and the rest), in the console's
  colour and washed with the chosen cover's (in Spectrum and Classic; Blur makes the softened cover the whole
  backdrop, and Artwork shows the game's own wide art sharp where it has some on disk, else its cover standing
  at the right), the floor mirroring the cases. Custom themes work as in the Electron
  app: each folder in `Custom themes` with a picture named `background` is a theme (with pictures for single
  consoles and a `theme.json`), the same three examples are written the first time, and Settings → Display →
  Make a theme from a picture makes one of a picture of yours. Over them: the console's name in its own lettering, the consoles' tabs, the clock, the chosen game's
  title, and the bar of keys (the keyboard's, or the pad's once a pad is used), all as the Electron app sets them.
  A shelf has the Electron app's own cases among its games: Surprise me (Enter or R lands on a game, Enter again
  plays it), Open with the console's emulator, and Add games on a shelf that has none and at the end of Custom.
  A favourite wears its heart, a game that isn't installed stands in greys, and the shelves keep the order given
  them in the Electron app's Settings.
  A game without box art gets the cover the Electron app draws for it: a scene and a title font picked by its name
  under the console's band, and a back with four small scenes, its description and what Details knows of it.
- Covers a console's game starting with the Electron app's boot slide: the console's own colours and lettering,
  the game's cover and title, what starts it, and a line filling (their moving backgrounds aren't drawn yet).
- Starts a game the way the Electron app does: in its own emulator build when Details gives it one, else its console's,
  else RetroArch with the console's core; with the options the emulator in use takes (a standalone retro emulator,
  Ryujinx, Dolphin's settings for the session, the GameCube startup), the real Xenia behind Xenia Manager, a PS Vita game
  by its title id, an unpacked Switch 2 game in its newest update, and a PS5 game with the Kyty launcher's settings.
- Asks which build a Switch 2 game starts in, as the Electron app does: Play lists every Eglantine build and fork in
  the Switch 2 emulator folders (with the ones added by hand), starting on the one the game was last started in, a
  .nup game on a build that opens .nup; "+ Add another emulator…" adds one from there. Settings → Switch 2 →
  Emulators (forks) chooses one build for every game instead, adds or removes builds and folders of them, and a
  shelf build that's gone is replaced by the newest one at the start.
- Shows your installed Steam games on a PC shelf, from every Steam library, with Steam's covers (or the cover you chose
  in the Electron app) and your playtime, and starts them through Steam. Steam's tools (Proton, runtimes) stay off it
  unless Settings → PC games says otherwise.
- Puts the other stores' games on the PC shelf too: the Epic Games Launcher's, GOG Galaxy's, Ubisoft Connect's, the EA
  app's, Xbox / Game Pass and HoYoPlay's (on Windows), and every game in the PC games folder. Epic's, Ubisoft's and EA's
  start through their store, an Xbox game as the Windows app it is, the others by their own program.
- Shows the apps Quiver Launcher installed and the programs you added by hand on the Custom shelf, Quiver's ports and
  recompilations on the Ports & Recomps shelf, or on the PC shelf or a console's shelf when you moved them there, and starts them in their own folder (with Windows' administrator prompt
  for a program that asks for it).
- Gives a PC game the cover you chose, else its console box when Settings → PC games → Console box art is on, else
  the store's cover, a picture in the game's own folder, or the cover of the Steam game with the same name. With
  Console box art on, the boxes not made yet are made behind the shelf, one game at a time and never while a game
  runs: the front of the newest console release the LaunchBox index has (PS5, Xbox Series X|S, PS4, Xbox One,
  Switch), with its back and spine where they're scanned, the console's band painted over with a PC one
  (`data/pc-console-covers`). Only an index already built is used: box art downloads build it.
- Shows the Media shelf's apps (Stremio, Nuvio) and starts them.
- Keeps the play history the Electron app keeps (`data\history.json`): a console game's launches and time in it, a PC
  game's or an app's launches. An emulator that closes within five seconds wasn't a play, and the shelf says so.
- Shows the names you gave games in the Electron app (Details → Rename).
- Does what the Electron app does around a launch: Eden and its forks, PCSX2 and RPCS3 stop asking before they close,
  Dolphin's Stop hotkey takes the quit combo from every controller, a 3DS `.cia` is installed into Azahar the first
  time it's played, and on Windows a RetroArch core that's missing is downloaded (the boot slide says how far each is).
- Fetches what the shelves need by itself, behind the shelf: the Xbox 360 title list (again after a week), the arcade
  names (once), the size of unpacked Switch 2 games and of PC games' folders, and, unless box art downloads are off,
  the covers and store info Steam's own cache doesn't have. With box art downloads on, the other stores' games get
  the art of the Steam game with the same name. The shelf shows all of it as soon as it's in.
- Downloads box art for console games missing a front, a back or a spine, unless box art downloads are off: fronts
  from the libretro thumbnail library, backs, spines and discs from the LaunchBox Games Database (its index is
  downloaded and built once), and where those have none a spine cut from LaunchBox's full box wrap, a spine (GameCube, Wii) or a spine and a back (Switch,
  3DS) cut from GameTDB's full cover scan, or a Switch 2 front or back from the Super Mario Wiki; a Switch or 3DS
  game no source has a cartridge photo of gets one drawn from its front cover. With your own SteamGridDB key, a game
  no source has a front for gets its best-voted poster, replaced by a real scan when a later check finds one. Each game is looked at once a week at most. With
  Settings → Box art → Sharpest scans first (on unless switched off), the sharpest version comes first and a low-res
  picture RustyShelf downloaded is replaced by a clearly sharper one; any other picture that's there is never
  replaced. Both apps keep the same record of which pictures they placed (`data/art-provenance.json`).
- With your own SteamGridDB key (Settings → Accounts → SteamGridDB, with a check of the key) and box art downloads on, gives PC
  games, ports and recomps that have no cover at all SteamGridDB's best poster, as the Electron app does (their
  titles are sent to SteamGridDB). The key is kept encrypted like the RetroAchievements one, and the lookups are
  kept in the same `data\sgdb-cache.json`.
- Signs in to ScreenScraper with your own account (Settings → Accounts → ScreenScraper: username, password, a developer login of
  your own if you have one, and a check that shows the lookups left today), kept encrypted like the other keys.
  The lookup itself is the Electron app's (a game matched by its file's checksums, name and size, then by title;
  the account's limits kept to; the same `data\ss-cache.json`), ready for the Art Studio, which is where its
  box scans are offered and isn't in the Rust app yet.

- Sets itself up on the first run: it looks for your emulators, games folders, Steam and the streaming apps, shows the
  consoles it found switched on, and lets you change each one's emulator and games folders before the shelf opens.
- Has Settings (S or F1, or Start on the pad and then Settings; View opens Details): each console's shelf, emulator and games folders, RetroArch, Steam and the
  other stores, the PC games folder, box art downloads and their region, the art library, fullscreen, the quit combo,
  searching again and restoring the emulators' own settings. Everything is saved to `data\config.json` by the rules
  the Electron app's settings go through, so both apps keep reading the same file.
- Follows Look's Interface size (everything bigger for a TV across the room, and scaled up on its own in a window
  taller than 1080), Open into (the shelf and game you were on last time, kept in `data\native-place.json`, or Home)
  and Reduce motion (the moving themes stand still; Follow takes it from Windows' or GNOME's animation switch), and
  Controls' Scroll speed (how quickly a held direction repeats) and Controller rumble (short pulses as a game starts,
  the Surprise me case lands, a game is made a favourite or hidden, the shelf changes, and a controller connects).
- Moves in Settings, Details and the shelf's menu as the Electron app does: one highlight glides from row to row, the
  list scrolls to keep the chosen row in view (▲ / ▼ when there's more), a value slides in on ← / →, a switch's knob
  springs over, a pressed row gives a little, a page comes in from just below (Details from the right) and the whole
  fades in and out. Reduce motion keeps only the fades, quicker.
- Changes a Steam, store or added game's cover from Details → Box art: a grid of covers to choose from — Steam's
  own (a Steam game's, else the Steam store's games of that name), the boxes of its console releases (a box chosen
  brings its back and spine, and its console's case), SteamGridDB's posters with a key — and tiles to search under
  another name, use a picture of yours, or go back to the automatic cover (`data/pc-covers`, as the Electron app
  keeps it). The covers' thumbnails are kept in `data/pc-cover-cache`.
- Shows every PC game's cover in a gallery (Settings → PC games and apps → PC game covers, or Box art & media → PC
  games), each with where it comes from (your pick, its console box, the store's box art, Steam's art, or a
  made-up case); choosing one opens its cover picker, and the gallery comes back when it closes.
- Settings → Custom games → Covers, for the custom games without a cover of their own: Choose covers one by one
  (a picker for each, with Skip; B stops), Use the best cover for each (SteamGridDB's top cover, with a key), and
  Undo the covers just found.
- Has the Electron app's two panels over the shelf: the Sort panel (G, or Menu held), a card at the top with the
  order and the filter as rows of chips, applied as they change; and the quick menu (Menu, Tab, or a finger held on
  the shelf), sliding in at the right while the shelf steps aside, with the chosen game, its Favorite / Details /
  Box art / Hide tiles, and Find a game, the volume (← →), Sort & filter's chips, Trophies, Updates, Settings and
  Power (Exit RustyShelf, Sleep, and Restart and Shut down pressed twice). A white pill glides between its rows.
- Has Settings → Shelf → Console order: each shelf moves up (←) or down (→) the list, and the shelves along the top
  follow at once (`ui.shelfOrder`, which the Electron app reads too); Reset puts RustyShelf's own order back.
- Takes text from the keyboard or, with a pad, from an on-screen keyboard: each console's launch options and
  emulator options (Settings → the console → Emulator & advanced, which also opens the emulator on its own, holds the
  GameCube startup and Kyty's launcher switch, and the console's own art folder), and the RetroArch core a console
  runs in.
- Has Custom games in Settings: a program added by hand, its name, launch arguments and the shelf it sits on.
  Many can be added at once, as in the Electron app: several programs picked together, or a folder of games
  scanned (one game per folder, its main program picked, helpers left out, folders of categories stepped into,
  the picture in a game's folder taken as its cover), each found game ticked or not before it's added. On
  Windows a Playnite library can be brought over too (experimental): the games added there by hand onto the
  Custom shelf with their covers and favorites, emulated games' folders onto their consoles, and it can all
  be taken off again.
- Sets emulators up for you on Windows, as the Electron app does (Settings → a console → Set up for me, and the
  first run's switch): the controller, the Home menu's hotkeys, one BIOS folder, a graphics preset and no quit
  questions for RetroArch, Dolphin, DuckStation, PCSX2 and PPSSPP, each file backed up first, with Undo. The page
  also shows the console's RetroArch core (and downloads it) and checks its BIOS files.
- Closes a running game with the quit combo (L3 + R3 unless Settings → Controller says otherwise), held for about
  a second: the emulator is asked to close as a click on its X does, and ended if it's still there eight seconds
  later. On Windows the pads are read through XInput while the game is in front, and the combo only counts while the
  game or RustyShelf is; held on the shelf, it closes RustyShelf.
- Follows a Steam, Epic, Ubisoft, EA or Xbox game while it runs (Seamless play, on Windows, as the Electron app does):
  the game's own process is found in its install folder, its playtime is kept, the quit combo closes it (never
  Steam or a store's launcher), and the shelf knows when it has ended. Settings → Display turns it off.
- Keeps a PC game's window the way Details → Window says (for Quiver's, GOG's and HoYoPlay's games, the PC games
  folder's and programs added by hand): borderless takes its frame off and stretches it over its screen while it
  runs, with the Electron app's own helper.
- Plays the interface's sounds, synthesised as the Electron app's are (the same voices, room and limiter, checked
  against renders of the Electron app's own engine): Aero, Melody with its built-in songs, and Music box; each shelf
  in its own key; the five boot sounds. Settings → Sounds has the switch, the volume, the voice and the boot sound.
- Plays the menu music under the shelf (Settings → Sounds → Menu music): Drift, Night and Glow, written as it goes
  like the Electron app's (the same notes for the same seed, in the shelf's key), stepping back for each sound and
  going quiet while a game runs.
- Plays your own music instead (Menu music → Your music): the audio files in the Music folder beside RustyShelf
  (mp3, ogg, wav, flac, m4a; sub-folders too), shuffled, never the same one twice running; a file that can't be
  played is skipped. Opus files aren't played yet.
- Reads the MIDI files in the Songs folder as songs for the Melody voice, as the Electron app does (the top line as
  the tune, the bass under it as the chords), with or without the built-in songs.
- Shows what you're playing on Discord, when Settings → Display has it on with your Discord application id.
- On Linux, adds itself to Steam as a non-Steam game (Settings → System → Add RustyShelf to Steam: Steam closes and
  opens again, its old list is backed up first), so it starts from Steam and Game Mode with the controller as a
  gamepad. On a Steam Deck in Desktop Mode, started outside Steam, the shelf says so once and offers to do it. Holding
  Select + Start closes a running game.
- Has the shelf's menu (G, or Menu on the pad): the order of the shelves (A-Z, release year, last played and the rest of
  the Electron app's), which games they show (favorites, unplayed, a genre, a decade …), and the chosen game as a
  favorite. The choices are the Electron app's own, in the same settings.
- Has the Home shelf (the twelve games you played last, then your favorites), the All shelf when Settings → Display
  asks for it, and your collections (Details → Collections puts a game on shelves of your own; Settings →
  Collections renames and deletes them), each game starting as it does on its own shelf.
- Groups Settings → Accounts by service, each with its own page: RetroAchievements (with Emulators, whether each
  console's emulator is signed in, Xenia's and RPCS3's own achievements included), Steam, SteamGridDB, IGDB
  (Client ID and Client Secret), ScreenScraper and RomM. Every sign-in and key is also written to `data\.env`, one
  `NAME=value` line each in plain text (`RA_USER`, `RA_API_KEY`, `STEAM_WEB_API_KEY`, `STEAMGRIDDB_API_KEY`,
  `IGDB_CLIENT_ID`, `IGDB_CLIENT_SECRET`, `SCREENSCRAPER_USER`, `SCREENSCRAPER_PASSWORD`, `SCREENSCRAPER_DEV_ID`,
  `SCREENSCRAPER_DEV_PASSWORD`); a value there wins over `config.json`, so the file can be edited by hand or
  carried to another PC. The keys already in `config.json` are copied there on start.
- Reads RetroAchievements as the Electron app does (Settings → Accounts → RetroAchievements: the username and web
  API key, a check of the account): a game's progress beside its facts on the shelf, its achievements in Details,
  and what was unlocked when a game is come back from. The key is also kept as the Electron app keeps it,
  encrypted for this Windows user, so both apps open each other's. PCSX2 counts as signed in with its token in
  `inis\secrets.ini`, where PCSX2 2.x keeps it.
- Shows a Steam game's achievements in Details, read from what Steam keeps on this PC (its `appcache\stats`
  files, for the account last signed in), as the Electron app does, with how many players have each (Steam's
  world percentages) and steamhunters.com's points for it, so they sort by rarity or points; a game never started
  on this PC has none.
- Has the Trophies screen (T on the shelf, the quick menu, or Settings → Library → Trophies): the whole
  RetroAchievements account and, with your own Steam Web API key (Settings → Accounts → Steam), the Steam
  achievements of the account signed in to Steam on this PC: every game with how far it is, the last 30 days'
  unlocks with how rare each is, and the mastered games, from everywhere or from one of the two (From ◀ ▶). The
  highlighted game's icon or achievement's badge shows beside the list, as it does in Details' achievements. The
  lookups are kept where the Electron app keeps them (`data\ra`, `data\steam-ach`).
- Lists the room the PC games take (Settings → PC games and apps → Storage): the installed Steam, Epic, GOG,
  HoYoPlay and PC games folder games by drive, the biggest first, with each drive's free space.
- Finds a game (/ or Ctrl+F, or the left stick held): type a part of its title and the matches from every shelf
  are listed as you type, best first, by the Electron app's own rules (capitals, accents and punctuation don't
  matter); with nothing typed, the games played last. Enter takes the shelf to the one chosen.
- Has Details (I, or X on the pad): the game's description, release date, developer, publisher, genre, players,
  rating and score from the box art database (or Steam's store for a Steam game), its file, emulator and playtime,
  with Play, Favorite and Hide from the shelf. Settings → Hidden games shows a hidden game again.
- Exports and imports everything (Settings → System), as the Electron app does and in the same layout, so an
  export made by either app goes into the other: the settings, the downloads (art, manuals, the box art
  database) and the art kept next to your games, into one folder with a manifest. The page says how big the
  export will be, shows the copy as it goes, and can pause or cancel it; exporting into a folder that already
  holds an export copies only what's new or changed. An import replaces files of the same name, keeps the
  settings there were as a backup file, and starts RustyShelf again.
- Tells you the newest release when asked (Settings → System → Check for a new version), comparing versions
  as the Electron app does, and opens its page; nothing is downloaded or installed, and it doesn't ask by
  itself at start yet.
- Reads a game's manual (Details → Read the manual) when its pages are beside its box art, where the Electron
  app keeps them: the cover on its own, then the pages in pairs like an open booklet, ← → to turn. A PDF manual
  is drawn into pages by the Electron app's rule (1600 pixels across, 80 pages at most, the PDF removed once all
  of it is in pages), with hayro, a PDF renderer written in Rust; one kept as a PDF opens in the PC's own PDF reader.
- Finds manuals as the Electron app does: Details → Find a manual lists what archive.org and GamesDatabase.org
  have for the game (matched and scored by the Electron app's own rules: the right game, the right sequel, the
  right console, a manual and not a guide), and the one picked is saved as pages beside the box art. With
  Settings → Box art → Fetch manuals for new games on, the confident English matches (or what the Electron
  app's Manual Studio set for the console) come in by themselves, forty games a start at most, a game looked at
  again a day after an error and a month after finding nothing, in the same `data\manual-auto.json`. Before
  that search, the console's scan collections on the Internet Archive are used as the Electron app uses them
  (the same matching of a game to a collection's file, the same `data\manual-collections.json`): the sharpest
  manuals there are, also over a manual that's there unless that one is complete and much sharper, the old
  pages moved to `.manual-originals` beside them. A manual scanned open (two pages a picture) is cut into single
  pages, the front cover first. A collection's manual that has no page scans (Microsoft's digital Xbox 360
  manuals) is drawn from its PDF, and a small scan gives way to GamesDatabase's copy when that one, drawn 2600
  pixels across, really holds more detail. Details has the Manual Studio's two switches for a manual that's there: Keep this manual (no
  automatic pass changes it) and Wrong manual (the pages are another game's: they go to `.manual-originals`,
  that copy is never brought again, and the next best takes its place, a collection's first). A console's page
  says which manuals its games get (the best English one, or a language or region), in the Electron app's
  `data\manual-studio.json`. Details → Manual Studio lists every manual found for the game (the one in use, earlier
  ones set aside, the console's collections, GamesDatabase's PDF, archive.org's uploads, each checked and its first
  page fetched), with its language, region, pages, scan quality and whether it's complete, the best for the
  console's choice first by the Electron app's own points; Enter puts one in place, Undo puts the manual before back,
  and Your own PDF… turns a PDF of yours into pages (the file stays where it is). The manual replaced is always kept
  in `.manual-originals`. Repair is still the Electron app's.
- Has the Home menu (Settings → Controller turns it off, or makes it a centred card). Over a running game, Guide
  on the pad (or View held on its own for a second) pauses the game with its emulator's own hotkey and brings
  RustyShelf forward with Resume, Save state, Load state, Screenshot, Power (Sleep, Restart, Shut down) and Quit
  to shelf: only what that emulator can do, by the Electron app's table of hotkeys and the console's own over
  it. What's chosen goes back to the game's window as those hotkeys; Quit, Restart and Shut down need the button
  held. On the shelf, Home or P (or Guide) opens it with the Home shelf, Settings and Power. Unlike the Electron
  app's, it's drawn in RustyShelf's own window in front of the paused game, not over the game's picture, so a
  saved state is confirmed by the emulator's own message.
- Shows the buttons of the controller in hand (Settings → Controller → Button prompts and Confirm button, as in
  the Electron app): Xbox letters, PlayStation shapes or a Switch pad's letters on the hint bar, in Settings and
  in the Home menu, by the pad's maker and model, then its name. With a Switch pad (or when chosen) the right
  face button confirms and the bottom one goes back, on every screen. A pad Windows shows as an Xbox 360 one is
  asked about as the Electron app asks: the real controllers plugged in (USB or Bluetooth) say whether a
  PlayStation or Switch pad is behind Steam Input or DS4Windows.
- Has a first Inspect (V, or the right stick's click): the chosen case comes up off the shelf to be turned
  freely with the arrows or a stick, tipped, flipped to its other side and brought nearer or further; M reads
  its manual, Enter plays it, Escape puts it back. The case doesn't open yet.
- Has the Art Studio's picker (Details → Box art): for a console game's front, back, spine and disc, every
  version LaunchBox, libretro and GameTDB have (from the lists the box art download keeps), on Switch 2 the
  Super Mario Wiki's box, with your own account ScreenScraper's scans and with your own key SteamGridDB's posters
  for the front (only the HD look picks one by itself), for a Switch or 3DS cartridge one drawn from the front cover,
  the picture in use
  and the ones replaced before it, the best for the console's style first by the Electron app's own points
  (authentic, HD, a region, special edition), each shown beside the list as it's highlighted. Enter puts one in
  use: the old picture goes to `replaced` beside it, never deleted, a spine or a back is cut from a full wrap
  where that's what the source has, and the part is kept as chosen (`art.locks`, which the automatic art check
  respects in both apps). Put the earlier picture back undoes it, and A picture of yours puts a PNG, JPEG or
  WebP of your own in (for a spine, also cut from a picture of the whole box opened flat). Downloads are kept in `data\art-cand`, the
  Electron app's own cache. Region, Source, Type and Edition filters show where the versions differ (kept for the
  session, across games); Back to the shelf style hands a part you picked back to the console's style, which picks
  it again; Lock this game keeps every part as it is (shelf styles and the automatic art check skip it), and
  unlocking picks again only the parts the style changed since, or that are missing. A console's page has its Box art style (Authentic, HD, a region's boxes, Special
  editions), kept as the Electron app keeps it (`art.styles`): the picker puts that look's best first, and the
  automatic art check takes its parts in that look's order and region. Changing a console's style picks its games'
  pictures again in the background, two games at a time, with its progress on that row: every part whose look
  changed, was picked by hand (that pick gives way) or is missing gets the new look's best, a game locked as it is
  is left alone, and the old pictures go to `replaced`. Details → Box art → Complete sets offers a front, back, spine
  and disc that belong together (one region's LaunchBox, GameTDB or ScreenScraper scans, the best of each source for
  a region, or the HD set; a back and spine cut from one scan first), puts all four in or none, and undoes it. The
  Complete sets style (a region's, or HD) gives every unlocked game its set, and a game with none its parts one by one.
- Has Details → Art match for a console game whose file name or serial isn't the original game's (a ROM hack, a
  translation, a mod): it searches the LaunchBox index already built and libretro's list for the names holding the
  words typed, and the one chosen is kept with the game (`games.<id>.artName`, as the Electron app keeps it). Its
  art is fetched again under that name behind the shelf (parts picked in the Art Studio stay), and the Art Studio
  and Details' info look it up under that name too. Back to automatic goes back to the game's own name.
- Has the phone companion (Settings → Phone companion): the same page on the phone, served by the same rules as
  the Electron app's server (home-network addresses only, PIN pairing with its lockouts, tokens kept as hashes,
  pictures under random ids) and with the same `data\companion.json`, so a phone paired with one app stays paired
  with the other. The phone's remote is one more controller on every screen, its keyboard types into Find a game
  and Settings' text boxes, and Play starts the game as pressing A on its case would, closing what's open first.
  The PC's address is the adapter its traffic leaves by, or one on a 192.168 network when that's a VPN; the
  Electron app's list of virtual adapters to pass over isn't used.

**Not yet:** the rest of Settings (Artwork's wide art for games whose art is only on the web; adding Microsoft Store apps), the
rest of the Art Studio (backs and spines made by RustyShelf among the versions: the Electron app draws them in its page), the Manual Studio's Repair, and opening the case in Inspect (its disc and its manual inside). On Android the native
app is at its first stage: it builds as an APK of its own ("RustyShelf (native)", which installs beside the Electron branch's
Android app) and keeps its settings in its own storage. A controller works as on the desktop (its buttons, sticks and a
D-pad sent as a hat), Android's Back is Esc, and a finger works as in the Android app: a sideways drag along the shelf,
a tap on the chosen case to play, a swipe in from a side edge (or, off the shelf, a quick swipe left) for Back, a drag
up or down through Settings. Settings shows the consoles the Android app has; a console's Games folders → Add a folder
opens Android's folder picker, and the folder is read as the Android app reads it (`data/folders.json` keeps what was
picked). Play starts the game in the console's emulator app as the Android app does (its table of apps and how
each takes a game is `crates/rustyshelf-core/data/android-emulators.json`): the one set for the console,
else the first of its usual ones that's installed, and the launch is counted. Choosing the emulator app in Settings,
the time played, box art and the Thor's bottom screen aren't in it yet. Hiding the taskbar
while a game runs and the rest of Seamless play (the loading cover, the window mode, Go to desktop) aren't in it yet either.

## Run from source

```bash
cargo run -p rustyshelf --release -- --windowed
```

The Android APK (needs the Android SDK and NDK, and `cargo install cargo-ndk`):

```bash
cargo ndk -t arm64-v8a -P 26 -o crates/rustyshelf/android/app/src/main/jniLibs build --release -p rustyshelf --lib
```

```bash
cd crates/rustyshelf/android && ./gradlew assembleDebug
```

It uses the same folder as the Electron app: next to the program when a `data` folder is there, otherwise `~/RustyShelf`
on Linux. `RUSTYSHELF_ROOT` points it somewhere else.

## Controls

| Action | Keyboard | Pad |
|---|---|---|
| Choose a game | ← → | D-pad / left stick |
| Jump by letter | Z / C | LB / RB |
| Change console | ↑ / ↓ | LT / RT |
| Play | Enter | A |
| Turn the case round | F | Y |
| Inspect: hold the case up and turn it freely | V | Right stick click |
| Favorite | H | hold Y |
| Details | I | X |
| Sort and filter (the Sort panel) | G | hold Menu |
| Quick menu: Favorite, Details, Box art, Hide, Find, volume, sorting, trophies, Settings, power | Tab or Esc | Menu |
| Trophies | T | (the quick menu) |
| Settings | S | View |
| Surprise me | R | left stick click |
| Find a game | / | hold the left stick |
| Home menu | Home / P | Guide (over a game: Guide, or hold View) |
| Quit | Esc, then Power → Exit RustyShelf | |

## For developers

### Tests

`cargo test --workspace` runs its tests, which compare its title and folder rules against the Electron app's own answers
(`node scripts/rust-fixtures.js` regenerates those), its sounds and drawn covers against the Electron app's own
(`node scripts/rust-sound-fixtures.js`), its painting of the themes' backdrops against Chromium's
(`node scripts/rust-paint-fixtures.js`), and its Home menu against the Electron app's page
(`node scripts/rust-home-fixtures.js`); the last three need Playwright's Chromium.


Those scripts are on the `archive/electron` branch, as they run the Electron app's code.

### Release builds

Every push to `rustyshelf` triggers **Build/release check** (`.github/workflows/build-release-check.yml`), which decides
what to do from the commit message:

| Commit message contains | Result |
|---|---|
| Nothing special | The check stops; nothing is built |
| `For build` | The Windows zip and installer, the Linux AppImage and the Android APK are built (`build.yml`) and kept as the run's downloads for 14 days; no release |
| `For version 0.5.0` | The same builds, then the GitHub release `v0.5.0` (`release.yml`) with their SHA-256 sums, marked as the latest; its text is `.github/release-notes/v0.5.0.md` when that file is there, else the commit message |
| `For alpha` | The Windows zip and installer and the Android APK, published as a prerelease `A.B.C-alpha.N` (`alpha.yml`) |

A release is `A.B.C`, the Cargo workspace's version (`Cargo.toml`), which the app shows; a build is `A.B.C.DDDD`, `DDDD`
the build of it, counted from the release. `For build` and `For version` fail when they name another version than Cargo's: change the workspace
version first. A `For version` commit also fails if that version has already been released. `For build` only counts as
a phrase on its own, followed by a version, punctuation or the end of the line.

The Android APK is signed with RustyShelf's own key when the repository has the `ANDROID_KEYSTORE_BASE64` (the
keystore file in base64), `ANDROID_KEYSTORE_PASSWORD`, `ANDROID_KEY_ALIAS` and `ANDROID_KEY_PASSWORD` secrets, so each
one installs over the one before. Without them a build or an alpha gets a debug key made for that run, and a release
stops.

`node scripts/push-build.js "Build: what it is"` creates that build commit, pushes it and returns immediately.
`--status` shows how the latest runs are going.

### Community report

`node scripts/community-report.js` collects what people around Boxshelf ask for, report and build on GitHub, sorted with
keyword rules (no AI) into `community-report/report.md`. `node scripts/fork-survey.js` lists the changes in Boxshelf's
forks that aren't in RustyShelf yet.

## Credits and licence

The credits (box art and manual sources, fonts, libraries, contributors) and the licence are in the
[README](../README.md#credits).
