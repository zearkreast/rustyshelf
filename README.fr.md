<!-- Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): a French translation of
     RustyShelf's README.md, which was written from Boxshelf's README.md. -->
<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="renderer/brand/rustyshelf-logo-dark.svg">
  <img src="renderer/brand/rustyshelf-logo-light.svg" alt="RustyShelf" width="520">
</picture>

**Votre ludothèque comme une étagère que l'on parcourt depuis le canapé.**

[English](README.md) · [Русский](README.ru.md) · [Português](README.pt-BR.md) · **Français** · [Türkçe](README.tr.md)

[![Licence : GPL v3](https://img.shields.io/badge/licence-GPL--3.0-blue)](LICENSE)
[![Plateformes : Windows, Linux, Android](https://img.shields.io/badge/plateformes-Windows%20%C2%B7%20Linux%20%C2%B7%20Android-informational)](#installation)
[![Build/release check](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml/badge.svg)](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml)
[![Electron](https://img.shields.io/badge/app-Electron-47848F?logo=electron&logoColor=white)](docs/MANUAL.md#run-from-source)

</div>

Les jeux de console émulés, les jeux Steam et autres jeux PC, ainsi que les applications de streaming, se tiennent côte à
côte comme de vraies boîtes sur une étagère en 3D. Sortez-en une, ouvrez le boîtier, lisez le manuel, regardez le disque
s'insérer et jouez, le tout avec une seule manette.

## Ce qu'il fait

- **Une étagère 3D de vraies boîtes.** Chaque jeu se tient dans le boîtier de sa console, avec sa face avant, sa tranche et
  son dos. Retournez-le, sortez-le, ouvrez-le, retirez le disque, lisez le manuel.
- **Une seule manette pour tout.** Parcourez l'étagère, lancez un jeu, mettez-le en pause, sauvegardez et chargez des
  états, prenez une capture d'écran et revenez à l'étagère sans toucher au clavier.
- **Les jeux de console avec vos émulateurs.** Dolphin, Cemu, Eden, PPSSPP, PCSX2, RPCS3, shadPS4, Vita3K, xemu, Xenia,
  Azahar, et RetroArch pour les systèmes rétro. RustyShelf les trouve, ou vous lui indiquez où ils sont.
- **Les jeux PC sur la même étagère.** Steam, Epic Games, GOG Galaxy, Ubisoft Connect, l'application EA, Xbox / Game Pass,
  et un dossier dont chaque sous-dossier est un jeu. Sous Linux, les jeux Windows tournent avec Wine ou Proton, sans Steam.
- **Jaquettes et manuels trouvés pour vous.** Faces avant, dos, tranches, étiquettes de disque, descriptions et manuels
  viennent de bases de données ouvertes. L'Art Studio et le Manual Studio permettent de choisir entre les versions, pièce
  par pièce.
- **Un musée de votre collection.** Chaque système sur un mur d'étagères, chaque boîtier à sa taille réelle, avec quatre
  éclairages.
- **En plus.** RetroAchievements et succès Steam, une application compagnon sur téléphone qui sert de télécommande,
  dix-huit thèmes de couleurs et les vôtres, un mode démonstration, une musique de menu et vos propres modèles 3D de
  consoles.
- **Privé.** Pas de compte, pas de publicité, pas de télémétrie. Tout ce qu'il enregistre reste dans son propre dossier.

L'interface est disponible en anglais, en portugais du Brésil, en français et en turc.

## Installation

Téléchargez la dernière version sur la [page des releases](https://github.com/zearkreast/rustyshelf/releases), dès qu'une
release sera publiée. D'ici là, [lancez-le depuis les sources](docs/MANUAL.md#run-from-source).

| Plateforme | Fichier | Comment |
|---|---|---|
| **Windows** | `RustyShelf-<version>-windows-portable.exe` | Placez-le dans un dossier à lui et double-cliquez dessus. Si Windows signale un éditeur inconnu, choisissez **Informations complémentaires → Exécuter quand même**. |
| **Linux** | `RustyShelf-<version>-linux.AppImage` | Faites `chmod +x` une fois, puis lancez-le. Les réglages sont conservés dans `~/RustyShelf`. |
| **Android** | `RustyShelf-<version>-android.apk` | Ouvrez le fichier sur l'appareil et autorisez l'installation quand Android le demande. |

Au premier lancement, l'assistant de configuration propose **Find automatically** : il cherche vos émulateurs et vos
dossiers de jeux, vous vérifiez chaque système, puis vous choisissez **Open RustyShelf**.

## Commandes

| Action | Clavier | Manette Xbox / PlayStation |
|---|---|---|
| Se déplacer sur l'étagère | ← → | Croix directionnelle / stick gauche |
| Changer d'étagère | ↑ ↓ | LT / RT |
| Jouer / ouvrir | Entrée | A / Croix |
| Retour | Échap | B / Rond |
| Retourner la boîte | F | Y / Triangle |
| Détails | I | X / Carré |
| Inspecter | V | Clic du stick droit |
| Musée | M | Maintenir View / Back |
| Réglages | S | Start |
| Menu Home pendant un jeu | | Guide, ou maintenir View / Back |

La liste complète se trouve dans le [manuel](docs/MANUAL.md#controls).


## Documentation

Le [manuel](docs/MANUAL.md) décrit tout en détail (en anglais) :

- [Vos dossiers](docs/MANUAL.md#your-folders) : comment les jeux sont trouvés, avec des notes par système
- [Sur l'étagère](docs/MANUAL.md#on-the-shelf) : Inspecter, Détails, Home, le tri, le musée, le mode démonstration
- [Pendant un jeu](docs/MANUAL.md#while-a-game-runs) : le menu Home, les sauvegardes d'état, les raccourcis de chaque émulateur
- [Jaquettes et manuels](docs/MANUAL.md#box-art-and-manuals) et [jeux PC](docs/MANUAL.md#pc-games)
- [Réglages](docs/MANUAL.md#settings) et [extras](docs/MANUAL.md#extras) : application compagnon, succès, vos propres modèles 3D
- [Vie privée](docs/MANUAL.md#privacy) : chaque service contacté, quand, et ce qui lui est envoyé
- [Pour les développeurs](docs/MANUAL.md#for-developers) : tests et builds de release
- [Dépannage](docs/MANUAL.md#troubleshooting)

Les notes sur le Steam Deck sont dans [docs/STEAM-DECK.md](docs/STEAM-DECK.md).

## D'où vient RustyShelf

RustyShelf est un fork renommé de [(T)rustyShelf](https://github.com/GGStonks/t-rustyshelf) de GGStonks, lui-même un fork
de [Boxshelf](https://github.com/sassiestrhihno/boxshelf-beta) de sassiestrhihno. L'essentiel de ce que fait RustyShelf est
le travail de Boxshelf, de son auteur. Ce que les forks ont ajouté est marqué **[fork]** dans le manuel.

> **Vous voulez le Boxshelf d'origine ?** Téléchargez-le sur la [page des bêtas de Boxshelf](https://github.com/sassiestrhihno/boxshelf-beta/releases)
> et signalez ses bugs et vos idées dans les [issues de ce projet](https://github.com/sassiestrhihno/boxshelf-beta/issues/new/choose).

## Crédits et licence

Les sources des jaquettes et des manuels, les polices, les bibliothèques et les contributeurs sont listés dans la
[section Credits du README anglais](README.md#credits).

RustyShelf est un logiciel libre : vous pouvez le redistribuer et le modifier selon les termes de la GNU General Public
License, version 3 ([LICENSE](LICENSE)). Il est fourni sans aucune garantie.

**Nom et logo.** L'auteur de Boxshelf demande que les forks utilisent leur propre nom et leur propre logo : la GPL couvre le
code, pas le nom ni le logo de Boxshelf. Si vous publiez un fork ou une version modifiée de RustyShelf, donnez-lui aussi un
nom et un logo différents. Dire qu'il est « basé sur RustyShelf » est bienvenu.
