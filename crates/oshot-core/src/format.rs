//! Image formats: detection from file signatures and per-format capabilities.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ImageFormat {
    Png,
    Jpeg,
    WebP,
    Heic,
    Avif,
    Tiff,
    Gif,
    Bmp,
}

impl ImageFormat {
    /// Formats the converter can write, in picker order.
    pub const WRITABLE: [ImageFormat; 7] = [
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::WebP,
        ImageFormat::Heic,
        ImageFormat::Avif,
        ImageFormat::Tiff,
        ImageFormat::Gif,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ImageFormat::Png => "PNG",
            ImageFormat::Jpeg => "JPEG",
            ImageFormat::WebP => "WebP",
            ImageFormat::Heic => "HEIC",
            ImageFormat::Avif => "AVIF",
            ImageFormat::Tiff => "TIFF",
            ImageFormat::Gif => "GIF",
            ImageFormat::Bmp => "BMP",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            ImageFormat::Png => "png",
            ImageFormat::Jpeg => "jpg",
            ImageFormat::WebP => "webp",
            ImageFormat::Heic => "heic",
            ImageFormat::Avif => "avif",
            ImageFormat::Tiff => "tiff",
            ImageFormat::Gif => "gif",
            ImageFormat::Bmp => "bmp",
        }
    }

    /// Uniform Type Identifier ImageIO uses for this format.
    pub fn uti(self) -> &'static str {
        match self {
            ImageFormat::Png => "public.png",
            ImageFormat::Jpeg => "public.jpeg",
            ImageFormat::WebP => "org.webmproject.webp",
            ImageFormat::Heic => "public.heic",
            ImageFormat::Avif => "public.avif",
            ImageFormat::Tiff => "public.tiff",
            ImageFormat::Gif => "com.compuserve.gif",
            ImageFormat::Bmp => "com.microsoft.bmp",
        }
    }

    /// Whether a quality setting changes the output.
    pub fn is_lossy(self) -> bool {
        matches!(
            self,
            ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Heic | ImageFormat::Avif
        )
    }

    pub fn supports_alpha(self) -> bool {
        !matches!(self, ImageFormat::Jpeg | ImageFormat::Bmp)
    }

    pub fn from_extension(ext: &str) -> Option<ImageFormat> {
        Some(match ext.to_ascii_lowercase().as_str() {
            "png" => ImageFormat::Png,
            "jpg" | "jpeg" => ImageFormat::Jpeg,
            "webp" => ImageFormat::WebP,
            "heic" | "heif" => ImageFormat::Heic,
            "avif" => ImageFormat::Avif,
            "tif" | "tiff" => ImageFormat::Tiff,
            "gif" => ImageFormat::Gif,
            "bmp" => ImageFormat::Bmp,
            _ => return None,
        })
    }

    /// Detects the format from the first bytes of the file, so a misnamed
    /// file is still recognised.
    pub fn detect(bytes: &[u8]) -> Option<ImageFormat> {
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Some(ImageFormat::Png);
        }
        if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
            return Some(ImageFormat::Jpeg);
        }
        if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            return Some(ImageFormat::Gif);
        }
        if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            return Some(ImageFormat::WebP);
        }
        if bytes.starts_with(b"II*\0") || bytes.starts_with(b"MM\0*") {
            return Some(ImageFormat::Tiff);
        }
        if bytes.starts_with(b"BM") && bytes.len() > 14 {
            return Some(ImageFormat::Bmp);
        }
        if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
            let brand = &bytes[8..12];
            if brand == b"avif" || brand == b"avis" {
                return Some(ImageFormat::Avif);
            }
            if [b"heic", b"heix", b"hevc", b"heim", b"heis", b"mif1", b"msf1"]
                .iter()
                .any(|b| brand == *b)
            {
                return Some(ImageFormat::Heic);
            }
        }
        None
    }
}

/// Standard base64, for `data:` URIs.
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            T[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            T[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// `data:image/png;base64,…`
pub fn data_uri(format: ImageFormat, bytes: &[u8]) -> String {
    let mime = match format {
        ImageFormat::Jpeg => "image/jpeg",
        ImageFormat::WebP => "image/webp",
        ImageFormat::Gif => "image/gif",
        ImageFormat::Heic => "image/heic",
        ImageFormat::Avif => "image/avif",
        ImageFormat::Tiff => "image/tiff",
        ImageFormat::Bmp => "image/bmp",
        ImageFormat::Png => "image/png",
    };
    format!("data:{mime};base64,{}", base64(bytes))
}
/// "1.4 MB", "820 KB", "512 B".
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1000.0;
    let b = bytes as f64;
    if b >= KB * KB {
        format!("{:.1} MB", b / KB / KB)
    } else if b >= KB {
        format!("{:.0} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

/// Fits `w` x `h` inside a square of `max` pixels, keeping the aspect ratio.
pub fn fit_within(w: u32, h: u32, max: u32) -> (u32, u32) {
    if w <= max && h <= max {
        return (w, h);
    }
    let s = max as f64 / w.max(h) as f64;
    (
        ((w as f64 * s).round() as u32).max(1),
        ((h as f64 * s).round() as u32).max(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_signatures() {
        assert_eq!(ImageFormat::detect(b"\x89PNG\r\n\x1a\nrest"), Some(ImageFormat::Png));
        assert_eq!(ImageFormat::detect(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(ImageFormat::Jpeg));
        assert_eq!(ImageFormat::detect(b"RIFF\0\0\0\0WEBPVP8 "), Some(ImageFormat::WebP));
        assert_eq!(ImageFormat::detect(b"\0\0\0\x1cftypavif"), Some(ImageFormat::Avif));
        assert_eq!(ImageFormat::detect(b"\0\0\0\x18ftypheic"), Some(ImageFormat::Heic));
        assert_eq!(ImageFormat::detect(b"GIF89a.."), Some(ImageFormat::Gif));
        assert_eq!(ImageFormat::detect(b"MM\0*...."), Some(ImageFormat::Tiff));
        assert_eq!(ImageFormat::detect(b"hello world"), None);
    }

    #[test]
    fn sizes_and_fit() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(820_400), "820 KB");
        assert_eq!(human_size(1_400_000), "1.4 MB");
        assert_eq!(fit_within(4000, 2000, 1000), (1000, 500));
        assert_eq!(fit_within(400, 200, 1000), (400, 200));
    }

    #[test]
    fn base64_matches_rfc4648_vectors() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(data_uri(ImageFormat::Png, b"hi"), "data:image/png;base64,aGk=");
    }

    #[test]
    fn extension_round_trip() {
        for f in ImageFormat::WRITABLE {
            assert_eq!(ImageFormat::from_extension(f.extension()), Some(f));
        }
    }
}
