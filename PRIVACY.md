# Privacy

RustyShelf has no account of its own, no analytics, no ads and no crash reporting. It sends nothing to its developers.
Everything it knows about you stays in its `data` folder on your computer.

It does talk to other sites, to find art and game info and to read the accounts you connect. This page lists every one,
what it is sent and when.

## What stays on your computer

- **The `data` folder**: your settings (`config.json`), play history, favourites, collections, caches of art and game
  info, and the sign-ins and keys from Settings → Accounts.
- **Keys and sign-ins** are kept in `config.json` (on Windows, sealed so they open only for your Windows user) or, if
  you choose, in plain text in `data/.env`. They are never written to a log, and each is sent only to its own site.
- **Your games, saves and emulators** are only read, to show them and start them. Nothing about them is uploaded.

## Sent without an account or key

These need no sign-in. They send the name of a game (or its Steam app id) and get pictures or text back.

| Site | What it's asked | When |
|---|---|---|
| GitHub (`api.github.com`) | Which RustyShelf release is the newest | When you press Check for updates, or at start if automatic updates are on (off by default) |
| GitHub (`raw.githubusercontent.com`) | FinalBurn Neo's arcade name list, Xenia's Xbox 360 title list | Once for arcade, weekly for Xbox 360, when those shelves have games |
| Steam store (`store.steampowered.com`, Steam's CDN) | A Steam game's cover, release date, developer, genres, screenshots | For Steam games missing them, kept a month |
| steamhunters.com | A Steam game's achievement points | When you open its achievements, kept a week |
| libretro thumbnails, LaunchBox Games Database | A console game's box art | When box art fetching is on (off by default), or when you ask for art |
| GameTDB, Super Mario Wiki | Switch and 3DS box scans | Only when no other source had the part, kept a week |
| gamesdatabase.org, archive.org | Game manuals | Only when you look for or download a manual |
| libretro buildbot | A RetroArch core | Only when you press to download it (Windows) |
| umu (`umu.openwinecomponents.org`) | A game's store and id, for its Proton fixes | Linux, when a Windows game starts through umu-run |

Each request carries the user agent `RustyShelf` and your IP address, as any web request does.

## Sent only with your own account or key

Off until you add the account or key in Settings → Accounts. Each is sent only to its own site.

| Service | What it's sent |
|---|---|
| SteamGridDB | Your API key, and game names to find covers |
| IGDB (through Twitch) | Your Twitch developer app's Client ID and Secret, and game names to find info and screenshots |
| ScreenScraper | Your user name and password, and a game's name, console, file name, size and checksums (CRC, MD5, SHA-1) to find art |
| RetroAchievements | Your user name and web API key, to read your progress |
| Steam Web API | Your API key and Steam id, to read your achievements and library |
| RomM | Your server's address and sign-in, to read and download your games from your own server |
| Epic Games (Linux, through legendary) | Your Epic sign-in, kept by legendary in `data/legendary` |

## On your home network

- **Phone companion**: off until you switch it on. It runs a small web server on your home network, answers private
  network addresses only, and every request needs the token a phone gets by pairing with a six-digit PIN.
- **Game streaming**: RustyShelf asks Moonlight on this computer for the apps of a host you already paired, and can wake
  that host with a Wake-on-LAN packet. It streams nothing itself.
- **Discord**: with Rich Presence switched on and an application id set, it tells the Discord app on this computer
  which game you are playing. Discord shares that as it shares any game.

## Changing your mind

Removing a key or sign-in in Settings → Accounts stops that service at once. Deleting the `data` folder removes
everything RustyShelf has kept. Questions or a problem: [open an issue](https://github.com/zearkreast/rustyshelf/issues).
