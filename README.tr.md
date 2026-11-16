<!-- Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): a Turkish translation of
     RustyShelf's README.md, which was written from Boxshelf's README.md. -->
<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/brand/rustyshelf-logo-dark.svg">
  <img src="docs/brand/rustyshelf-logo-light.svg" alt="RustyShelf" width="520">
</picture>

**Oyun kütüphaneniz, koltuktan gezdiğiniz bir raf olarak.**

[English](README.md) · [Русский](README.ru.md) · [Português](README.pt-BR.md) · [Français](README.fr.md) · **Türkçe**

[![Lisans: GPL v3](https://img.shields.io/badge/lisans-GPL--3.0-blue)](LICENSE)
[![Platformlar: Windows, Linux, Android](https://img.shields.io/badge/platformlar-Windows%20%C2%B7%20Linux%20%C2%B7%20Android-informational)](#kurulum)
[![Build/release check](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml/badge.svg)](https://github.com/zearkreast/rustyshelf/actions/workflows/build-release-check.yml)
[![Rust + Bevy](https://img.shields.io/badge/app-Rust%20%2B%20Bevy-CE422B?logo=rust&logoColor=white)](docs/MANUAL.md#run-from-source)

</div>

Emülatörle oynanan konsol oyunları, Steam ve diğer PC oyunları bir rafta gerçek kutular olarak yan yana durur; hepsi
tek bir oyun koluyla oynanır.

> **Bu dal Rust ile yazılmış yerel uygulamadır.** Alfa sürümündedir, henüz tamamlanmadı. Yeniden yazıldığı Electron
> uygulaması (ve Capacitor ile yapılmış Android uygulaması)
> [`archive/electron`](https://github.com/zearkreast/rustyshelf/tree/archive/electron) dalındadır. İkisi de aynı ayarları ve
> verileri okur; birinde yapılan kurulum diğerinde de çalışır. Müze yalnızca Electron'da kalıyor.

## Kurulum

Windows ve Android alfaları [sürümler sayfasında](https://github.com/zearkreast/rustyshelf/releases). Ya da kaynaktan
çalıştırın:

```bash
cargo run -p rustyshelf --release -- --windowed
```

[Kılavuz](docs/MANUAL.md) (İngilizce) [şimdiye kadar çalışanları](docs/MANUAL.md#what-works), Android derlemesini ve
[kontrolleri](docs/MANUAL.md#controls) anlatır.

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
