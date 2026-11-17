<!-- Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): a Brazilian Portuguese
     translation of RustyShelf's README.md, which was written from Boxshelf's README.md. -->
<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/brand/rustyshelf-logo-dark.svg">
  <img src="docs/brand/rustyshelf-logo-light.svg" alt="RustyShelf" width="520">
</picture>

**Sua biblioteca de jogos como uma estante para navegar do sofá.**

[English](README.md) · [Русский](README.ru.md) · **Português** · [Français](README.fr.md) · [Türkçe](README.tr.md)

[![Licença: GPL v3](https://img.shields.io/badge/licen%C3%A7a-GPL--3.0-blue)](LICENSE)
[![Plataformas: Windows, Linux, Android](https://img.shields.io/badge/plataformas-Windows%20%C2%B7%20Linux%20%C2%B7%20Android-informational)](#instalação)
[![Build/release check](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml/badge.svg)](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml)
[![Rust + Bevy](https://img.shields.io/badge/app-Rust%20%2B%20Bevy-CE422B?logo=rust&logoColor=white)](docs/MANUAL.md#run-from-source)

</div>

Jogos de console emulados, jogos da Steam e outros jogos de PC ficam lado a lado como caixas de verdade em uma estante,
e se jogam com um só controle.

> **Esta branch é o aplicativo nativo, em Rust.** Parte do aplicativo Electron ainda não chegou a ele. O aplicativo Electron a partir do qual
> ele é reescrito (e o seu aplicativo Android, feito com Capacitor) está na branch
> [`archive/electron`](https://github.com/zearkreast/rustyshelf/tree/archive/electron). Os dois leem as mesmas configurações
> e os mesmos dados, então o que for configurado em um funciona no outro. O museu continua só no Electron.

## Instalação

As versões para Windows, Linux e Android estão na [página de releases](https://github.com/zearkreast/rustyshelf/releases):
`RustyShelf-<versão>-windows.exe` (sem instalador), `RustyShelf-<versão>-linux.AppImage` e
`RustyShelf-<versão>-android.apk`. Ou execute a partir do código-fonte:

```bash
cargo run -p rustyshelf --release -- --windowed
```

O [manual](docs/MANUAL.md) (em inglês) lista [o que já funciona](docs/MANUAL.md#what-works), a compilação para Android e
os [controles](docs/MANUAL.md#controls).

## De onde vem o RustyShelf

O RustyShelf é um fork renomeado do [(T)rustyShelf](https://github.com/GGStonks/t-rustyshelf), de GGStonks, que por sua
vez é um fork do [Boxshelf](https://github.com/sassiestrhihno/boxshelf-beta), de sassiestrhihno. A maior parte do que o
RustyShelf faz é trabalho do Boxshelf, do seu autor. O que os forks acrescentaram está marcado com **[fork]** no manual.

> **Quer o Boxshelf original?** Baixe-o na [página de betas do Boxshelf](https://github.com/sassiestrhihno/boxshelf-beta/releases)
> e relate erros e ideias nas [issues daquele projeto](https://github.com/sassiestrhihno/boxshelf-beta/issues/new/choose).

## Créditos e licença

As fontes de capas e manuais, as fontes tipográficas, as bibliotecas e os colaboradores estão listados na
[seção Credits do README em inglês](README.md#credits).

O RustyShelf é software livre: você pode redistribuí-lo e modificá-lo sob os termos da GNU General Public License,
versão 3 ([LICENSE](LICENSE)). Ele é fornecido sem nenhuma garantia.

**Nome e logotipo.** O autor do Boxshelf pede que os forks usem nome e logotipo próprios: a GPL cobre o código, não o nome
nem o logotipo do Boxshelf. Se você publicar um fork ou uma versão modificada do RustyShelf, dê a ela também um nome e um
logotipo diferentes. Dizer que é "baseado no RustyShelf" é bem-vindo.
