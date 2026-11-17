use image::{DynamicImage, GenericImageView};

/// Splits a two-page scan into its left and right halves.
pub fn crop_both(img: &DynamicImage) -> (DynamicImage, DynamicImage) {
    let (w, h) = img.dimensions();
    let half = w / 2;
    let left = img.crop_imm(0, 0, half, h);
    let right = img.crop_imm(w - half, 0, half, h);
    (left, right)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgba, RgbaImage};

    #[test]
    fn test_crop_operation() {
        let mut img = RgbaImage::new(200, 100);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([100, 150, 200, 255]);
        }
        let dynamic = DynamicImage::ImageRgba8(img);
        let (left, right) = crop_both(&dynamic);
        assert_eq!(left.width(), 100);
        assert_eq!(left.height(), 100);
        assert_eq!(right.width(), 100);
        assert_eq!(right.height(), 100);
    }
}
