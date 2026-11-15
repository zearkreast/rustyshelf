<!-- Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): a Russian translation of
     RustyShelf's README.md, which was written from Boxshelf's README.md. -->
<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="renderer/brand/rustyshelf-logo-dark.svg">
  <img src="renderer/brand/rustyshelf-logo-light.svg" alt="RustyShelf" width="520">
</picture>

**Ваша библиотека игр как полка, которую листают с дивана.**

[English](README.md) · **Русский** · [Português](README.pt-BR.md) · [Français](README.fr.md) · [Türkçe](README.tr.md)

[![Лицензия: GPL v3](https://img.shields.io/badge/%D0%BB%D0%B8%D1%86%D0%B5%D0%BD%D0%B7%D0%B8%D1%8F-GPL--3.0-blue)](LICENSE)
[![Платформы: Windows, Linux, Android](https://img.shields.io/badge/%D0%BF%D0%BB%D0%B0%D1%82%D1%84%D0%BE%D1%80%D0%BC%D1%8B-Windows%20%C2%B7%20Linux%20%C2%B7%20Android-informational)](#установка)
[![Build/release check](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml/badge.svg)](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml)
[![Electron](https://img.shields.io/badge/app-Electron-47848F?logo=electron&logoColor=white)](docs/MANUAL.md#run-from-source)
[![Rust + Bevy](https://img.shields.io/badge/rewrite-Rust%20%2B%20Bevy-CE422B?logo=rust&logoColor=white)](#нативная-версия-на-rust)

</div>

Консольные игры в эмуляторах, игры из Steam и другие игры для ПК, а также стриминговые приложения стоят рядом на
3D-полке, как настоящие коробки. Достаньте одну, откройте футляр, полистайте руководство, посмотрите, как вставляется
диск, и играйте. Для всего этого хватает одного геймпада.

## Что умеет

- **3D-полка с настоящими коробками.** Каждая игра стоит в футляре своей консоли: с обложкой, корешком и оборотом.
  Переверните коробку, снимите с полки, откройте, выньте диск, прочитайте руководство.
- **Всё с одного геймпада.** Листайте полку, запускайте игру, ставьте на паузу, сохраняйте и загружайте состояния,
  делайте снимки экрана и возвращайтесь на полку, не трогая клавиатуру.
- **Консольные игры через ваши эмуляторы.** Dolphin, Cemu, Eden, PPSSPP, PCSX2, RPCS3, shadPS4, Vita3K, xemu, Xenia, Azahar
  и RetroArch для ретросистем. RustyShelf находит их сам, или вы указываете путь.
- **Игры для ПК на той же полке.** Steam, Epic Games, GOG Galaxy, Ubisoft Connect, приложение EA, Xbox / Game Pass и папка,
  в которой каждая вложенная папка считается игрой. В Linux игры для Windows запускаются через Wine или Proton без Steam.
- **Обложки и руководства находятся сами.** Лицевые и обратные стороны, корешки, этикетки дисков, описания и руководства
  берутся из открытых баз данных. В Art Studio и Manual Studio можно выбирать между вариантами, по частям.
- **Музей вашей коллекции.** Каждая система на стене из полок, каждый футляр в настоящем размере, четыре вида освещения.
- **Дополнительно.** RetroAchievements и достижения Steam, приложение-компаньон для телефона, которое работает как пульт,
  восемнадцать цветовых тем и ваши собственные, демонстрационный режим, музыка в меню и свои 3D-модели консолей.
- **Приватность.** Ни учётных записей, ни рекламы, ни телеметрии. Всё, что программа сохраняет, остаётся в её папке.

Интерфейс доступен на английском, бразильском португальском, французском и турецком языках.

## Установка

Скачайте свежую сборку со [страницы релизов](https://github.com/zearkreast/rustyshelf/releases), когда выйдет первый
релиз. До тех пор программу можно [запустить из исходников](docs/MANUAL.md#run-from-source).

| Платформа | Файл | Как |
|---|---|---|
| **Windows** | `RustyShelf-<версия>-windows-portable.exe` | Положите файл в отдельную папку и запустите двойным щелчком. Если Windows предупредит о неизвестном издателе, выберите **Подробнее → Выполнить в любом случае**. |
| **Linux** | `RustyShelf-<версия>-linux.AppImage` | Один раз выполните `chmod +x`, затем запустите. Настройки хранятся в `~/RustyShelf`. |
| **Android** | `RustyShelf-<версия>-android.apk` | Откройте файл на устройстве и разрешите установку, когда Android спросит. |

При первом запуске мастер настройки предлагает **Find automatically**: он ищет эмуляторы и папки с играми, вы
просматриваете каждую систему и выбираете **Open RustyShelf**.

## Управление

| Действие | Клавиатура | Геймпад Xbox / PlayStation |
|---|---|---|
| Двигаться по полке | ← → | Крестовина / левый стик |
| Сменить полку | ↑ ↓ | LT / RT |
| Играть / открыть | Enter | A / Крест |
| Назад | Esc | B / Круг |
| Перевернуть коробку | F | Y / Треугольник |
| Сведения | I | X / Квадрат |
| Осмотр | V | Нажатие правого стика |
| Музей | M | Удерживать View / Back |
| Настройки | S | Start |
| Меню Home во время игры | | Guide или удерживать View / Back |

Полный список есть в [руководстве](docs/MANUAL.md#controls).

## Нативная версия на Rust

RustyShelf переписывается как нативное приложение на Rust, в папке `crates/`, на движке [Bevy](https://bevy.org/). Оно
читает те же настройки и данные, что и приложение на Electron, так что настройка, сделанная в одном, работает в другом.
Это пока не замена: устанавливайте и используйте приложение на Electron. В руководстве перечислено,
[что уже работает](docs/MANUAL.md#native-rust-version).

```bash
cargo run -p rustyshelf --release -- --windowed
```

## Документация

[Руководство](docs/MANUAL.md) описывает всё подробно (на английском):

- [Ваши папки](docs/MANUAL.md#your-folders): как находятся игры и заметки по каждой системе
- [На полке](docs/MANUAL.md#on-the-shelf): осмотр, сведения, Home, сортировка, музей, демонстрационный режим
- [Во время игры](docs/MANUAL.md#while-a-game-runs): меню Home, сохранения состояний, горячие клавиши каждого эмулятора
- [Обложки и руководства](docs/MANUAL.md#box-art-and-manuals) и [игры для ПК](docs/MANUAL.md#pc-games)
- [Настройки](docs/MANUAL.md#settings) и [дополнения](docs/MANUAL.md#extras): компаньон для телефона, достижения, свои 3D-модели
- [Приватность](docs/MANUAL.md#privacy): каждый сервис, к которому программа обращается, когда и что она отправляет
- [Разработчикам](docs/MANUAL.md#for-developers): тесты и релизные сборки
- [Решение проблем](docs/MANUAL.md#troubleshooting)

Заметки о Steam Deck находятся в [docs/STEAM-DECK.md](docs/STEAM-DECK.md).

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
