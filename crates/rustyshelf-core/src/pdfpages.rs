// Based on Boxshelf by sassiestrhihno (https://github.com/sassiestrhihno/boxshelf-source, GPL-3.0): ported from
// Boxshelf's renderer/pdf-pages.js, as RustyShelf has it.
//! A PDF manual drawn into JPEG pages, by the same size rule as the Electron app's converter.

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{render, PixmapSettings, RenderCache, RenderSettings};

/// JPEG quality of a page (renderer/pdf-pages.js: 0.86)
const QUALITY: u8 = 86;

/// The pages of a PDF as JPEGs, at most `max` (the last ones with `last`: the back cover), each scaled so its width,
/// or three quarters of its height when that's more, is `size` pixels.
pub fn pages(pdf: &[u8], size: u32, max: usize, last: bool) -> Result<Vec<Vec<u8>>, String> {
    let doc = Pdf::new(pdf.to_vec()).map_err(|e| format!("not a PDF that can be read ({e:?})"))?;
    let all = doc.pages();
    let n = all.len().min(max);
    let from = if last { all.len() - n } else { 0 };
    let (cache, settings) = (RenderCache::new(), InterpreterSettings::default());
    let mut out = Vec::with_capacity(n);
    for page in all.iter().skip(from).take(n) {
        let (w, h) = page.render_dimensions();
        let scale = size as f32 / w.max(h * 0.75).max(1.0);
        let pix = render(page, &cache, &settings, &RenderSettings::default(), &PixmapSettings { x_scale: scale, y_scale: scale, bg_color: WHITE });
        let (pw, ph) = (u32::from(pix.width()), u32::from(pix.height()));
        // (drawn on white: every pixel is opaque, so its premultiplied colour is its colour)
        let rgb: Vec<u8> = pix.data_as_u8_slice().as_chunks::<4>().0.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, QUALITY).encode(&rgb, pw, ph, image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
        out.push(jpeg);
    }
    Ok(out)
}

/// How many pages a PDF has, or None when it can't be read.
pub fn count(pdf: &[u8]) -> Option<usize> {
    Pdf::new(pdf.to_vec()).ok().map(|d| d.pages().len())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A made-up PDF of `n` pages, each a 300×400 JPEG picture filling a page of 600×800 points, with its offsets
    /// table built as a PDF reader expects.
    pub fn pdf_of(n: usize) -> Vec<u8> {
        let mut jpeg = Vec::new();
        let img = image::RgbImage::from_fn(300, 400, |x, y| image::Rgb([(x % 256) as u8, (y % 256) as u8, 120]));
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 90).encode_image(&img).unwrap();
        let mut objs: Vec<Vec<u8>> = Vec::new();
        let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 3 + i * 3)).collect();
        objs.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
        objs.push(format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")).into_bytes());
        for i in 0..n {
            let (content, image) = (4 + i * 3, 5 + i * 3);
            objs.push(format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 800] /Resources << /XObject << /Im0 {image} 0 R >> >> /Contents {content} 0 R >>").into_bytes());
            let draw = b"q 600 0 0 800 0 0 cm /Im0 Do Q";
            let mut c = format!("<< /Length {} >>\nstream\n", draw.len()).into_bytes();
            c.extend_from_slice(draw);
            c.extend_from_slice(b"\nendstream");
            objs.push(c);
            let mut im = format!("<< /Type /XObject /Subtype /Image /Width 300 /Height 400 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /DCTDecode /Length {} >>\nstream\n", jpeg.len()).into_bytes();
            im.extend_from_slice(&jpeg);
            im.extend_from_slice(b"\nendstream");
            objs.push(im);
        }
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut at = Vec::new();
        for (i, o) in objs.iter().enumerate() {
            at.push(out.len());
            out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
            out.extend_from_slice(o);
            out.extend_from_slice(b"\nendobj\n");
        }
        let xref = out.len();
        out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
        for a in at {
            out.extend_from_slice(format!("{a:010} 00000 n \n").as_bytes());
        }
        out.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objs.len() + 1).as_bytes());
        out
    }

    #[test]
    fn a_pdf_becomes_jpeg_pages_of_the_converters_size() {
        let pdf = pdf_of(3);
        let got = pages(&pdf, 1600, 80, false).unwrap();
        assert_eq!(got.len(), 3);
        let img = image::load_from_memory(&got[0]).unwrap();
        // 600×800 points: 800 × 0.75 = 600, so the width is the size
        assert_eq!((img.width(), img.height()), (1600, 2133));
        // the picture is drawn, not a blank page
        let px = img.to_rgb8().get_pixel(800, 1000).0;
        assert!(px != [255, 255, 255], "{px:?}");
        assert_eq!(pages(&pdf, 900, 1, true).unwrap().len(), 1);
        assert!(pages(b"not a pdf", 1600, 80, false).is_err());
    }
}
