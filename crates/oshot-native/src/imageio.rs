//! Decoding and encoding through ImageIO, plus the converter pipeline.

#![allow(non_upper_case_globals)]

use oshot_core::bitmap::{Bitmap, BitmapRef};
use oshot_core::format::{ImageFormat, fit_within};

use crate::cf::{self, CFStringRef, CFTypeRef, Cf};
use crate::cg;

#[link(name = "ImageIO", kind = "framework")]
unsafe extern "C" {
    fn CGImageSourceCreateWithData(data: CFTypeRef, options: CFTypeRef) -> CFTypeRef;
    fn CGImageSourceGetCount(src: CFTypeRef) -> usize;
    fn CGImageSourceCopyPropertiesAtIndex(src: CFTypeRef, index: usize, options: CFTypeRef) -> CFTypeRef;
    fn CGImageSourceCreateThumbnailAtIndex(src: CFTypeRef, index: usize, options: CFTypeRef) -> CFTypeRef;
    fn CGImageDestinationCreateWithData(
        data: CFTypeRef,
        uti: CFStringRef,
        count: usize,
        options: CFTypeRef,
    ) -> CFTypeRef;
    fn CGImageDestinationAddImage(dest: CFTypeRef, image: CFTypeRef, props: CFTypeRef);
    fn CGImageDestinationSetProperties(dest: CFTypeRef, props: CFTypeRef);
    fn CGImageDestinationFinalize(dest: CFTypeRef) -> bool;

    static kCGImageSourceCreateThumbnailFromImageAlways: CFStringRef;
    static kCGImageSourceThumbnailMaxPixelSize: CFStringRef;
    static kCGImageSourceCreateThumbnailWithTransform: CFStringRef;
    static kCGImageSourceShouldCacheImmediately: CFStringRef;
    static kCGImagePropertyPixelWidth: CFStringRef;
    static kCGImagePropertyPixelHeight: CFStringRef;
    static kCGImagePropertyDPIWidth: CFStringRef;
    static kCGImagePropertyDPIHeight: CFStringRef;
    static kCGImagePropertyOrientation: CFStringRef;
    static kCGImageDestinationLossyCompressionQuality: CFStringRef;
    static kCGImagePropertyGIFDictionary: CFStringRef;
    static kCGImagePropertyGIFDelayTime: CFStringRef;
    static kCGImagePropertyGIFLoopCount: CFStringRef;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Info {
    pub format: Option<ImageFormat>,
    pub width: u32,
    pub height: u32,
    /// Pixels per point implied by the DPI (144 DPI is 2x).
    pub scale: u32,
    pub frames: usize,
}

fn source(bytes: &[u8]) -> Result<Cf, String> {
    let data = cf::data(bytes);
    // SAFETY: valid CFData; null options.
    Cf::wrap(unsafe { CGImageSourceCreateWithData(data.ptr(), std::ptr::null()) })
        .ok_or_else(|| "Not an image macOS can read".to_string())
}

fn property(props: &Cf, key: CFStringRef) -> Option<f64> {
    // SAFETY: valid dictionary and key; the value is borrowed from it.
    unsafe { cf::get_f64(cf::CFDictionaryGetValue(props.ptr(), key)) }
}

pub fn info(bytes: &[u8]) -> Result<Info, String> {
    let src = source(bytes)?;
    // SAFETY: valid source.
    let props = Cf::wrap(unsafe { CGImageSourceCopyPropertiesAtIndex(src.ptr(), 0, std::ptr::null()) })
        .ok_or("Image has no readable frames")?;
    // SAFETY: framework constants.
    let (wk, hk, dk, ok) = unsafe {
        (
            kCGImagePropertyPixelWidth,
            kCGImagePropertyPixelHeight,
            kCGImagePropertyDPIWidth,
            kCGImagePropertyOrientation,
        )
    };
    let mut width = property(&props, wk).unwrap_or(0.0) as u32;
    let mut height = property(&props, hk).unwrap_or(0.0) as u32;
    // EXIF orientations 5-8 rotate by 90 degrees, so the visible size swaps.
    if property(&props, ok).is_some_and(|o| o >= 5.0) {
        std::mem::swap(&mut width, &mut height);
    }
    let dpi = property(&props, dk).unwrap_or(72.0);
    Ok(Info {
        format: ImageFormat::detect(bytes),
        width,
        height,
        scale: ((dpi / 72.0).round() as u32).max(1),
        // SAFETY: valid source.
        frames: unsafe { CGImageSourceGetCount(src.ptr()) },
    })
}

/// Decodes the first frame, applying EXIF orientation, optionally limited
/// to `max_side` pixels.
fn decode_image(src: &Cf, max_side: u32) -> Result<Cf, String> {
    let max = cf::number_i32(max_side as i32);
    // SAFETY: framework constants and booleans.
    let opts = unsafe {
        cf::dict(&[
            (kCGImageSourceCreateThumbnailFromImageAlways, cf::kCFBooleanTrue),
            (kCGImageSourceCreateThumbnailWithTransform, cf::kCFBooleanTrue),
            (kCGImageSourceShouldCacheImmediately, cf::kCFBooleanTrue),
            (kCGImageSourceThumbnailMaxPixelSize, max.ptr()),
        ])
    };
    // SAFETY: valid source and options.
    Cf::wrap(unsafe { CGImageSourceCreateThumbnailAtIndex(src.ptr(), 0, opts.ptr()) })
        .ok_or_else(|| "Could not decode image".to_string())
}

/// Decodes to BGRA. Returns the bitmap and its pixels-per-point scale.
pub fn decode(bytes: &[u8]) -> Result<(Bitmap, u32), String> {
    let meta = info(bytes)?;
    let src = source(bytes)?;
    let image = decode_image(&src, meta.width.max(meta.height).max(1))?;
    let bmp = // SAFETY: `image` is a CGImage this function owns.
        unsafe { cg::bitmap_from_image(image.ptr()) }.ok_or("Could not draw image")?;
    Ok((bmp, meta.scale))
}

fn destination(format: ImageFormat, frames: usize) -> Result<(Cf, Cf), String> {
    // SAFETY: a mutable CFData with no size limit.
    let out = Cf::wrap(unsafe { cf::CFDataCreateMutable(std::ptr::null(), 0) }).expect("CFMutableData");
    let uti = cf::string(format.uti());
    // SAFETY: valid data and type.
    let dest = Cf::wrap(unsafe { CGImageDestinationCreateWithData(out.ptr(), uti.ptr(), frames, std::ptr::null()) })
        .ok_or_else(|| format!("macOS cannot write {}", format.label()))?;
    Ok((out, dest))
}

fn image_props(format: ImageFormat, quality: f32, scale: u32) -> Cf {
    let q = cf::number_f64(quality.clamp(0.0, 1.0) as f64);
    let dpi = cf::number_f64(72.0 * scale.max(1) as f64);
    // SAFETY: framework constants.
    let (qk, dw, dh) = unsafe {
        (
            kCGImageDestinationLossyCompressionQuality,
            kCGImagePropertyDPIWidth,
            kCGImagePropertyDPIHeight,
        )
    };
    let mut pairs = vec![(dw, dpi.ptr()), (dh, dpi.ptr())];
    if format.is_lossy() {
        pairs.push((qk, q.ptr()));
    }
    cf::dict(&pairs)
}

fn encode_image(
    image: &Cf,
    width: u32,
    height: u32,
    format: ImageFormat,
    quality: f32,
    lossless: bool,
    scale: u32,
) -> Result<Vec<u8>, String> {
    if format == ImageFormat::WebP {
        let bmp = // SAFETY: `image` is a CGImage this function owns.
        unsafe { cg::bitmap_from_image(image.ptr()) }.ok_or("Could not draw image")?;
        return crate::webp::encode(bmp.view(), quality, lossless);
    }
    let _ = (width, height);
    let (out, dest) = destination(format, 1)?;
    let props = image_props(format, quality, scale);
    // SAFETY: valid destination, image and properties.
    unsafe { CGImageDestinationAddImage(dest.ptr(), image.ptr(), props.ptr()) };
    // SAFETY: valid destination.
    if !unsafe { CGImageDestinationFinalize(dest.ptr()) } {
        return Err(format!("Could not encode {}", format.label()));
    }
    Ok(cf::data_bytes(&out))
}

/// Encodes a bitmap. `scale` records Retina captures as 144 DPI so they open
/// at the right size elsewhere.
pub fn encode(bitmap: BitmapRef, format: ImageFormat, quality: f32, scale: u32) -> Result<Vec<u8>, String> {
    if format == ImageFormat::WebP {
        return crate::webp::encode(bitmap, quality, false);
    }
    let image = cg::image_from_bitmap(bitmap).ok_or("Could not create image")?;
    encode_image(&image, bitmap.width, bitmap.height, format, quality, false, scale)
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConvertOptions {
    pub format: ImageFormat,
    pub quality: f32,
    pub lossless: bool,
    pub max_dimension: Option<u32>,
    pub downscale_retina: bool,
    pub strip_metadata: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Converted {
    pub bytes: Vec<u8>,
    pub width: u32,
    pub height: u32,
}

/// Converts encoded image bytes to another format.
///
/// Metadata: ImageIO only writes the properties passed in, so stripping is
/// the natural path; keeping it copies the source properties across (minus
/// orientation, which decoding has already applied).
pub fn convert(input: &[u8], opts: &ConvertOptions) -> Result<Converted, String> {
    let meta = info(input)?;
    if meta.width == 0 || meta.height == 0 {
        return Err("Image has no size".into());
    }
    let (mut w, mut h) = (meta.width, meta.height);
    let mut scale = meta.scale;
    if opts.downscale_retina && meta.scale > 1 {
        w = (w / meta.scale).max(1);
        h = (h / meta.scale).max(1);
        scale = 1;
    }
    if let Some(max) = opts.max_dimension {
        (w, h) = fit_within(w, h, max);
    }
    let src = source(input)?;
    let image = decode_image(&src, w.max(h))?;
    // SAFETY: valid image.
    let (w, h) = unsafe {
        (
            cg::CGImageGetWidth(image.ptr()) as u32,
            cg::CGImageGetHeight(image.ptr()) as u32,
        )
    };

    if opts.format == ImageFormat::WebP || opts.strip_metadata {
        let bytes = encode_image(&image, w, h, opts.format, opts.quality, opts.lossless, scale)?;
        return Ok(Converted {
            bytes,
            width: w,
            height: h,
        });
    }

    let (out, dest) = destination(opts.format, 1)?;
    // SAFETY: valid source; the copy is mutable so we can edit it.
    let props = unsafe {
        let original =
            Cf::wrap(CGImageSourceCopyPropertiesAtIndex(src.ptr(), 0, std::ptr::null())).ok_or("No properties")?;
        let props =
            Cf::wrap(cf::CFDictionaryCreateMutableCopy(std::ptr::null(), 0, original.ptr())).ok_or("No properties")?;
        cf::CFDictionaryRemoveValue(props.ptr(), kCGImagePropertyOrientation);
        cf::CFDictionaryRemoveValue(props.ptr(), kCGImagePropertyPixelWidth);
        cf::CFDictionaryRemoveValue(props.ptr(), kCGImagePropertyPixelHeight);
        if opts.format.is_lossy() {
            let q = cf::number_f64(opts.quality.clamp(0.0, 1.0) as f64);
            cf::CFDictionarySetValue(props.ptr(), kCGImageDestinationLossyCompressionQuality, q.ptr());
        }
        props
    };
    // SAFETY: valid destination, image and properties.
    unsafe { CGImageDestinationAddImage(dest.ptr(), image.ptr(), props.ptr()) };
    // SAFETY: valid destination.
    if !unsafe { CGImageDestinationFinalize(dest.ptr()) } {
        return Err(format!("Could not encode {}", opts.format.label()));
    }
    Ok(Converted {
        bytes: cf::data_bytes(&out),
        width: w,
        height: h,
    })
}

/// Two-or-more-frame looping GIF, `delay` seconds per frame.
pub fn encode_gif(frames: &[BitmapRef], delay: f32) -> Result<Vec<u8>, String> {
    let (out, dest) = destination(ImageFormat::Gif, frames.len())?;
    let loop_count = cf::number_i32(0);
    let delay = cf::number_f64(delay as f64);
    // SAFETY: framework constants and valid dictionaries.
    unsafe {
        let file_gif = cf::dict(&[(kCGImagePropertyGIFLoopCount, loop_count.ptr())]);
        let file_props = cf::dict(&[(kCGImagePropertyGIFDictionary, file_gif.ptr())]);
        CGImageDestinationSetProperties(dest.ptr(), file_props.ptr());
        let frame_gif = cf::dict(&[(kCGImagePropertyGIFDelayTime, delay.ptr())]);
        let frame_props = cf::dict(&[(kCGImagePropertyGIFDictionary, frame_gif.ptr())]);
        for f in frames {
            let image = cg::image_from_bitmap(*f).ok_or("Could not create frame")?;
            CGImageDestinationAddImage(dest.ptr(), image.ptr(), frame_props.ptr());
        }
        if !CGImageDestinationFinalize(dest.ptr()) {
            return Err("Could not encode GIF".into());
        }
    }
    Ok(cf::data_bytes(&out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use oshot_core::color::rgb;

    fn sample() -> Bitmap {
        let mut b = Bitmap::filled(64, 32, rgb(10, 132, 255));
        b.set_pixel(0, 0, rgb(255, 0, 0));
        b
    }

    #[test]
    fn png_round_trip_is_lossless() {
        let bytes = encode(sample().view(), ImageFormat::Png, 1.0, 2).unwrap();
        assert_eq!(ImageFormat::detect(&bytes), Some(ImageFormat::Png));
        let i = info(&bytes).unwrap();
        assert_eq!((i.width, i.height, i.scale), (64, 32, 2));
        let (back, scale) = decode(&bytes).unwrap();
        assert_eq!(scale, 2);
        assert_eq!(back.pixel(0, 0), Some(rgb(255, 0, 0)));
        assert_eq!(back.pixel(10, 10), Some(rgb(10, 132, 255)));
    }

    #[test]
    fn converts_to_every_writable_format() {
        let png = encode(sample().view(), ImageFormat::Png, 1.0, 1).unwrap();
        for format in ImageFormat::WRITABLE {
            let opts = ConvertOptions {
                format,
                quality: 0.8,
                lossless: false,
                max_dimension: None,
                downscale_retina: false,
                strip_metadata: true,
            };
            let out = convert(&png, &opts).unwrap_or_else(|e| panic!("{format:?}: {e}"));
            assert_eq!(ImageFormat::detect(&out.bytes), Some(format), "{format:?} signature");
            assert_eq!((out.width, out.height), (64, 32));
            let back = info(&out.bytes).unwrap();
            assert_eq!((back.width, back.height), (64, 32), "{format:?} readable");
        }
    }

    #[test]
    fn convert_resizes_and_downscales_retina() {
        let png = encode(Bitmap::filled(400, 200, rgb(1, 2, 3)).view(), ImageFormat::Png, 1.0, 2).unwrap();
        let mut opts = ConvertOptions {
            format: ImageFormat::Jpeg,
            quality: 0.7,
            lossless: false,
            max_dimension: Some(100),
            downscale_retina: false,
            strip_metadata: false,
        };
        let out = convert(&png, &opts).unwrap();
        assert_eq!((out.width, out.height), (100, 50));
        opts.max_dimension = None;
        opts.downscale_retina = true;
        let out = convert(&png, &opts).unwrap();
        assert_eq!((out.width, out.height), (200, 100));
    }

    #[test]
    fn quality_changes_jpeg_size() {
        let mut noisy = Bitmap::new(128, 128);
        for y in 0..128 {
            for x in 0..128 {
                noisy.set_pixel(x, y, rgb(((x * 7) ^ (y * 13)) as u8, (x * y) as u8, (x + y * 3) as u8));
            }
        }
        let png = encode(noisy.view(), ImageFormat::Png, 1.0, 1).unwrap();
        let opts = |q| ConvertOptions {
            format: ImageFormat::Jpeg,
            quality: q,
            lossless: false,
            max_dimension: None,
            downscale_retina: false,
            strip_metadata: true,
        };
        let low = convert(&png, &opts(0.2)).unwrap().bytes.len();
        let high = convert(&png, &opts(0.95)).unwrap().bytes.len();
        assert!(low < high, "low {low} high {high}");
    }

    #[test]
    fn gif_has_two_frames() {
        let a = Bitmap::filled(16, 16, rgb(255, 0, 0));
        let b = Bitmap::filled(16, 16, rgb(0, 0, 255));
        let gif = encode_gif(&[a.view(), b.view()], 0.8).unwrap();
        let i = info(&gif).unwrap();
        assert_eq!(i.format, Some(ImageFormat::Gif));
        assert_eq!(i.frames, 2);
    }

    #[test]
    fn rejects_garbage() {
        assert!(info(b"definitely not an image").is_err());
        assert!(decode(b"definitely not an image").is_err());
    }
}
