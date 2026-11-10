//! WebP output through libwebp (ImageIO reads WebP but cannot write it).

use oshot_core::bitmap::BitmapRef;

pub fn encode(bitmap: BitmapRef, quality: f32, lossless: bool) -> Result<Vec<u8>, String> {
    let mut out: *mut u8 = std::ptr::null_mut();
    let stride = bitmap.width as i32 * 4;
    // SAFETY: the buffer holds width * height BGRA pixels with this stride;
    // libwebp allocates `out`, which we free with WebPFree.
    unsafe {
        let len = if lossless {
            libwebp_sys::WebPEncodeLosslessBGRA(
                bitmap.data.as_ptr(),
                bitmap.width as i32,
                bitmap.height as i32,
                stride,
                &mut out,
            )
        } else {
            libwebp_sys::WebPEncodeBGRA(
                bitmap.data.as_ptr(),
                bitmap.width as i32,
                bitmap.height as i32,
                stride,
                (quality.clamp(0.0, 1.0) * 100.0).round(),
                &mut out,
            )
        };
        if len == 0 || out.is_null() {
            return Err("Could not encode WebP".into());
        }
        let bytes = std::slice::from_raw_parts(out, len).to_vec();
        libwebp_sys::WebPFree(out as *mut std::ffi::c_void);
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oshot_core::bitmap::Bitmap;
    use oshot_core::color::rgb;
    use oshot_core::format::ImageFormat;

    #[test]
    fn lossless_round_trips_through_imageio() {
        let b = Bitmap::filled(20, 10, rgb(12, 34, 56));
        let bytes = encode(b.view(), 1.0, true).unwrap();
        assert_eq!(ImageFormat::detect(&bytes), Some(ImageFormat::WebP));
        let (back, _) = crate::imageio::decode(&bytes).unwrap();
        assert_eq!(back.pixel(5, 5), Some(rgb(12, 34, 56)));
    }
}
