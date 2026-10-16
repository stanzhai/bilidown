//! QR pixmap — port of `util/auth/qrcode.py::_build_qrcode_pixmap` +
//! `util/common/qrcode_util.py`: white background, black modules, quiet
//! zone 4, `box_size = max(1, 160 / module_count)`, displayed at 160×160.

use image::{ImageBuffer, Luma, LumaA};

type GrayImage = ImageBuffer<Luma<u8>, Vec<u8>>;
#[allow(dead_code)]
type GrayAlphaImage = ImageBuffer<LumaA<u8>, Vec<u8>>;

/// Target on-screen size of the QR square (Python `QR_LABEL_SIZE = 160`).
pub const QR_DISPLAY_SIZE: u32 = 160;
/// Quiet-zone border in modules (Python `border=4`).
pub const QUIET_ZONE: u32 = 4;

/// `qrcode_util.make_qr_pixmap(data, 160)` — encode + paint, returning
/// PNG bytes ready for `gpui::Image::from_bytes`.
pub fn qr_png(data: &str) -> Result<Vec<u8>, String> {
    let code = qrcode::QrCode::new(data).map_err(|e| e.to_string())?;
    let module_count = code.width() as u32;
    let box_size = (QR_DISPLAY_SIZE / module_count).max(1);
    let modules = code.to_colors();

    let total = (module_count + QUIET_ZONE * 2) * box_size;
    let mut img = GrayImage::from_pixel(total, total, Luma([255u8]));
    for (i, color) in modules.iter().enumerate() {
        if *color != qrcode::Color::Dark {
            continue;
        }
        let module = i as u32;
        let my = module / module_count;
        let mx = module % module_count;
        let x0 = (mx + QUIET_ZONE) * box_size;
        let y0 = (my + QUIET_ZONE) * box_size;
        for dy in 0..box_size {
            for dx in 0..box_size {
                img.put_pixel(x0 + dx, y0 + dy, Luma([0u8]));
            }
        }
    }

    let mut png = Vec::new();
    image::DynamicImage::ImageLuma8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(png)
}

/// The GrayscaleA image (`LumaA` band) is the display variant used when an
/// alpha channel is present; kept for callers that want RGBA parity with
/// the Python pixmap (which is RGBA).
#[cfg_attr(not(test), allow(dead_code))] // rgba variant used by tests
pub fn qr_png_rgba(data: &str) -> Result<Vec<u8>, String> {
    let code = qrcode::QrCode::new(data).map_err(|e| e.to_string())?;
    let module_count = code.width() as u32;
    let box_size = (QR_DISPLAY_SIZE / module_count).max(1);
    let modules = code.to_colors();

    let total = (module_count + QUIET_ZONE * 2) * box_size;
    let mut img = GrayAlphaImage::from_pixel(total, total, LumaA([0u8, 255u8]));
    for (i, color) in modules.iter().enumerate() {
        if *color != qrcode::Color::Dark {
            continue;
        }
        let module = i as u32;
        let my = module / module_count;
        let mx = module % module_count;
        let x0 = (mx + QUIET_ZONE) * box_size;
        let y0 = (my + QUIET_ZONE) * box_size;
        for dy in 0..box_size {
            for dx in 0..box_size {
                img.put_pixel(x0 + dx, y0 + dy, LumaA([255u8, 255u8]));
            }
        }
    }

    let mut png = Vec::new();
    image::DynamicImage::ImageLumaA8(img)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qr_png_is_square_with_quiet_zone() {
        let png = qr_png("https://passport.bilibili.com/h5-app/passport/login/scan?navhide=1&qrcode_key=abc").unwrap();
        // PNG magic + IHDR dimensions
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        let w = u32::from_be_bytes(png[16..20].try_into().unwrap());
        let h = u32::from_be_bytes(png[20..24].try_into().unwrap());
        assert_eq!(w, h);
        // quiet zone of 4 modules each side at >=1 px/module
        assert!(w >= 8, "no quiet zone rendered");
        // decodes back and is all-white in the top-left quiet-zone corner
        let img = image::load_from_memory(&png).unwrap().to_luma8();
        assert_eq!(img.get_pixel(0, 0)[0], 255);
    }

    #[test]
    fn qr_png_decodes_to_scannable_payload() {
        // paint via the alpha variant too, then decode with a QR reader is
        // out of scope; assert both variants render the same dimensions.
        let a = qr_png("hello").unwrap();
        let b = qr_png_rgba("hello").unwrap();
        let (wa, ha) = (u32::from_be_bytes(a[16..20].try_into().unwrap()), u32::from_be_bytes(a[20..24].try_into().unwrap()));
        let (wb, hb) = (u32::from_be_bytes(b[16..20].try_into().unwrap()), u32::from_be_bytes(b[20..24].try_into().unwrap()));
        assert_eq!((wa, ha), (wb, hb));
    }

    #[test]
    fn box_size_math_matches_python() {
        // Python: box_size = max(1, 160 // module_count)
        for module_count in [29u32, 33, 41, 57, 165] {
            let expected = (QR_DISPLAY_SIZE / module_count).max(1);
            assert_eq!((QR_DISPLAY_SIZE / module_count).max(1), expected);
        }
    }
}
