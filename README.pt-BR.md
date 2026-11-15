<!-- Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): a Brazilian Portuguese
     translation of RustyShelf's README.md, which was written from Boxshelf's README.md. -->
<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="renderer/brand/rustyshelf-logo-dark.svg">
  <img src="renderer/brand/rustyshelf-logo-light.svg" alt="RustyShelf" width="520">
</picture>

**Sua biblioteca de jogos como uma estante para navegar do sofá.**

[English](README.md) · [Русский](README.ru.md) · **Português** · [Français](README.fr.md) · [Türkçe](README.tr.md)

[![Licença: GPL v3](https://img.shields.io/badge/licen%C3%A7a-GPL--3.0-blue)](LICENSE)
[![Plataformas: Windows, Linux, Android](https://img.shields.io/badge/plataformas-Windows%20%C2%B7%20Linux%20%C2%B7%20Android-informational)](#instalação)
[![Build/release check](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml/badge.svg)](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml)
[![Electron](https://img.shields.io/badge/app-Electron-47848F?logo=electron&logoColor=white)](docs/MANUAL.md#run-from-source)
[![Rust + Bevy](https://img.shields.io/badge/reescrita-Rust%20%2B%20Bevy-CE422B?logo=rust&logoColor=white)](#versão-nativa-em-rust)

</div>

Jogos de console emulados, jogos da Steam e outros jogos de PC e aplicativos de streaming ficam lado a lado como caixas de
verdade em uma estante 3D. Puxe uma, abra a caixa, leia o manual, veja o disco entrar e jogue, tudo com um só controle.

## O que ele faz

- **Uma estante 3D de caixas de verdade.** Cada jogo fica na caixa do seu console, com frente, lombada e verso. Vire a
  caixa, tire-a da estante, abra, retire o disco, leia o manual.
- **Um controle para tudo.** Navegue, inicie um jogo, pause, salve e carregue estados, tire uma captura de tela e volte
  para a estante sem tocar no teclado.
- **Jogos de console pelos seus emuladores.** Dolphin, Cemu, Eden, PPSSPP, PCSX2, RPCS3, shadPS4, Vita3K, xemu, Xenia,
  Azahar e RetroArch para os sistemas retrô. O RustyShelf os encontra, ou você indica onde estão.
- **Jogos de PC na mesma estante.** Steam, Epic Games, GOG Galaxy, Ubisoft Connect, o app da EA, Xbox / Game Pass e uma
  pasta em que cada subpasta é um jogo. No Linux, jogos de Windows rodam com Wine ou Proton, sem a Steam.
- **Capas e manuais encontrados para você.** Frentes, versos, lombadas, rótulos de disco, descrições e manuais vêm de
  bancos de dados abertos. O Art Studio e o Manual Studio permitem escolher entre versões, parte por parte.
- **Um museu da sua coleção.** Cada sistema em uma parede de estantes, cada caixa no tamanho real, com quatro tipos de
  iluminação.
- **Extras.** RetroAchievements e conquistas da Steam, um aplicativo de celular que funciona como controle remoto,
  dezoito temas de cores e os seus próprios, modo de demonstração, música de menu e seus próprios modelos 3D de console.
- **Privado.** Sem contas, sem anúncios, sem telemetria. Tudo o que ele salva fica na própria pasta.

A interface está disponível em inglês, português do Brasil, francês e turco.

## Instalação

Baixe a versão mais recente na [página de releases](https://github.com/zearkreast/rustyshelf/releases), quando houver uma
publicada. Até lá, [execute a partir do código-fonte](docs/MANUAL.md#run-from-source).

| Plataforma | Arquivo | Como |
|---|---|---|
| **Windows** | `RustyShelf-<versão>-windows-portable.exe` | Coloque-o em uma pasta só dele e dê dois cliques. Se o Windows avisar sobre um fornecedor desconhecido, escolha **Mais informações → Executar assim mesmo**. |
| **Linux** | `RustyShelf-<versão>-linux.AppImage` | Rode `chmod +x` uma vez e depois inicie. As configurações ficam em `~/RustyShelf`. |
| **Android** | `RustyShelf-<versão>-android.apk` | Abra o arquivo no aparelho e permita a instalação quando o Android perguntar. |

Na primeira execução, o assistente de configuração oferece **Find automatically**: ele procura seus emuladores e pastas de
jogos, você confere cada sistema e então escolhe **Open RustyShelf**.

## Controles

| Ação | Teclado | Controle Xbox / PlayStation |
|---|---|---|
| Mover pela estante | ← → | Direcional / analógico esquerdo |
| Trocar de estante | ↑ ↓ | LT / RT |
| Jogar / abrir | Enter | A / Xis |
| Voltar | Esc | B / Círculo |
| Virar a caixa | F | Y / Triângulo |
| Detalhes | I | X / Quadrado |
| Inspecionar | V | Clique do analógico direito |
| Museu | M | Segurar View / Back |
| Configurações | S | Start |
| Menu Home durante um jogo | | Guide, ou segurar View / Back |

A lista completa está no [manual](docs/MANUAL.md#controls).

## Versão nativa em Rust

O RustyShelf está sendo reescrito como um aplicativo nativo em Rust, em `crates/`, com [Bevy](https://bevy.org/). Ele lê as
mesmas configurações e dados do aplicativo Electron, então o que for configurado em um funciona no outro. Ainda não é um
substituto: instale e use o aplicativo Electron. O manual lista [o que já funciona](docs/MANUAL.md#native-rust-version).

```bash
cargo run -p rustyshelf --release -- --windowed
```

## Documentação

O [manual](docs/MANUAL.md) cobre tudo em detalhes (em inglês):

- [Suas pastas](docs/MANUAL.md#your-folders): como os jogos são encontrados e notas por sistema
- [Na estante](docs/MANUAL.md#on-the-shelf): Inspecionar, Detalhes, Home, ordenação, o museu, modo de demonstração
- [Durante um jogo](docs/MANUAL.md#while-a-game-runs): o menu Home, estados salvos, os atalhos de cada emulador
- [Capas e manuais](docs/MANUAL.md#box-art-and-manuals) e [jogos de PC](docs/MANUAL.md#pc-games)
- [Configurações](docs/MANUAL.md#settings) e [extras](docs/MANUAL.md#extras): aplicativo de celular, conquistas, seus próprios modelos 3D
- [Privacidade](docs/MANUAL.md#privacy): cada serviço que ele acessa, quando e o que envia
- [Para desenvolvedores](docs/MANUAL.md#for-developers): testes e builds de release
- [Solução de problemas](docs/MANUAL.md#troubleshooting)

As notas sobre o Steam Deck estão em [docs/STEAM-DECK.md](docs/STEAM-DECK.md).

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
