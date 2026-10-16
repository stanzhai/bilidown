//! View-cover dialog (ticket 31): port of
//! `src/gui/dialog/misc/view_cover.py`. Fetches the RAW cover URL
//! (no `@.suffix` decoration — original-quality bytes), decodes it,
//! aspect-fits into a 640×340 area, and offers Save As writing the
//! original bytes untouched. Public API: parse-tree context menus
//! (out of this seam list) call `open_view_cover`.

use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::component::button::Button;
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::label::Label;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::WindowExt;
use gpui_kit::gpui::{img, Image, ImageFormat, ImageSource, ObjectFit, StyledImage};
use gpui_kit::{div, prelude::*, px, ParentElement, Window};

use super::active_catalog;

const CTX: &str = "ViewCoverDialog";

/// View-area size from Python (`640x340` label).
const VIEW_W: f32 = 640.;
const VIEW_H: f32 = 340.;

/// Map sniffed `image::ImageFormat` to gpui `ImageFormat` (subset
/// bilibili covers actually use).
fn gpui_format(bytes: &[u8]) -> Option<(ImageFormat, &'static str)> {
    let format = image::guess_format(bytes).ok()?;
    let (format, ext) = match format {
        image::ImageFormat::Png => (ImageFormat::Png, "png"),
        image::ImageFormat::Jpeg => (ImageFormat::Jpeg, "jpg"),
        image::ImageFormat::WebP => (ImageFormat::Webp, "webp"),
        image::ImageFormat::Gif => (ImageFormat::Gif, "gif"),
        image::ImageFormat::Bmp => (ImageFormat::Bmp, "bmp"),
        image::ImageFormat::Tiff => (ImageFormat::Tiff, "tiff"),
        image::ImageFormat::Ico => (ImageFormat::Ico, "ico"),
        _ => return None,
    };
    Some((format, ext))
}

/// Open the cover viewer for `cover_url`.
pub fn open_view_cover(
    cover_url: String,
    cfg: config::ConfigStore,
    window: &mut Window,
    cx: &mut gpui_kit::App,
) {
    let catalog = active_catalog(&cfg);
    let title = catalog.tr(CTX, "Cover").to_owned();
    let save_as = catalog.tr(CTX, "Save As").to_owned();
    let close = "Close".to_owned();

    // Shared between the fetch future and the content closure.
    let image_cell: Rc<std::cell::RefCell<Option<Arc<Image>>>> =
        Rc::new(std::cell::RefCell::new(None));
    let bytes_cell: Rc<std::cell::RefCell<Option<Vec<u8>>>> =
        Rc::new(std::cell::RefCell::new(None));
    let ext_cell: Rc<std::cell::RefCell<&'static str>> = Rc::new(std::cell::RefCell::new("jpg"));

    // Fetch raw bytes (Python fetches the undecorated URL too).
    let client = crate::ui::parse::dispatch::ParseService::new(&cfg).client;
    let url = cover_url.clone();
    let img_for_spawn = image_cell.clone();
    let bytes_for_spawn = bytes_cell.clone();
    let ext_for_spawn = ext_cell.clone();
    window
        .spawn(cx, async move |cx: &mut gpui_kit::AsyncWindowContext| {
            let fetched: anyhow::Result<Vec<u8>> = async {
                let bytes = client
                    .inner()
                    .get(&url)
                    .send()
                    .await?
                    .error_for_status()?
                    .bytes()
                    .await?
                    .to_vec();
                anyhow::Ok(bytes)
            }
            .await;
            let decoded = fetched.and_then(|bytes| {
                let (format, ext) = gpui_format(&bytes)
                    .ok_or_else(|| anyhow::anyhow!("unsupported cover image format"))?;
                // Sniffed magic is enough; gpui decodes at render time
                // (Image::from_bytes stores bytes, no eager decode).
                let image = Image::from_bytes(format, bytes.clone());
                anyhow::Ok((image, bytes, ext))
            });
            match decoded {
                Ok((image, bytes, ext)) => {
                    *img_for_spawn.borrow_mut() = Some(Arc::new(image));
                    *bytes_for_spawn.borrow_mut() = Some(bytes);
                    *ext_for_spawn.borrow_mut() = ext;
                }
                Err(e) => {
                    // Python logs the decode failure and stops the spinner.
                    let msg = e.to_string();
                    let _ = cx.update(|window, cx| {
                        window.push_notification(Notification::error(msg), cx);
                    });
                }
            }
            let _ = cx.update(|window, _| window.refresh());
        })
        .detach();

    window.open_dialog(cx, move |dialog, _, _| {
        let image_cell = image_cell.clone();
        let bytes_cell = bytes_cell.clone();
        let ext_cell = ext_cell.clone();
        let save_as = save_as.clone();

        dialog
            .title(title.clone())
            .w(px(660.))
            .content(move |content, _, _| {
                let image = image_cell.borrow().clone();
                content.child(
                    div()
                        .id("cover-view")
                        .size(px(VIEW_W))
                        .h(px(VIEW_H))
                        .flex()
                        .justify_center()
                        .items_center()
                        .child(match image {
                            Some(image) => img(ImageSource::from(image))
                                .max_w(px(VIEW_W))
                                .max_h(px(VIEW_H))
                                .object_fit(ObjectFit::Contain)
                                .into_any_element(),
                            None => Label::new("Loading...").into_any_element(),
                        }),
                )
            })
            .footer(
                DialogFooter::new()
                    .child(Button::new("cover-save").label(save_as.clone()).on_click({
                        let bytes_cell = bytes_cell.clone();
                        let ext_cell = ext_cell.clone();
                        move |_, window, cx| {
                            let Some(bytes) = bytes_cell.borrow().clone() else {
                                return; // nothing fetched yet — Python stops the spinner too
                            };
                            let ext = *ext_cell.borrow();
                            let suggested = format!("cover.{ext}");
                            let rx =
                                cx.prompt_for_new_path(std::path::Path::new("."), Some(&suggested));
                            let window_handle = window.window_handle();
                            cx.spawn(async move |cx| {
                                if let Ok(Ok(Some(path))) = rx.await {
                                    if let Err(e) = std::fs::write(&path, bytes) {
                                        let msg = e.to_string();
                                        let _ = window_handle.update(cx, |_, window, cx| {
                                            window.push_notification(Notification::error(msg), cx);
                                        });
                                    }
                                }
                            })
                            .detach();
                        }
                    }))
                    .child(Button::new("cover-close").label(close.clone()).on_click(
                        move |_, window, cx| {
                            window.close_dialog(cx);
                        },
                    )),
            )
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpui_format_sniffs_the_supported_formats() {
        // 1x1 PNG.
        let png: &[u8] = &[
            0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48,
            0x44, 0x52,
        ];
        assert!(matches!(gpui_format(png), Some((ImageFormat::Png, "png"))));
        // JPEG magic.
        let jpg: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0];
        assert!(matches!(gpui_format(jpg), Some((ImageFormat::Jpeg, "jpg"))));
        // Garbage.
        assert!(gpui_format(b"not an image at all").is_none());
    }
}
