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

/// The page indexes each row shows, in order: one per row, or `per_row`.
fn page_rows(count: usize, per_row: usize) -> Vec<Vec<usize>> {
    let per_row = per_row.max(1);
    (0..count)
        .step_by(per_row)
        .map(|start| (start..(start + per_row).min(count)).collect())
        .collect()
}

/// The zoom a fit mode resolves to for a viewport, given every row of page
/// sizes as (width, height) pairs. A row holds one page, or two side by side.
fn fit_zoom(
    mode: FitMode,
    custom_zoom: f32,
    rows: &[Vec<(f32, f32)>],
    viewport: (f32, f32),
) -> f32 {
    let avail_width = (viewport.0 - 2. * PAGE_MARGIN - 12.).max(1.);
    let avail_height = (viewport.1 - 2. * PAGE_MARGIN).max(1.);
    let row_width = |row: &[(f32, f32)]| {
        row.iter().map(|(width, _)| *width).sum::<f32>()
            + PAGE_GAP * row.len().saturating_sub(1) as f32
    };
    match mode {
        FitMode::Custom => custom_zoom.clamp(MIN_ZOOM, MAX_ZOOM),
        FitMode::Width => {
            let widest = rows.iter().map(|row| row_width(row)).fold(1.0f32, f32::max);
            (avail_width / widest).clamp(MIN_ZOOM, MAX_ZOOM)
        }
        FitMode::Page => {
            let first = rows.first().map(Vec::as_slice).unwrap_or(&[]);
            let width = row_width(first).max(1.);
            let height = first
                .iter()
                .map(|(_, height)| *height)
                .fold(1.0f32, f32::max);
            (avail_width / width)
                .min(avail_height / height)
                .clamp(MIN_ZOOM, MAX_ZOOM)
        }
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
    /// Whether two pages share a row, the way a bound document reads.
    spread: bool,
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
            spread: false,
            current_page: 0,
            scroll: VirtualListScrollHandle::new(),
            focus_handle: cx.focus_handle(),
        }
    }

    /// Whether two pages are shown side by side.
    pub fn spread(&self) -> bool {
        self.spread
    }

    /// Show two pages side by side, or one at a time.
    pub fn set_spread(&mut self, spread: bool, cx: &mut Context<Self>) {
        if self.spread != spread {
            self.spread = spread;
            cx.notify();
        }
    }

    /// How many pages share a row.
    fn per_row(&self) -> usize {
        if self.spread { 2 } else { 1 }
    }

    /// The page indexes each row shows, in order.
    fn rows(&self) -> Vec<Vec<usize>> {
        page_rows(self.page_count(), self.per_row())
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
        // Rows hold one or two pages; scroll to the row showing this one.
        self.scroll
            .scroll_to_item(index / self.per_row(), ScrollStrategy::Top);
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
        let rows: Vec<Vec<(f32, f32)>> = self
            .rows()
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|ix| (doc.pages[ix].width, doc.pages[ix].height))
                    .collect()
            })
            .collect();
        fit_zoom(
            self.fit,
            self.custom_zoom,
            &rows,
            (viewport.width.as_f32(), viewport.height.as_f32()),
        )
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
                        Button::new("pdf-spread")
                            .ghost()
                            .small()
                            .compact()
                            .label("Two pages")
                            .tooltip("Show two pages side by side")
                            .toggled(self.spread)
                            .disabled(total == 0)
                            .on_click(
                                cx.listener(|this, _, _, cx| this.set_spread(!this.spread, cx)),
                            ),
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
        // One row per page, or per pair when the pages are spread out.
        let rows: Rc<Vec<Vec<usize>>> = Rc::new(self.rows());
        let row_width = |row: &[usize]| {
            row.iter().map(|&ix| doc.pages[ix].width).sum::<f32>()
                + PAGE_GAP * row.len().saturating_sub(1) as f32
        };
        let row_height = |row: &[usize]| {
            row.iter()
                .map(|&ix| doc.pages[ix].height)
                .fold(0.0f32, f32::max)
        };
        let sizes: Rc<Vec<Size<Pixels>>> = Rc::new(
            rows.iter()
                .map(|row| {
                    gpui_kit::size(
                        px(row_width(row) * zoom),
                        px(row_height(row) * zoom + PAGE_GAP),
                    )
                })
                .collect(),
        );
        // Measure the widest row, so the content can be wider than the
        // viewport and zoomed-in pages stay horizontally reachable.
        let measure_ix = rows
            .iter()
            .enumerate()
            .max_by(|a, b| row_width(a.1).total_cmp(&row_width(b.1)))
            .map(|(ix, _)| ix)
            .unwrap_or(0);

        let border = theme.border;
        let muted = theme.muted_foreground;

        v_virtual_list(
            cx.entity(),
            "pdf-pages",
            sizes,
            move |this, range: Range<usize>, _window, _cx| {
                if let Some(first) = rows.get(range.start).and_then(|row| row.first())
                    && let Some(doc) = &this.doc
                {
                    this.current_page = (*first).min(doc.pages.len().saturating_sub(1));
                }
                range
                    .filter_map(|row_ix| {
                        let row = rows.get(row_ix)?.clone();
                        let pages = row
                            .iter()
                            .filter_map(|&ix| this.doc.as_ref()?.pages.get(ix).cloned())
                            .collect::<Vec<PdfPage>>();
                        let width: f32 = pages.iter().map(|page| page.width).sum::<f32>()
                            + PAGE_GAP * pages.len().saturating_sub(1) as f32;
                        let row_width = px((width * zoom).max(viewport_width));
                        let cards = pages
                            .iter()
                            .zip(&row)
                            .map(|(page, &page_ix)| {
                                let width = px(page.width * zoom);
                                let height = px(page.height * zoom);
                                match this.doc.as_mut().and_then(|doc| doc.image(page_ix)) {
                                    Some(image) => div()
                                        .w(width)
                                        .h(height)
                                        .flex_none()
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
                                        .flex_none()
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
                                }
                            })
                            .collect::<Vec<_>>();
                        let first = pages.first()?.page_number;
                        Some(
                            v_flex()
                                .id(ElementId::NamedInteger("pdf-page".into(), row_ix as u64))
                                .w(row_width)
                                .items_center()
                                .pb(px(PAGE_GAP))
                                .child(
                                    cards
                                        .into_iter()
                                        .fold(h_flex().gap_2().items_start(), |row, card| {
                                            row.child(card)
                                        }),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(format!("{first} / {total}")),
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

    #[test]
    fn page_rows_pairs_pages_only_when_spread() {
        assert_eq!(page_rows(4, 1), vec![vec![0], vec![1], vec![2], vec![3]]);
        assert_eq!(page_rows(5, 2), vec![vec![0, 1], vec![2, 3], vec![4]]);
        assert_eq!(page_rows(1, 2), vec![vec![0]]);
        assert!(page_rows(0, 2).is_empty());
    }

    #[test]
    fn fit_width_shares_the_viewport_between_two_pages() {
        let page = (600.0, 800.0);
        let viewport = (1260.0, 900.0);
        // 1260 - 2 * 24 - 12 = 1200 usable, so one page fits at 2x…
        let single = fit_zoom(FitMode::Width, 1.0, &[vec![page], vec![page]], viewport);
        assert!((single - 2.0).abs() < 0.001, "got {single}");
        // …and a pair sits at nearly half of that, leaving room for the
        // page beside it.
        let spread = fit_zoom(FitMode::Width, 1.0, &[vec![page, page]], viewport);
        assert!((spread - 0.9836).abs() < 0.001, "got {spread}");
    }

    #[test]
    fn fit_page_measures_the_whole_row() {
        let page = (600.0, 800.0);
        // 1400 - 60 = 1340 wide, 700 - 48 = 652 high: the height binds.
        let zoom = fit_zoom(FitMode::Page, 1.0, &[vec![page, page]], (1400.0, 700.0));
        assert!((zoom - 0.815).abs() < 0.001, "got {zoom}");
    }

    #[test]
    fn custom_zoom_passes_through_the_clamp() {
        assert_eq!(fit_zoom(FitMode::Custom, 3.5, &[], (1000.0, 800.0)), 3.5);
        assert_eq!(
            fit_zoom(FitMode::Custom, 100.0, &[], (1000.0, 800.0)),
            MAX_ZOOM
        );
    }
}
