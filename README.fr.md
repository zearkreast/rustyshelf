<!-- Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): a French translation of
     RustyShelf's README.md, which was written from Boxshelf's README.md. -->
<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/brand/rustyshelf-logo-dark.svg">
  <img src="docs/brand/rustyshelf-logo-light.svg" alt="RustyShelf" width="520">
</picture>

**Votre ludothèque comme une étagère que l'on parcourt depuis le canapé.**

[English](README.md) · [Русский](README.ru.md) · [Português](README.pt-BR.md) · **Français** · [Türkçe](README.tr.md)

[![Licence : GPL v3](https://img.shields.io/badge/licence-GPL--3.0-blue)](LICENSE)
[![Plateformes : Windows, Linux, Android](https://img.shields.io/badge/plateformes-Windows%20%C2%B7%20Linux%20%C2%B7%20Android-informational)](#installation)
[![Build/release check](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml/badge.svg)](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml)
[![Rust + Bevy](https://img.shields.io/badge/app-Rust%20%2B%20Bevy-CE422B?logo=rust&logoColor=white)](docs/MANUAL.md#run-from-source)

</div>

Les jeux de console émulés, les jeux Steam et autres jeux PC se tiennent côte à côte comme de vraies boîtes sur une
étagère, et se jouent avec une seule manette.

> **Cette branche est l'application native, en Rust.** C'est une alpha, pas encore complète. L'application Electron dont
> elle est réécrite (et son application Android, faite avec Capacitor) se trouve sur la branche
> [`archive/electron`](https://github.com/zearkreast/rustyshelf/tree/archive/electron). Les deux lisent les mêmes réglages
> et les mêmes données : une configuration faite dans l'une fonctionne dans l'autre. Le musée reste propre à Electron.

## Installation

Les alphas pour Windows et Android sont sur la [page des releases](https://github.com/zearkreast/rustyshelf/releases).
Ou lancez-la depuis les sources :

```bash
cargo run -p rustyshelf --release -- --windowed
```

Le [manuel](docs/MANUAL.md) (en anglais) indique [ce qui fonctionne déjà](docs/MANUAL.md#what-works), la compilation pour
Android et les [commandes](docs/MANUAL.md#controls).

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
