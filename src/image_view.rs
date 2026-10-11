//! Image viewing support: opens PNG/JPEG/WebP/GIF/BMP/TIFF/ICO/SVG files
//! with a zoom toolbar, mirroring the PDF viewer's structure.

use std::{path::PathBuf, sync::Arc};

use std::path::Path;

use gpui_kit::{
    App, Context, FocusHandle, Focusable, Image, ImageFormat, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, Styled as _, Window,
    assets::IconName,
    base::Disableable as _,
    component::{
        ActiveTheme as _, Icon, Sizable as _,
        button::{Button, ButtonVariants as _},
        h_flex,
        scroll::ScrollableElement as _,
        v_flex,
    },
    div, img, px,
};

/// Default zoom factor (100%).
pub const DEFAULT_ZOOM: f32 = 1.0;
/// Minimum allowed zoom (25%).
pub const MIN_ZOOM: f32 = 0.25;
/// Maximum allowed zoom (800%).
pub const MAX_ZOOM: f32 = 8.0;
/// Multiplier for zoom in / out steps.
const ZOOM_STEP: f32 = 1.2;

/// Map an image extension to the gpui format it decodes as.
pub fn image_format(path: &Path) -> Option<ImageFormat> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => ImageFormat::Png,
        "jpg" | "jpeg" => ImageFormat::Jpeg,
        "webp" => ImageFormat::Webp,
        "gif" => ImageFormat::Gif,
        "svg" => ImageFormat::Svg,
        "bmp" => ImageFormat::Bmp,
        "tif" | "tiff" => ImageFormat::Tiff,
        "ico" => ImageFormat::Ico,
        _ => return None,
    })
}

pub struct ImageView {
    title: String,
    image: Arc<Image>,
    width: f32,
    height: f32,
    zoom: f32,
    focus_handle: FocusHandle,
}

/// Decode `bytes` as the image `path` names; returns the image with its
/// natural size in logical pixels.
pub fn load_image(path: &Path, bytes: Vec<u8>) -> Result<(Arc<Image>, f32, f32), String> {
    let format = image_format(path).ok_or_else(|| {
        format!(
            "unknown image format ({})",
            path.extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("no extension")
        )
    })?;
    let (width, height) = probe_dimensions(&bytes, format).unwrap_or((1024., 768.));
    let image = Arc::new(Image::from_bytes(format, bytes));
    Ok((image, width, height))
}

/// Read an image's pixel dimensions without keeping the decoded bitmap.
fn probe_dimensions(bytes: &[u8], format: ImageFormat) -> Option<(f32, f32)> {
    let fmt = match format {
        ImageFormat::Png => image::ImageFormat::Png,
        ImageFormat::Jpeg => image::ImageFormat::Jpeg,
        ImageFormat::Webp => image::ImageFormat::WebP,
        ImageFormat::Gif => image::ImageFormat::Gif,
        ImageFormat::Bmp => image::ImageFormat::Bmp,
        ImageFormat::Tiff => image::ImageFormat::Tiff,
        ImageFormat::Ico => image::ImageFormat::Ico,
        // SVG has no fixed pixel size; the container fits it.
        _ => return None,
    };
    let reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), fmt);
    let (width, height) = reader.into_dimensions().ok()?;
    Some((width as f32, height as f32))
}

impl ImageView {
    pub fn new(
        path: Option<PathBuf>,
        image: Arc<Image>,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) -> Self {
        let title = path
            .as_deref()
            .and_then(|p| p.file_name())
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "Image".into());
        Self {
            title,
            image,
            width,
            height,
            zoom: DEFAULT_ZOOM,
            focus_handle: cx.focus_handle(),
        }
    }

    pub fn zoom_in(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom * ZOOM_STEP, cx);
    }

    pub fn zoom_out(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom / ZOOM_STEP, cx);
    }

    pub fn reset_zoom(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(DEFAULT_ZOOM, cx);
    }

    pub fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        let clamped = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if (self.zoom - clamped).abs() > f32::EPSILON {
            self.zoom = clamped;
            cx.notify();
        }
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let zoom_pct = format!("{:.0}%", self.zoom * 100.0);
        let dims = format!("{} × {} px", self.width as u32, self.height as u32);

        h_flex()
            .id("image-toolbar")
            .h_9()
            .px_3()
            .flex_shrink_0()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.title_bar)
            .items_center()
            .justify_between()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Icon::new(IconName::Image)
                            .small()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .child(self.title.clone()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(dims),
                    ),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("image-zoom-out")
                            .ghost()
                            .small()
                            .compact()
                            .icon(Icon::new(IconName::ZoomOut))
                            .tooltip("Zoom out")
                            .disabled(self.zoom <= MIN_ZOOM)
                            .on_click(cx.listener(|this, _, _, cx| this.zoom_out(cx))),
                    )
                    .child(
                        Button::new("image-zoom-reset")
                            .ghost()
                            .small()
                            .compact()
                            .label(zoom_pct)
                            .tooltip("Reset zoom (100%)")
                            .on_click(cx.listener(|this, _, _, cx| this.reset_zoom(cx))),
                    )
                    .child(
                        Button::new("image-zoom-in")
                            .ghost()
                            .small()
                            .compact()
                            .icon(Icon::new(IconName::ZoomIn))
                            .tooltip("Zoom in")
                            .disabled(self.zoom >= MAX_ZOOM)
                            .on_click(cx.listener(|this, _, _, cx| this.zoom_in(cx))),
                    ),
            )
    }

    fn render_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let width = px(self.width * self.zoom);
        let height = px(self.height * self.zoom);

        v_flex()
            .id("image-pages-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_scrollbar()
            .bg(theme.background)
            .py_8()
            .child(
                div()
                    .id("image-card")
                    .max_w_full()
                    .w(width)
                    .h(height)
                    .mx_auto()
                    .overflow_hidden()
                    .child(
                        img(self.image.clone())
                            .id("image-canvas")
                            .w(width)
                            .h(height),
                    ),
            )
    }
}

impl Focusable for ImageView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for ImageView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("image-view")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(|this, _: &crate::actions::ZoomIn, _, cx| this.zoom_in(cx)))
            .on_action(cx.listener(|this, _: &crate::actions::ZoomOut, _, cx| this.zoom_out(cx)))
            .on_action(
                cx.listener(|this, _: &crate::actions::ZoomReset, _, cx| this.reset_zoom(cx)),
            )
            .child(self.render_toolbar(cx))
            .child(self.render_content(cx))
    }
}
