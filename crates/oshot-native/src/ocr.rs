//! Text and QR code recognition with Apple's Vision framework.

use objc2::encode::{Encoding, RefEncode};
use objc2::rc::{Allocated, Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2::{class, msg_send};
use objc2_foundation::{NSRange, NSRect, NSString};

use oshot_core::bitmap::BitmapRef;
use oshot_core::geometry::{Rect, rect};
use oshot_core::redact::{SecretKind, find_secrets};

use crate::cg;

#[link(name = "Vision", kind = "framework")]
unsafe extern "C" {}

#[repr(C)]
struct CGImageOpaque {
    _private: [u8; 0],
}

// SAFETY: matches the Objective-C type encoding of CGImageRef.
unsafe impl RefEncode for CGImageOpaque {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGImage", &[]));
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextLine {
    pub text: String,
    /// In image pixels, top-left origin.
    pub rect: Rect,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recognized {
    pub lines: Vec<TextLine>,
    pub codes: Vec<String>,
    /// Secrets found in the lines, located in image pixels.
    pub secrets: Vec<(Rect, SecretKind)>,
}

impl Recognized {
    /// Lines top to bottom, followed by any QR payloads.
    pub fn text(&self) -> String {
        let mut parts: Vec<String> = self.lines.iter().map(|l| l.text.clone()).collect();
        parts.extend(self.codes.iter().cloned());
        parts.join("\n")
    }
}

fn to_pixels(r: NSRect, w: u32, h: u32) -> Rect {
    // Vision boxes are normalised with a bottom-left origin.
    let (w, h) = (w as f64, h as f64);
    rect(
        (r.origin.x * w) as f32,
        ((1.0 - r.origin.y - r.size.height) * h) as f32,
        (r.size.width * w) as f32,
        (r.size.height * h) as f32,
    )
}

fn utf16_offset(s: &str, byte: usize) -> usize {
    s[..byte].encode_utf16().count()
}

/// Recognises text and QR codes in `image`. Runs synchronously; call it off
/// the main thread.
pub fn recognize(image: BitmapRef) -> Result<Recognized, String> {
    let cg_image = cg::image_from_bitmap(image).ok_or("Could not create image")?;
    let (w, h) = (image.width, image.height);
    autoreleasepool(|_| {
        // SAFETY: plain Vision API calls on objects we own; every selector and
        // argument type matches the documented Vision interface.
        unsafe {
            let text_req: Retained<AnyObject> = msg_send![class!(VNRecognizeTextRequest), new];
            let _: () = msg_send![&*text_req, setRecognitionLevel: 0isize];
            let _: () = msg_send![&*text_req, setUsesLanguageCorrection: true];
            let code_req: Retained<AnyObject> = msg_send![class!(VNDetectBarcodesRequest), new];
            let options: Retained<AnyObject> = msg_send![class!(NSDictionary), dictionary];
            let handler: Allocated<AnyObject> = msg_send![class!(VNImageRequestHandler), alloc];
            let handler: Retained<AnyObject> =
                msg_send![handler, initWithCGImage: cg_image.ptr() as *const CGImageOpaque, options: &*options];
            let requests: Retained<AnyObject> =
                msg_send![class!(NSArray), arrayWithObjects: [&*text_req, &*code_req].as_ptr(), count: 2usize];
            let ok: bool =
                msg_send![&*handler, performRequests: &*requests, error: std::ptr::null_mut::<*mut AnyObject>()];
            if !ok {
                return Err("Text recognition failed".to_string());
            }

            let mut out = Recognized::default();
            let results: Option<Retained<AnyObject>> = msg_send![&*text_req, results];
            if let Some(results) = results {
                let count: usize = msg_send![&*results, count];
                for i in 0..count {
                    let obs: Retained<AnyObject> = msg_send![&*results, objectAtIndex: i];
                    let candidates: Retained<AnyObject> = msg_send![&*obs, topCandidates: 1usize];
                    let n: usize = msg_send![&*candidates, count];
                    if n == 0 {
                        continue;
                    }
                    let cand: Retained<AnyObject> = msg_send![&*candidates, objectAtIndex: 0usize];
                    let s: Retained<NSString> = msg_send![&*cand, string];
                    let text = s.to_string();
                    let bbox: NSRect = msg_send![&*obs, boundingBox];
                    for (range, kind) in find_secrets(&text) {
                        let start = utf16_offset(&text, range.start);
                        let end = utf16_offset(&text, range.end);
                        let ns_range = NSRange {
                            location: start,
                            length: end - start,
                        };
                        let sub: Option<Retained<AnyObject>> = msg_send![&*cand, boundingBoxForRange: ns_range, error: std::ptr::null_mut::<*mut AnyObject>()];
                        let r = match sub {
                            Some(sub) => {
                                let b: NSRect = msg_send![&*sub, boundingBox];
                                b
                            }
                            None => bbox,
                        };
                        out.secrets.push((to_pixels(r, w, h).inflate(2.0), kind));
                    }
                    out.lines.push(TextLine {
                        text,
                        rect: to_pixels(bbox, w, h),
                    });
                }
            }
            let codes: Option<Retained<AnyObject>> = msg_send![&*code_req, results];
            if let Some(codes) = codes {
                let count: usize = msg_send![&*codes, count];
                for i in 0..count {
                    let obs: Retained<AnyObject> = msg_send![&*codes, objectAtIndex: i];
                    let payload: Option<Retained<NSString>> = msg_send![&*obs, payloadStringValue];
                    if let Some(p) = payload {
                        out.codes.push(p.to_string());
                    }
                }
            }
            out.lines
                .sort_by(|a, b| a.rect.y.total_cmp(&b.rect.y).then(a.rect.x.total_cmp(&b.rect.x)));
            Ok(out)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{ExportOptions, render};
    use oshot_core::bitmap::Bitmap;
    use oshot_core::color::rgb;
    use oshot_core::effects::EffectCache;
    use oshot_core::geometry::pt;
    use oshot_core::model::{Document, Shape, Style, Tool};

    /// Draws text with the exporter so the test needs no fixture files.
    fn image_with_text(text: &str) -> Bitmap {
        let img = Bitmap::filled(900, 140, rgb(255, 255, 255));
        let mut doc = Document::new(900, 140, 1.0);
        let style = Style {
            color: rgb(0, 0, 0),
            text_size: 40.0,
            text_background: false,
            ..Style::for_tool(Tool::Text, 1.0)
        };
        doc.add(
            Shape::Text {
                origin: pt(20.0, 40.0),
                text: text.into(),
                extent: pt(0.0, 0.0),
                arrow: None,
            },
            style,
        );
        render(&doc, img.view(), &mut EffectCache::default(), ExportOptions::default())
    }

    #[test]
    fn reads_text_and_locates_secrets() {
        let img = image_with_text("Contact max@example.com today");
        let r = recognize(img.view()).unwrap();
        let text = r.text();
        assert!(text.contains("example.com"), "got {text:?}");
        assert_eq!(r.secrets.len(), 1, "{:?}", r.secrets);
        let (rect, kind) = r.secrets[0];
        assert_eq!(kind, SecretKind::Email);
        assert!(
            rect.x > 100.0 && rect.right() < 750.0 && rect.y > 20.0 && rect.bottom() < 120.0,
            "{rect:?}"
        );
    }

    #[test]
    fn blank_image_has_no_text() {
        let img = Bitmap::filled(200, 100, rgb(255, 255, 255));
        assert!(recognize(img.view()).unwrap().text().is_empty());
    }
}
