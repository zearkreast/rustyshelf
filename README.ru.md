<!-- Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): a Russian translation of
     RustyShelf's README.md, which was written from Boxshelf's README.md. -->
<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/brand/rustyshelf-logo-dark.svg">
  <img src="docs/brand/rustyshelf-logo-light.svg" alt="RustyShelf" width="520">
</picture>

**Ваша библиотека игр как полка, которую листают с дивана.**

[English](README.md) · **Русский** · [Português](README.pt-BR.md) · [Français](README.fr.md) · [Türkçe](README.tr.md)

[![Лицензия: GPL v3](https://img.shields.io/badge/%D0%BB%D0%B8%D1%86%D0%B5%D0%BD%D0%B7%D0%B8%D1%8F-GPL--3.0-blue)](LICENSE)
[![Платформы: Windows, Linux, Android](https://img.shields.io/badge/%D0%BF%D0%BB%D0%B0%D1%82%D1%84%D0%BE%D1%80%D0%BC%D1%8B-Windows%20%C2%B7%20Linux%20%C2%B7%20Android-informational)](#установка)
[![Build/release check](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml/badge.svg)](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml)
[![Rust + Bevy](https://img.shields.io/badge/app-Rust%20%2B%20Bevy-CE422B?logo=rust&logoColor=white)](docs/MANUAL.md#run-from-source)

</div>

Консольные игры в эмуляторах, игры из Steam и другие игры для ПК стоят рядом на полке, как настоящие коробки, и для
всего этого хватает одного геймпада.

> **В этой ветке нативное приложение на Rust.** Это альфа-версия, ещё не полная. Приложение на Electron, с которого оно
> переписывается (и его приложение для Android на Capacitor), находится в ветке
> [`archive/electron`](https://github.com/zearkreast/rustyshelf/tree/archive/electron). Оба читают одни и те же настройки и
> данные, так что настройка, сделанная в одном, работает в другом. Музей остаётся только в версии на Electron.

## Установка

Альфа-сборки для Windows и Android есть на [странице релизов](https://github.com/zearkreast/rustyshelf/releases). Или
запустите из исходников:

```bash
cargo run -p rustyshelf --release -- --windowed
```

В [руководстве](docs/MANUAL.md) (на английском) перечислено, [что уже работает](docs/MANUAL.md#what-works), описана сборка
для Android и [управление](docs/MANUAL.md#controls).

## Откуда взялся RustyShelf

RustyShelf — переименованный форк [(T)rustyShelf](https://github.com/GGStonks/t-rustyshelf) от GGStonks, который, в свою
очередь, является форком [Boxshelf](https://github.com/sassiestrhihno/boxshelf-beta) от sassiestrhihno. Большая часть
того, что умеет RustyShelf, — работа автора Boxshelf. То, что добавили форки, отмечено в руководстве меткой **[fork]**.

> **Нужен оригинальный Boxshelf?** Скачайте его со [страницы бета-версий Boxshelf](https://github.com/sassiestrhihno/boxshelf-beta/releases),
> а об ошибках и идеях сообщайте в [issues того проекта](https://github.com/sassiestrhihno/boxshelf-beta/issues/new/choose).

## Благодарности и лицензия

Источники обложек и руководств, шрифты, библиотеки и участники перечислены в
[разделе Credits английского README](README.md#credits).

RustyShelf — свободное программное обеспечение: его можно распространять и изменять на условиях GNU General Public
License версии 3 ([LICENSE](LICENSE)). Программа поставляется без каких-либо гарантий.

**Название и логотип.** Автор Boxshelf просит, чтобы форки использовали собственные название и логотип: GPL
распространяется на код, но не на название и логотип Boxshelf. Если вы выпускаете форк или изменённую версию RustyShelf,
пожалуйста, дайте ей другое название и логотип. Упоминание «основано на RustyShelf» приветствуется.
