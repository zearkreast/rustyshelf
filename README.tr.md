<!-- Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): a Turkish translation of
     RustyShelf's README.md, which was written from Boxshelf's README.md. -->
<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="renderer/brand/rustyshelf-logo-dark.svg">
  <img src="renderer/brand/rustyshelf-logo-light.svg" alt="RustyShelf" width="520">
</picture>

**Oyun kütüphaneniz, koltuktan gezdiğiniz bir raf olarak.**

[English](README.md) · [Русский](README.ru.md) · [Português](README.pt-BR.md) · [Français](README.fr.md) · **Türkçe**

[![Lisans: GPL v3](https://img.shields.io/badge/lisans-GPL--3.0-blue)](LICENSE)
[![Platformlar: Windows, Linux, Android](https://img.shields.io/badge/platformlar-Windows%20%C2%B7%20Linux%20%C2%B7%20Android-informational)](#kurulum)
[![Build/release check](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml/badge.svg)](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml)
[![Electron](https://img.shields.io/badge/uygulama-Electron-47848F?logo=electron&logoColor=white)](docs/MANUAL.md#run-from-source)
[![Rust + Bevy](https://img.shields.io/badge/yeniden%20yaz%C4%B1m-Rust%20%2B%20Bevy-CE422B?logo=rust&logoColor=white)](#yerel-rust-sürümü)

</div>

Emülatörle oynanan konsol oyunları, Steam ve diğer PC oyunları ile yayın uygulamaları, 3D bir rafta gerçek kutular olarak
yan yana durur. Birini çekin, kutusunu açın, kılavuzunu okuyun, diskin takılışını izleyin ve oynayın; hepsi tek bir oyun
koluyla.

## Neler yapar

- **Gerçek kutulardan oluşan 3D bir raf.** Her oyun kendi konsolunun kutusunda durur: ön yüzü, sırtı ve arka yüzüyle.
  Çevirin, raftan alın, açın, diski çıkarın, kılavuzu okuyun.
- **Her şey tek oyun koluyla.** Klavyeye dokunmadan gezin, oyunu başlatın, duraklatın, durum kaydedip yükleyin, ekran
  görüntüsü alın ve rafa dönün.
- **Konsol oyunları kendi emülatörlerinizle.** Dolphin, Cemu, Eden, PPSSPP, PCSX2, RPCS3, shadPS4, Vita3K, xemu, Xenia,
  Azahar ve retro sistemler için RetroArch. RustyShelf bunları kendisi bulur ya da yerlerini siz gösterirsiniz.
- **PC oyunları aynı rafta.** Steam, Epic Games, GOG Galaxy, Ubisoft Connect, EA uygulaması, Xbox / Game Pass ve her alt
  klasörü bir oyun olan bir klasör. Linux'ta Windows oyunları Steam olmadan Wine veya Proton ile çalışır.
- **Kutu resimleri ve kılavuzlar sizin için bulunur.** Ön yüzler, arka yüzler, sırtlar, disk etiketleri, açıklamalar ve
  kılavuzlar açık veritabanlarından gelir. Art Studio ve Manual Studio ile sürümler arasından parça parça seçim yaparsınız.
- **Koleksiyonunuzun müzesi.** Her sistem bir raf duvarında, her kutu gerçek boyutunda, dört farklı aydınlatmayla.
- **Ekstralar.** RetroAchievements ve Steam başarımları, kumanda görevi gören telefon uygulaması, on sekiz renk teması ve
  kendi temalarınız, tanıtım modu, menü müziği ve kendi 3D konsol modelleriniz.
- **Gizlilik.** Hesap yok, reklam yok, telemetri yok. Kaydettiği her şey kendi klasöründe kalır.

Arayüz İngilizce, Brezilya Portekizcesi, Fransızca ve Türkçe olarak kullanılabilir.

## Kurulum

En yeni derlemeyi, bir sürüm yayınlandığında [sürümler sayfasından](https://github.com/zearkreast/rustyshelf/releases)
indirin. O zamana kadar [kaynaktan çalıştırabilirsiniz](docs/MANUAL.md#run-from-source).

| Platform | Dosya | Nasıl |
|---|---|---|
| **Windows** | `RustyShelf-<sürüm>-windows-portable.exe` | Kendine ait bir klasöre koyup çift tıklayın. Windows bilinmeyen yayıncı uyarısı verirse **Ek bilgi → Yine de çalıştır** seçin. |
| **Linux** | `RustyShelf-<sürüm>-linux.AppImage` | Bir kez `chmod +x` ile çalıştırılabilir yapın, sonra başlatın. Ayarlar `~/RustyShelf` klasöründe tutulur. |
| **Android** | `RustyShelf-<sürüm>-android.apk` | Dosyayı cihazda açın ve Android sorduğunda kuruluma izin verin. |

İlk açılışta kurulum sihirbazı **Find automatically** seçeneğini sunar: emülatörlerinizi ve oyun klasörlerinizi arar, siz
her sistemi gözden geçirirsiniz, sonra **Open RustyShelf** seçersiniz.

## Kontroller

| İşlem | Klavye | Xbox / PlayStation kolu |
|---|---|---|
| Rafta ilerle | ← → | D-pad / sol çubuk |
| Raf değiştir | ↑ ↓ | LT / RT |
| Oyna / aç | Enter | A / Çarpı |
| Geri | Esc | B / Daire |
| Kutuyu çevir | F | Y / Üçgen |
| Ayrıntılar | I | X / Kare |
| İncele | V | Sağ çubuğa basma |
| Müze | M | View / Back basılı tutma |
| Ayarlar | S | Start |
| Oyun çalışırken Home menüsü | | Guide ya da View / Back basılı tutma |

Tam liste [kılavuzda](docs/MANUAL.md#controls).

## Yerel Rust sürümü

RustyShelf, `crates/` klasöründe [Bevy](https://bevy.org/) ile Rust'ta yerel bir uygulama olarak yeniden yazılıyor.
Electron uygulamasıyla aynı ayarları ve verileri okur; birinde yapılan kurulum diğerinde de çalışır. Henüz onun yerini
tutmuyor: Electron uygulamasını kurup kullanın. Kılavuzda [şimdiye kadar çalışanların](docs/MANUAL.md#native-rust-version)
listesi var.

```bash
cargo run -p rustyshelf --release -- --windowed
```

## Belgeler

[Kılavuz](docs/MANUAL.md) her şeyi ayrıntısıyla anlatır (İngilizce):

- [Klasörleriniz](docs/MANUAL.md#your-folders): oyunların nasıl bulunduğu ve sistem başına notlar
- [Rafta](docs/MANUAL.md#on-the-shelf): İncele, Ayrıntılar, Home, sıralama, müze, tanıtım modu
- [Oyun çalışırken](docs/MANUAL.md#while-a-game-runs): Home menüsü, durum kayıtları, her emülatörün kısayolları
- [Kutu resimleri ve kılavuzlar](docs/MANUAL.md#box-art-and-manuals) ile [PC oyunları](docs/MANUAL.md#pc-games)
- [Ayarlar](docs/MANUAL.md#settings) ve [Ekstralar](docs/MANUAL.md#extras): telefon uygulaması, başarımlar, kendi 3D modelleriniz
- [Gizlilik](docs/MANUAL.md#privacy): bağlandığı her servis, ne zaman bağlandığı ve ne gönderdiği
- [Geliştiriciler için](docs/MANUAL.md#for-developers): testler ve sürüm derlemeleri
- [Sorun giderme](docs/MANUAL.md#troubleshooting)

Steam Deck notları [docs/STEAM-DECK.md](docs/STEAM-DECK.md) dosyasında.

## RustyShelf nereden geliyor

RustyShelf, GGStonks'un [(T)rustyShelf](https://github.com/GGStonks/t-rustyshelf) projesinin yeniden adlandırılmış bir
fork'udur; o da sassiestrhihno'nun [Boxshelf](https://github.com/sassiestrhihno/boxshelf-beta) projesinin fork'udur.
RustyShelf'in yaptıklarının çoğu Boxshelf'in, yani yazarının emeğidir. Fork'ların ekledikleri kılavuzda **[fork]** ile
işaretlidir.

> **Özgün Boxshelf'i mi istiyorsunuz?** [Boxshelf beta sayfasından](https://github.com/sassiestrhihno/boxshelf-beta/releases)
> indirin; hatalarını ve fikirlerinizi [o projenin issue sayfasına](https://github.com/sassiestrhihno/boxshelf-beta/issues/new/choose)
> bildirin.

## Emeği geçenler ve lisans

Kutu resmi ve kılavuz kaynakları, yazı tipleri, kütüphaneler ve katkıda bulunanlar
[İngilizce README'nin Credits bölümünde](README.md#credits) listelenir.

RustyShelf özgür yazılımdır: GNU Genel Kamu Lisansı sürüm 3 ([LICENSE](LICENSE)) koşullarıyla dağıtabilir ve
değiştirebilirsiniz. Hiçbir garanti verilmez.

**Ad ve logo.** Boxshelf'in yazarı, fork'ların kendi adını ve logosunu kullanmasını ister: GPL kodu kapsar, Boxshelf adını
ve logosunu kapsamaz. RustyShelf'in bir fork'unu ya da değiştirilmiş sürümünü yayınlarsanız lütfen ona da farklı bir ad ve
logo verin. "RustyShelf tabanlı" demeniz memnuniyetle karşılanır.
