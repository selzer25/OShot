//! Lossless PNG shrinking with oxipng.

/// Recompresses a PNG without changing a single pixel. Returns the original
/// bytes if oxipng cannot make them smaller.
pub fn shrink_png(png: &[u8]) -> Result<Vec<u8>, String> {
    let mut opts = oxipng::Options::from_preset(2);
    opts.strip = oxipng::StripChunks::Safe;
    let out = oxipng::optimize_from_memory(png, &opts).map_err(|e| e.to_string())?;
    Ok(if out.len() < png.len() { out } else { png.to_vec() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use oshot_core::bitmap::Bitmap;
    use oshot_core::color::rgb;
    use oshot_core::format::ImageFormat;

    #[test]
    fn shrinks_without_changing_pixels() {
        let mut b = Bitmap::filled(300, 200, rgb(240, 240, 240));
        b.fill_rect(
            oshot_core::geometry::IRect {
                x: 20,
                y: 20,
                w: 80,
                h: 40,
            },
            rgb(10, 132, 255),
        );
        let png = crate::imageio::encode(b.view(), ImageFormat::Png, 1.0, 1).unwrap();
        let small = shrink_png(&png).unwrap();
        assert!(small.len() <= png.len());
        let (back, _) = crate::imageio::decode(&small).unwrap();
        assert_eq!(back.pixel(30, 30), Some(rgb(10, 132, 255)));
        assert_eq!(back.pixel(200, 150), Some(rgb(240, 240, 240)));
    }
}
