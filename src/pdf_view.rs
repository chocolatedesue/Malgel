//! PDF viewing support: renders PDF documents using Hayro and GPUI.
//!
//! PDF pages are loaded using `hayro_interpret::hayro_syntax::Pdf` and converted
//! into crisp SVG representations with `hayro_svg::convert`. Each page is rendered
//! as a clean sheet with shadow and border, complete with zoom controls and page numbering.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use gpui_kit::{
    base::Disableable as _,
    div, img, px, App, Context, ElementId, FocusHandle, Focusable,
    Image, ImageFormat, InteractiveElement as _, IntoElement, ParentElement as _,
    prelude::FluentBuilder as _,
    Render, Styled as _, Window,
    assets::IconName,
    component::{
        h_flex, v_flex, ActiveTheme as _, Icon, Sizable as _,
        button::{Button, ButtonVariants as _},
        scroll::ScrollableElement as _,
    },
};

use hayro_svg::{
    convert, RenderCache, SvgRenderSettings,
    hayro_interpret::{
        InterpreterSettings,
        font::{FontData, FontQuery, StandardFont},
        hayro_syntax::Pdf,
    },
};

use crate::actions::{ZoomIn, ZoomOut, ZoomReset};

/// Default zoom factor (100%).
pub const DEFAULT_ZOOM: f32 = 1.0;
/// Minimum allowed zoom (25%).
pub const MIN_ZOOM: f32 = 0.25;
/// Maximum allowed zoom (400%).
pub const MAX_ZOOM: f32 = 4.0;
/// Multiplier for zoom in / out steps.
pub const ZOOM_STEP: f32 = 1.25;

/// A single rendered page of a PDF document.
#[derive(Clone)]
pub struct PdfPage {
    /// 0-indexed page number in document.
    pub index: usize,
    /// 1-indexed human-readable page number.
    pub page_number: usize,
    /// Width in PDF points.
    pub width: f32,
    /// Height in PDF points.
    pub height: f32,
    /// Rendered SVG markup.
    pub svg: String,
    /// GPUI Image resource created from `svg`.
    pub image: Arc<Image>,
}

/// Create default interpreter settings using Typst's bundled Foxit standard fonts.
pub fn default_interpreter_settings() -> InterpreterSettings {
    let select_standard_font = |font: StandardFont| -> Option<(FontData, u32)> {
        let bytes: &'static [u8] = match font {
            StandardFont::Helvetica => typst_assets::pdf::SANS,
            StandardFont::HelveticaBold => typst_assets::pdf::SANS_BOLD,
            StandardFont::HelveticaOblique => typst_assets::pdf::SANS_ITALIC,
            StandardFont::HelveticaBoldOblique => typst_assets::pdf::SANS_BOLD_ITALIC,
            StandardFont::Courier => typst_assets::pdf::FIXED,
            StandardFont::CourierBold => typst_assets::pdf::FIXED_BOLD,
            StandardFont::CourierOblique => typst_assets::pdf::FIXED_ITALIC,
            StandardFont::CourierBoldOblique => typst_assets::pdf::FIXED_BOLD_ITALIC,
            StandardFont::TimesRoman => typst_assets::pdf::SERIF,
            StandardFont::TimesBold => typst_assets::pdf::SERIF_BOLD,
            StandardFont::TimesItalic => typst_assets::pdf::SERIF_ITALIC,
            StandardFont::TimesBoldItalic => typst_assets::pdf::SERIF_BOLD_ITALIC,
            StandardFont::ZapfDingBats => typst_assets::pdf::DING_BATS,
            StandardFont::Symbol => typst_assets::pdf::SYMBOL,
        };
        Some((Arc::new(bytes) as FontData, 0))
    };

    InterpreterSettings {
        font_resolver: Arc::new(move |query| match query {
            FontQuery::Standard(s) => select_standard_font(*s),
            FontQuery::Fallback(f) => select_standard_font(f.pick_standard_font()),
        }),
        cmap_resolver: Arc::new(|_| None),
        warning_sink: Arc::new(|_| {}),
        render_annotations: true,
    }
}

/// Render all pages of a PDF document from raw bytes.
pub fn render_pdf_bytes(bytes: &[u8]) -> Result<Vec<PdfPage>, String> {
    let pdf = Pdf::new(bytes.to_vec())
        .map_err(|e| format!("Failed to parse PDF: {e:?}"))?;

    let cache = RenderCache::new();
    let interpreter_settings = default_interpreter_settings();
    let render_settings = SvgRenderSettings {
        bg_color: [255, 255, 255, 255],
    };

    let mut pages = Vec::with_capacity(pdf.pages().len());
    for (index, page) in pdf.pages().iter().enumerate() {
        let (width, height) = page.render_dimensions();
        let svg = convert(page, &cache, &interpreter_settings, &render_settings);
        let image = Arc::new(Image::from_bytes(ImageFormat::Svg, svg.as_bytes().to_vec()));
        pages.push(PdfPage {
            index,
            page_number: index + 1,
            width,
            height,
            svg,
            image,
        });
    }

    Ok(pages)
}

/// A GPUI view displaying a scrollable PDF document.
pub struct PdfView {
    path: Option<PathBuf>,
    title: String,
    pages: Vec<PdfPage>,
    zoom: f32,
    focus_handle: FocusHandle,
    error: Option<String>,
}

impl PdfView {
    /// Create a `PdfView` from already rendered pages.
    pub fn new(path: Option<PathBuf>, pages: Vec<PdfPage>, cx: &mut Context<Self>) -> Self {
        let title = path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "PDF Document".to_string());

        Self {
            path,
            title,
            pages,
            zoom: DEFAULT_ZOOM,
            focus_handle: cx.focus_handle(),
            error: None,
        }
    }

    /// Create a `PdfView` in an error state.
    pub fn new_with_error(error: String, path: Option<PathBuf>, cx: &mut Context<Self>) -> Self {
        let title = path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "PDF Document".to_string());

        Self {
            path,
            title,
            pages: Vec::new(),
            zoom: DEFAULT_ZOOM,
            focus_handle: cx.focus_handle(),
            error: Some(error),
        }
    }

    /// Load and render a PDF document from raw bytes.
    pub fn from_bytes(
        bytes: &[u8],
        path: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) -> Result<Self, String> {
        let pages = render_pdf_bytes(bytes)?;
        Ok(Self::new(path, pages, cx))
    }

    /// Open and render a PDF document from a file path.
    pub fn from_path(path: impl AsRef<Path>, cx: &mut Context<Self>) -> Result<Self, String> {
        let path = path.as_ref().to_path_buf();
        let bytes = std::fs::read(&path)
            .map_err(|e| format!("Failed to read file: {e}"))?;
        Self::from_bytes(&bytes, Some(path), cx)
    }

    /// The path of the open document, if opened from disk.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Document title shown in the toolbar.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// All rendered pages in the document.
    pub fn pages(&self) -> &[PdfPage] {
        &self.pages
    }

    /// Total number of pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Current zoom level.
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Zoom in by one step.
    pub fn zoom_in(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom * ZOOM_STEP, cx);
    }

    /// Zoom out by one step.
    pub fn zoom_out(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(self.zoom / ZOOM_STEP, cx);
    }

    /// Reset zoom level back to 100%.
    pub fn reset_zoom(&mut self, cx: &mut Context<Self>) {
        self.set_zoom(DEFAULT_ZOOM, cx);
    }

    /// Set an explicit zoom factor, clamped between `MIN_ZOOM` and `MAX_ZOOM`.
    pub fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        let clamped = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if (self.zoom - clamped).abs() > f32::EPSILON {
            self.zoom = clamped;
            cx.notify();
        }
    }

    /// The error message, if document loading or parsing failed.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let zoom_pct = format!("{:.0}%", self.zoom * 100.0);
        let page_count = self.pages.len();

        h_flex()
            .id("pdf-toolbar")
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
                        Icon::new(IconName::FileText)
                            .small()
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .child(self.title.clone()),
                    )
                    .when(page_count > 0, |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(format!(
                                    "({} {})",
                                    page_count,
                                    if page_count == 1 { "page" } else { "pages" }
                                )),
                        )
                    }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("pdf-zoom-out")
                            .ghost()
                            .small()
                            .compact()
                            .icon(Icon::new(IconName::ZoomOut))
                            .tooltip("Zoom out")
                            .disabled(self.zoom <= MIN_ZOOM || self.pages.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| this.zoom_out(cx))),
                    )
                    .child(
                        Button::new("pdf-zoom-reset")
                            .ghost()
                            .small()
                            .compact()
                            .label(zoom_pct)
                            .tooltip("Reset zoom (100%)")
                            .disabled(self.pages.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| this.reset_zoom(cx))),
                    )
                    .child(
                        Button::new("pdf-zoom-in")
                            .ghost()
                            .small()
                            .compact()
                            .icon(Icon::new(IconName::ZoomIn))
                            .tooltip("Zoom in")
                            .disabled(self.zoom >= MAX_ZOOM || self.pages.is_empty())
                            .on_click(cx.listener(|this, _, _, cx| this.zoom_in(cx))),
                    ),
            )
    }

    fn render_content(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();

        if let Some(error) = &self.error {
            return v_flex()
                .id("pdf-error")
                .size_full()
                .items_center()
                .justify_center()
                .gap_3()
                .p_8()
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size_8()
                        .text_color(theme.danger),
                )
                .child(
                    div()
                        .text_sm()
                        .font_weight(gpui_kit::FontWeight::MEDIUM)
                        .child("Failed to load PDF"),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(error.clone()),
                )
                .into_any_element();
        }

        if self.pages.is_empty() {
            return v_flex()
                .id("pdf-empty")
                .size_full()
                .items_center()
                .justify_center()
                .p_8()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child("This PDF has no pages.")
                .into_any_element();
        }

        let total = self.pages.len();
        let zoom = self.zoom;

        v_flex()
            .id("pdf-pages-scroll")
            .flex_1()
            .min_h_0()
            .w_full()
            .overflow_y_scrollbar()
            .bg(theme.background)
            .py_8()
            .px_4()
            .gap_8()
            .items_center()
            .children(self.pages.iter().map(|page| {
                let width = px(page.width * zoom);
                let height = px(page.height * zoom);
                let page_num = page.page_number;

                v_flex()
                    .id(ElementId::NamedInteger(
                        "pdf-page-wrapper".into(),
                        page_num as u64,
                    ))
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id(ElementId::NamedInteger(
                                "pdf-page-card".into(),
                                page_num as u64,
                            ))
                            .w(width)
                            .h(height)
                            .bg(gpui_kit::rgb(0xffffff))
                            .border_1()
                            .border_color(theme.border)
                            .shadow_md()
                            .rounded_sm()
                            .overflow_hidden()
                            .child(
                                img(page.image.clone())
                                    .w(width)
                                    .h(height),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("Page {} / {}", page_num, total)),
                    )
            }))
            .into_any_element()
    }
}

impl Focusable for PdfView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for PdfView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .id("pdf-view")
            .track_focus(&self.focus_handle)
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(|this, _: &ZoomIn, _, cx| this.zoom_in(cx)))
            .on_action(cx.listener(|this, _: &ZoomOut, _, cx| this.zoom_out(cx)))
            .on_action(cx.listener(|this, _: &ZoomReset, _, cx| this.reset_zoom(cx)))
            .child(self.render_toolbar(cx))
            .child(self.render_content(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_renders_minimal_pdf() {
        // Generate a minimal valid PDF using Typst's world & compiler
        let markup = "= Hello Malgel PDF\n\nThis is a test of Hayro PDF rendering.";
        let doc = crate::typst_world::compile(
            markup.into(),
            std::collections::HashMap::new(),
            &crate::typst_world::BUNDLED_FONTS,
        )
        .expect("compile typst doc");

        let pdf_bytes = typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default())
            .expect("generate pdf bytes");

        let pages = render_pdf_bytes(&pdf_bytes).expect("render pdf pages");
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].page_number, 1);
        assert!(pages[0].width > 0.0);
        assert!(pages[0].height > 0.0);
        assert!(pages[0].svg.contains("<svg"));
        assert!(pages[0].svg.contains("</svg>"));
    }

    #[test]
    fn reports_error_on_invalid_pdf_bytes() {
        let invalid = b"NOT A VALID PDF FILE";
        let result = render_pdf_bytes(invalid);
        assert!(result.is_err());
    }
}
