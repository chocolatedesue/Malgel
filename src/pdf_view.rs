//! PDF viewing support: renders PDF documents using Hayro and GPUI.
//!
//! Pages are parsed with `hayro_interpret::hayro_syntax::Pdf` and turned into
//! crisp SVG sheets with `hayro_svg::convert` **on demand**: only the pages in
//! view are rendered, and rendered pages are cached, so long documents open
//! and scroll smoothly. The viewer is one continuous vertical scroll with
//! fit-width / fit-page / custom zoom and page navigation.

use std::{
    collections::HashMap,
    ops::Range,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use gpui_kit::{
    App, Context, ElementId, FocusHandle, Focusable, Image, ImageFormat, InteractiveElement as _,
    IntoElement, ParentElement as _, Pixels, Render, ScrollStrategy, Size, Styled as _, Window,
    assets::IconName,
    base::Disableable as _,
    component::{
        ActiveTheme as _, Icon, Sizable as _, VirtualListScrollHandle,
        button::{Button, ButtonVariants as _},
        h_flex, v_flex, v_virtual_list,
    },
    div, img,
    prelude::FluentBuilder as _,
    px, rgb,
};

use hayro_svg::{
    RenderCache, SvgRenderSettings, convert,
    hayro_interpret::{
        InterpreterSettings,
        font::{FontData, FontQuery, StandardFont},
        hayro_syntax::Pdf,
    },
};

use crate::actions::{ZoomIn, ZoomOut, ZoomReset};

/// Default zoom factor (100%).
pub const DEFAULT_ZOOM: f32 = 1.0;
/// Minimum allowed zoom (10%).
pub const MIN_ZOOM: f32 = 0.1;
/// Maximum allowed zoom (800%).
pub const MAX_ZOOM: f32 = 8.0;
/// Multiplier for zoom in / out steps.
pub const ZOOM_STEP: f32 = 1.25;

/// Gap between pages, in logical pixels.
const PAGE_GAP: f32 = 20.;
/// Margin around the page column, in logical pixels.
const PAGE_MARGIN: f32 = 24.;

/// How the zoom factor is chosen.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    /// A user-set factor, from the zoom buttons.
    Custom,
    /// The widest page fills the viewport width.
    Width,
    /// The first page fills the viewport.
    Page,
}

/// A page's fixed geometry, known without rendering it.
#[derive(Clone)]
pub struct PdfPage {
    /// 0-indexed page number in the document.
    pub index: usize,
    /// 1-indexed human-readable page number.
    pub page_number: usize,
    /// Width in PDF points.
    pub width: f32,
    /// Height in PDF points.
    pub height: f32,
}

/// A parsed PDF: its pages' geometry, plus lazily rendered SVGs.
pub struct PdfDocument {
    pdf: Pdf,
    pages: Vec<PdfPage>,
    rendered: HashMap<usize, Arc<Image>>,
    interpreter_settings: InterpreterSettings,
    render_settings: SvgRenderSettings,
}

/// Create default interpreter settings using Typst's bundled Foxit standard fonts.
fn default_interpreter_settings() -> InterpreterSettings {
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

impl PdfDocument {
    /// Parse `bytes` into a document. Pages are measured now and rendered
    /// lazily, so a long document is cheap to open.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        let pdf = Pdf::new(bytes).map_err(|err| format!("Failed to parse PDF: {err:?}"))?;
        let pages = pdf
            .pages()
            .iter()
            .enumerate()
            .map(|(index, page)| {
                let (width, height) = page.render_dimensions();
                PdfPage {
                    index,
                    page_number: index + 1,
                    width,
                    height,
                }
            })
            .collect();
        Ok(Self {
            pdf,
            pages,
            rendered: HashMap::new(),
            interpreter_settings: default_interpreter_settings(),
            render_settings: SvgRenderSettings {
                bg_color: [255, 255, 255, 255],
            },
        })
    }

    /// The page geometry, in document order.
    pub fn pages(&self) -> &[PdfPage] {
        &self.pages
    }

    /// Total number of pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// The rendered SVG image for page `index`, rendering and caching it the
    /// first time it is asked for.
    fn image(&mut self, index: usize) -> Option<Arc<Image>> {
        if let Some(image) = self.rendered.get(&index) {
            return Some(image.clone());
        }
        let page = self.pdf.pages().get(index)?;
        let cache = RenderCache::new();
        let svg = convert(
            page,
            &cache,
            &self.interpreter_settings,
            &self.render_settings,
        );
        let image = Arc::new(Image::from_bytes(ImageFormat::Svg, svg.into_bytes()));
        self.rendered.insert(index, image.clone());
        Some(image)
    }
}

/// A GPUI view displaying a scrollable PDF document.
pub struct PdfView {
    path: Option<PathBuf>,
    title: String,
    doc: Option<PdfDocument>,
    fit: FitMode,
    /// The factor actually used, kept in sync each frame for the toolbar.
    zoom: f32,
    /// The user's factor, used when `fit` is [`FitMode::Custom`].
    custom_zoom: f32,
    /// The page currently at the top of the viewport.
    current_page: usize,
    scroll: VirtualListScrollHandle,
    focus_handle: FocusHandle,
}

impl PdfView {
    /// Create a `PdfView` from a parsed document.
    pub fn new(path: Option<PathBuf>, doc: PdfDocument, cx: &mut Context<Self>) -> Self {
        let title = path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "PDF Document".to_string());

        Self {
            path,
            title,
            doc: Some(doc),
            fit: FitMode::Custom,
            zoom: DEFAULT_ZOOM,
            custom_zoom: DEFAULT_ZOOM,
            current_page: 0,
            scroll: VirtualListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
        }
    }

    /// The path of the open document, if opened from disk.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Document title shown in the toolbar.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Total number of pages.
    pub fn page_count(&self) -> usize {
        self.doc.as_ref().map_or(0, PdfDocument::page_count)
    }

    /// Current zoom level.
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// The zoom mode in effect.
    pub fn fit_mode(&self) -> FitMode {
        self.fit
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

    /// Set an explicit zoom factor, switching to custom zoom.
    pub fn set_zoom(&mut self, zoom: f32, cx: &mut Context<Self>) {
        let clamped = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        if self.fit != FitMode::Custom || (self.custom_zoom - clamped).abs() > f32::EPSILON {
            self.fit = FitMode::Custom;
            self.custom_zoom = clamped;
            self.zoom = clamped;
            cx.notify();
        }
    }

    /// Scale the widest page to the viewport width.
    pub fn fit_width(&mut self, cx: &mut Context<Self>) {
        if self.fit != FitMode::Width {
            self.fit = FitMode::Width;
            cx.notify();
        }
    }

    /// Scale the first page to fit the viewport.
    pub fn fit_page(&mut self, cx: &mut Context<Self>) {
        if self.fit != FitMode::Page {
            self.fit = FitMode::Page;
            cx.notify();
        }
    }

    /// Scroll so page `index` (0-based) is at the top.
    pub fn go_to_page(&mut self, index: usize, cx: &mut Context<Self>) {
        let count = self.page_count();
        if count == 0 {
            return;
        }
        let index = index.min(count - 1);
        self.current_page = index;
        self.scroll.scroll_to_item(index, ScrollStrategy::Top);
        cx.notify();
    }

    /// The zoom factor a fit mode resolves to for a given viewport.
    fn resolved_zoom(&self, viewport: Size<Pixels>) -> f32 {
        let Some(doc) = &self.doc else {
            return DEFAULT_ZOOM;
        };
        if doc.pages.is_empty() {
            return DEFAULT_ZOOM;
        }
        let avail_width = (viewport.width.as_f32() - 2. * PAGE_MARGIN - 12.).max(1.);
        let avail_height = (viewport.height.as_f32() - 2. * PAGE_MARGIN).max(1.);
        match self.fit {
            FitMode::Custom => self.custom_zoom.clamp(MIN_ZOOM, MAX_ZOOM),
            FitMode::Width => {
                let widest = doc.pages.iter().fold(1.0f32, |w, p| w.max(p.width));
                (avail_width / widest).clamp(MIN_ZOOM, MAX_ZOOM)
            }
            FitMode::Page => {
                let page = &doc.pages[0];
                let scale = (avail_width / page.width).min(avail_height / page.height);
                scale.clamp(MIN_ZOOM, MAX_ZOOM)
            }
        }
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let zoom_pct = format!("{:.0}%", self.zoom * 100.0);
        let total = self.page_count();
        let current = (self.current_page + 1).min(total.max(1));

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
                    .when(total > 0, |this| {
                        this.child(div().text_xs().text_color(theme.muted_foreground).child(
                            format!("({total} {})", if total == 1 { "page" } else { "pages" }),
                        ))
                    }),
            )
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(
                        Button::new("pdf-fit-width")
                            .ghost()
                            .small()
                            .compact()
                            .label("Fit width")
                            .tooltip("Fit page width")
                            .toggled(self.fit == FitMode::Width)
                            .disabled(total == 0)
                            .on_click(cx.listener(|this, _, _, cx| this.fit_width(cx))),
                    )
                    .child(
                        Button::new("pdf-fit-page")
                            .ghost()
                            .small()
                            .compact()
                            .label("Fit page")
                            .tooltip("Fit whole page")
                            .toggled(self.fit == FitMode::Page)
                            .disabled(total == 0)
                            .on_click(cx.listener(|this, _, _, cx| this.fit_page(cx))),
                    )
                    .child(
                        Button::new("pdf-zoom-reset")
                            .ghost()
                            .small()
                            .compact()
                            .label(zoom_pct)
                            .tooltip("Actual size (100%)")
                            .toggled(self.fit == FitMode::Custom)
                            .disabled(total == 0)
                            .on_click(cx.listener(|this, _, _, cx| this.reset_zoom(cx))),
                    )
                    .child(
                        Button::new("pdf-zoom-out")
                            .ghost()
                            .small()
                            .compact()
                            .icon(Icon::new(IconName::ZoomOut))
                            .tooltip("Zoom out")
                            .disabled(total == 0 || self.zoom <= MIN_ZOOM)
                            .on_click(cx.listener(|this, _, _, cx| this.zoom_out(cx))),
                    )
                    .child(
                        Button::new("pdf-zoom-in")
                            .ghost()
                            .small()
                            .compact()
                            .icon(Icon::new(IconName::ZoomIn))
                            .tooltip("Zoom in")
                            .disabled(total == 0 || self.zoom >= MAX_ZOOM)
                            .on_click(cx.listener(|this, _, _, cx| this.zoom_in(cx))),
                    )
                    .child(div().w(px(1.)).h_4().bg(theme.border).mx_1())
                    .child(
                        Button::new("pdf-prev-page")
                            .ghost()
                            .small()
                            .compact()
                            .icon(Icon::new(IconName::ChevronUp))
                            .tooltip("Previous page")
                            .disabled(total == 0 || current <= 1)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let page = this.current_page.saturating_sub(1);
                                this.go_to_page(page, cx);
                            })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .min_w(px(72.))
                            .text_center()
                            .child(format!("Page {current} / {total}")),
                    )
                    .child(
                        Button::new("pdf-next-page")
                            .ghost()
                            .small()
                            .compact()
                            .icon(Icon::new(IconName::ChevronDown))
                            .tooltip("Next page")
                            .disabled(total == 0 || current >= total)
                            .on_click(cx.listener(|this, _, _, cx| {
                                let page = this.current_page + 1;
                                this.go_to_page(page, cx);
                            })),
                    ),
            )
    }

    fn render_content(&self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let theme = cx.theme();

        let Some(doc) = &self.doc else {
            return self.render_empty("No PDF loaded.", theme.muted_foreground);
        };
        if doc.pages.is_empty() {
            return self.render_empty("This PDF has no pages.", theme.muted_foreground);
        }

        let zoom = self.zoom;
        let total = doc.pages.len();
        let viewport_width = self.scroll.bounds().size.width.as_f32();
        let sizes: Rc<Vec<Size<Pixels>>> = Rc::new(
            doc.pages
                .iter()
                .map(|page| {
                    gpui_kit::size(px(page.width * zoom), px(page.height * zoom + PAGE_GAP))
                })
                .collect(),
        );
        // Measure the widest page, so the content can be wider than the
        // viewport and zoomed-in pages stay horizontally reachable.
        let measure_ix = doc
            .pages
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.width.total_cmp(&b.1.width))
            .map(|(ix, _)| ix)
            .unwrap_or(0);

        let border = theme.border;
        let muted = theme.muted_foreground;

        v_virtual_list(
            cx.entity(),
            "pdf-pages",
            sizes,
            move |this, range: Range<usize>, _window, _cx| {
                if let Some(doc) = &this.doc {
                    this.current_page = range.start.min(doc.pages.len().saturating_sub(1));
                }
                range
                    .filter_map(|ix| {
                        let page = this.doc.as_ref()?.pages.get(ix)?.clone();
                        let width = px(page.width * zoom);
                        let height = px(page.height * zoom);
                        let row_width = px((page.width * zoom).max(viewport_width));
                        let image = this.doc.as_mut()?.image(ix);
                        let card = match image {
                            Some(image) => div()
                                .w(width)
                                .h(height)
                                .bg(rgb(0xffffff))
                                .border_1()
                                .border_color(border)
                                .shadow_md()
                                .rounded_sm()
                                .overflow_hidden()
                                .child(img(image).w(width).h(height))
                                .into_any_element(),
                            None => div()
                                .w(width)
                                .h(height)
                                .bg(rgb(0xffffff))
                                .border_1()
                                .border_color(border)
                                .rounded_sm()
                                .flex()
                                .items_center()
                                .justify_center()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("Page {}", page.page_number))
                                .into_any_element(),
                        };
                        Some(
                            v_flex()
                                .id(ElementId::NamedInteger("pdf-page".into(), ix as u64))
                                .w(row_width)
                                .items_center()
                                .pb(px(PAGE_GAP))
                                .child(card)
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(format!("{} / {}", page.page_number, total)),
                                )
                                .into_any_element(),
                        )
                    })
                    .collect::<Vec<_>>()
            },
        )
        .with_item_to_measure_index(measure_ix)
        .track_scroll(&self.scroll)
        .bg(theme.background)
        .into_any_element()
    }

    fn render_empty(&self, message: &str, muted: gpui_kit::Hsla) -> gpui_kit::AnyElement {
        v_flex()
            .id("pdf-empty")
            .size_full()
            .items_center()
            .justify_center()
            .p_8()
            .text_sm()
            .text_color(muted)
            .child(message.to_string())
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
        // Resolve fit modes against the live viewport before drawing the
        // toolbar, so the percentage shown matches the pages below it.
        let viewport = self.scroll.bounds().size;
        self.zoom = self.resolved_zoom(viewport);

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
    fn parses_and_measures_minimal_pdf() {
        // Generate a minimal valid PDF using Typst's world & compiler
        let markup = "= Hello Malgel PDF\n\nThis is a test of Hayro PDF rendering.";
        let doc = crate::typst_world::compile(
            markup.into(),
            std::collections::HashMap::new(),
            &crate::typst_world::BUNDLED_FONTS,
        )
        .expect("compile typst doc");

        let pdf_bytes =
            typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default()).expect("generate pdf bytes");

        let mut document = PdfDocument::from_bytes(pdf_bytes).expect("parse pdf");
        assert_eq!(document.page_count(), 1);
        assert_eq!(document.pages()[0].page_number, 1);
        assert!(document.pages()[0].width > 0.0);
        assert!(document.pages()[0].height > 0.0);
        // Rendering a page on demand produces an SVG-backed image.
        assert!(document.image(0).is_some());
        assert!(document.image(1).is_none());
    }

    #[test]
    fn reports_error_on_invalid_pdf_bytes() {
        let invalid = b"NOT A VALID PDF FILE".to_vec();
        assert!(PdfDocument::from_bytes(invalid).is_err());
    }
}
