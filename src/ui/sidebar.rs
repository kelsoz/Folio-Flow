//! The sidebar: document outline and page thumbnails.
//!
//! The outline is widget-based and virtualised. The Pages tab uses a canvas so
//! page drags can be captured outside an individual row.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use iced::advanced::image;
use iced::advanced::text::{Alignment, LineHeight, Shaping};
use iced::mouse;
use iced::widget::canvas::{self, Action, Frame, Geometry};
use iced::widget::{button, column, container, row, scrollable, space, text};
use iced::{Center, Element, Fill, Padding, Point, Rectangle, Size, Task, border};

use crate::app::{App, EditAnimation, EditKind, Message, ViewMode};
use crate::engine::{DocInfo, PageSize};
use crate::motion::Slide;
use crate::ui::icon::Icon;
use crate::ui::order::PageOrder;
use crate::ui::theme::{self, Palette};
use crate::ui::{file_drop, font, viewer::Viewer};

pub const HEADER: f32 = 56.0;
const TAB_GROUP_WIDTH: f32 = 192.0;
const TAB_GROUP_HEIGHT: f32 = 42.0;
const TAB_INSET: f32 = 4.0;
const OUTLINE_ROW: f32 = 28.0;
const PAGE_MAX_WIDTH: f32 = 128.0;
const PAGE_MAX_HEIGHT: f32 = 152.0;
const PAGE_PADDING_Y: f32 = 6.0;
const PAGE_NUMBER_GAP: f32 = 4.0;
const PAGE_NUMBER_HEIGHT: f32 = 14.0;
const OVERSCAN: usize = 3;
const SCROLLBAR_WIDTH: f32 = 8.0;
const SCROLLBAR_INSET: f32 = 10.0;
const DRAG_SLACK: f32 = 6.0;
const AUTO_SCROLL_EDGE: f32 = 26.0;
const AUTO_SCROLL_STEP: f32 = 20.0;
const EXTERNAL_GAP: f32 = 48.0;

#[derive(Default)]
struct PageLayout {
    rows: Vec<f32>,
    tops: Vec<f32>,
    height: f32,
}

impl PageLayout {
    fn new(pages: &[PageSize], width: f32, order: &PageOrder, document_order: &PageOrder) -> Self {
        let max_width = (width - 32.0).clamp(80.0, PAGE_MAX_WIDTH);
        let mut rows = Vec::with_capacity(order.as_sources().len());
        let mut tops = Vec::with_capacity(order.as_sources().len());
        let mut y = 0.0;

        for (slot, source) in order.as_sources().iter().copied().enumerate() {
            let document_slot = document_order.slot_of(source).unwrap_or(slot as u32) as usize;
            let row = pages.get(document_slot).map_or(0.0, |size| {
                let (_, height) = fit_page(*size, max_width, PAGE_MAX_HEIGHT);
                height + PAGE_PADDING_Y * 2.0 + PAGE_NUMBER_GAP + PAGE_NUMBER_HEIGHT
            });
            tops.push(y);
            rows.push(row);
            y += row;
        }

        Self {
            rows,
            tops,
            height: y,
        }
    }

    fn row_height(&self, slot: u32) -> f32 {
        self.rows.get(slot as usize).copied().unwrap_or(0.0)
    }

    fn slot_top(&self, slot: u32) -> f32 {
        self.tops.get(slot as usize).copied().unwrap_or(self.height)
    }

    fn slot_at(&self, y: f32) -> u32 {
        if self.tops.is_empty() {
            return 0;
        }

        self.tops
            .partition_point(|top| *top <= y)
            .saturating_sub(1)
            .min(self.tops.len() - 1) as u32
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Pages,
    Outline,
}

impl Tab {
    fn scroll_id(self) -> iced::widget::Id {
        iced::widget::Id::new(match self {
            Tab::Outline => "sidebar-outline",
            Tab::Pages => "sidebar-pages",
        })
    }
}

pub struct State {
    pub open: bool,
    pub drop_bounds: Rectangle,
    pub tab: Tab,
    pub offset: f32,
    wheel: Option<crate::motion::WheelScroll>,
    pub height: f32,
    pages_size: Size,
    page_sizes: Vec<PageSize>,
    page_count: usize,
    expanded: HashSet<usize>,
    rows: Vec<usize>,
    active: Option<usize>,
    followed_page: Option<u32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            open: false,
            drop_bounds: Rectangle::default(),
            tab: Tab::Pages,
            offset: 0.0,
            wheel: None,
            height: 1200.0,
            pages_size: Size::ZERO,
            page_sizes: Vec::new(),
            page_count: 0,
            expanded: HashSet::new(),
            rows: Vec::new(),
            active: None,
            followed_page: None,
        }
    }
}

pub fn reset_scroll() -> Task<Message> {
    Task::batch([Tab::Outline, Tab::Pages].map(|tab| {
        iced::widget::operation::snap_to(tab.scroll_id(), scrollable::RelativeOffset::START)
    }))
}

fn has_children(doc: &DocInfo, index: usize) -> bool {
    doc.outline
        .get(index + 1)
        .is_some_and(|next| next.depth > doc.outline[index].depth)
}

impl State {
    pub fn set_document(&mut self, doc: &DocInfo) {
        self.wheel = None;
        self.update_document(doc);
        self.offset = 0.0;
    }

    pub fn update_document(&mut self, doc: &DocInfo) {
        self.expanded = if doc.outline.len() <= 60 {
            (0..doc.outline.len()).collect()
        } else {
            HashSet::new()
        };
        self.page_count = doc.pages.len();
        self.page_sizes = doc.pages.clone();
        self.active = None;
        self.followed_page = None;
        self.rebuild(doc);
    }

    pub fn scroll_for_insertion(
        &mut self,
        delta: f32,
        order: &PageOrder,
        document_order: &PageOrder,
    ) {
        let layout = PageLayout::new(
            &self.page_sizes,
            self.pages_size.width,
            order,
            document_order,
        );
        self.offset = (self.offset + delta).clamp(
            0.0,
            (layout.height + EXTERNAL_GAP - self.pages_size.height).max(0.0),
        );
    }

    pub fn insertion_slot(
        &self,
        y: f32,
        order: &PageOrder,
        document_order: &PageOrder,
        preview: Option<u32>,
    ) -> u32 {
        let layout = PageLayout::new(
            &self.page_sizes,
            self.pages_size.width,
            order,
            document_order,
        );
        let mut y = y + self.offset;
        if let Some(gap) = preview {
            let start = layout.slot_top(gap);
            if (start..=start + EXTERNAL_GAP).contains(&y) {
                return gap;
            }
            if y > start {
                y -= EXTERNAL_GAP;
            }
        }
        let slot = layout.slot_at(y);
        (slot + u32::from(y > layout.slot_top(slot) + layout.row_height(slot) / 2.0))
            .min(order.as_sources().len() as u32)
    }

    pub(crate) fn page_tops(&self, order: &PageOrder, document_order: &PageOrder) -> Vec<f32> {
        PageLayout::new(
            &self.page_sizes,
            self.pages_size.width,
            order,
            document_order,
        )
        .tops
    }

    pub fn show(&mut self, tab: Tab) {
        if self.tab != tab {
            self.wheel = None;
            self.tab = tab;
            self.offset = 0.0;
        }
    }

    pub fn toggle(&mut self, doc: Option<&DocInfo>, index: usize) {
        if !self.expanded.remove(&index) {
            self.expanded.insert(index);
        }

        if let Some(doc) = doc {
            self.rebuild(doc);
        }
    }

    fn rebuild(&mut self, doc: &DocInfo) {
        self.rows.clear();
        let mut hidden_below: Option<u16> = None;

        for (index, item) in doc.outline.iter().enumerate() {
            if let Some(depth) = hidden_below {
                if item.depth > depth {
                    continue;
                }
                hidden_below = None;
            }

            self.rows.push(index);

            if !self.expanded.contains(&index) {
                hidden_below = Some(item.depth);
            }
        }
    }

    pub fn follow(&mut self, doc: Option<&DocInfo>, page: u32) {
        if self.followed_page == Some(page) {
            return;
        }

        self.followed_page = Some(page);
        self.active = doc.and_then(|doc| {
            doc.outline
                .iter()
                .enumerate()
                .filter(|(_, item)| item.page.is_some_and(|target| target <= page))
                .max_by_key(|(index, item)| (item.page, *index))
                .map(|(index, _)| index)
        });
    }

    fn active_row(&self) -> Option<usize> {
        let active = self.active?;

        self.rows
            .partition_point(|row| *row <= active)
            .checked_sub(1)
            .map(|position| self.rows[position])
    }

    pub fn reveal_source(&mut self, source: u32, order: &PageOrder, document_order: &PageOrder) {
        if !self.open || self.tab != Tab::Pages {
            return;
        }

        let Some(slot) = order.slot_of(source) else {
            return;
        };
        let layout = PageLayout::new(
            &self.page_sizes,
            self.pages_size.width,
            order,
            document_order,
        );
        let top = layout.slot_top(slot);
        let row = layout.row_height(slot);

        if top >= self.offset && top + row <= self.offset + self.pages_size.height {
            return;
        }

        self.offset = (top - (self.pages_size.height - row) / 2.0).max(0.0);
        self.clamp_pages(order, document_order);
    }

    pub fn set_pages_size(
        &mut self,
        size: Size,
        page_count: u32,
        order: &PageOrder,
        document_order: &PageOrder,
    ) {
        let first = self.pages_size == Size::ZERO;
        self.pages_size = size;
        self.height = size.height;
        self.page_count = page_count as usize;
        self.clamp_pages(order, document_order);

        if first && self.open && self.tab == Tab::Pages {
            self.offset = 0.0;
        }
    }

    pub fn stop_scroll(&mut self) {
        self.wheel = None;
    }

    pub fn scrolling(&self) -> bool {
        self.wheel.is_some()
    }

    pub fn wheel_by(&mut self, delta: f32, order: &PageOrder, document_order: &PageOrder) {
        let maximum = if self.tab == Tab::Pages {
            let layout = PageLayout::new(
                &self.page_sizes,
                self.pages_size.width,
                order,
                document_order,
            );
            (layout.height - self.pages_size.height).max(0.0)
        } else {
            (self.rows.len() as f32 * OUTLINE_ROW + 8.0 - self.height).max(0.0)
        };
        self.wheel
            .get_or_insert_with(|| crate::motion::WheelScroll::new(self.offset))
            .push(delta, maximum);
    }

    pub fn outline_scroll_task(&self) -> Task<Message> {
        if self.tab != Tab::Outline {
            return Task::none();
        }
        iced::widget::operation::scroll_to(
            Tab::Outline.scroll_id(),
            scrollable::AbsoluteOffset {
                x: 0.0,
                y: self.offset.max(0.0),
            },
        )
    }

    pub fn advance_scroll(&mut self, now: Instant) {
        if let Some(wheel) = &mut self.wheel
            && !wheel.advance(&mut self.offset, now)
        {
            self.wheel = None;
        }
    }

    pub fn scroll_by(&mut self, delta: f32, order: &PageOrder, document_order: &PageOrder) {
        self.offset += delta;
        self.clamp_pages(order, document_order);
    }

    pub fn clamp_pages(&mut self, order: &PageOrder, document_order: &PageOrder) {
        self.wheel = None;
        let layout = PageLayout::new(
            &self.page_sizes,
            self.pages_size.width,
            order,
            document_order,
        );
        self.offset = self
            .offset
            .clamp(0.0, (layout.height - self.pages_size.height).max(0.0));
    }

    pub fn visible_rows(&self, count: usize) -> Range<usize> {
        let first = ((self.offset / OUTLINE_ROW) as usize).saturating_sub(OVERSCAN);
        let last = first + (self.height / OUTLINE_ROW).ceil() as usize + 2 * OVERSCAN;

        first.min(count)..last.min(count)
    }

    pub fn visible_page_slots(
        &self,
        order: &PageOrder,
        document_order: &PageOrder,
    ) -> Range<usize> {
        let layout = PageLayout::new(
            &self.page_sizes,
            self.pages_size.width,
            order,
            document_order,
        );
        let first = (layout.slot_at(self.offset) as usize).saturating_sub(OVERSCAN);
        let last = (layout.slot_at(self.offset + self.pages_size.height) as usize + OVERSCAN + 2)
            .min(layout.tops.len());

        first.min(layout.tops.len())..last
    }
}

pub fn view(app: &App, width: f32) -> Element<'_, Message> {
    let palette = app.palette;
    let state = &app.sidebar;

    let group_width = (width - 24.0).clamp(1.0, TAB_GROUP_WIDTH);
    let tab_width = (group_width - TAB_INSET * 2.0) / 2.0;
    let tab_height = TAB_GROUP_HEIGHT - TAB_INSET * 2.0;
    let tab = |tab: Tab, label: &'static str| {
        let active = state.tab == tab;
        let color = if active {
            palette.on_accent
        } else {
            palette.muted
        };

        button(
            container(text(label).size(theme::TEXT_SMALL).color(color))
                .width(Fill)
                .height(Fill)
                .align_x(Center)
                .align_y(Center),
        )
        .padding(0)
        .width(tab_width)
        .height(tab_height)
        .on_press(Message::ShowTab(tab))
        .style(move |_, status| {
            let mut style = theme::ghost_button(palette, status, active);
            style.border = border::rounded(theme::TAB_RADIUS);
            style
        })
    };

    let tabs = container(
        row![tab(Tab::Pages, "页面"), tab(Tab::Outline, "书签")]
            .width(Fill)
            .height(Fill),
    )
    .width(group_width)
    .height(TAB_GROUP_HEIGHT)
    .padding(TAB_INSET)
    .style(move |_| container::Style {
        background: Some(palette.raised.into()),
        border: border::rounded(theme::TAB_GROUP_RADIUS),
        ..container::Style::default()
    });

    let header = container(tabs)
        .width(Fill)
        .height(HEADER)
        .padding(Padding {
            top: 14.0,
            right: 12.0,
            bottom: 0.0,
            left: 12.0,
        })
        .align_x(Center);

    let body: Element<'_, Message> = match (app.viewer.document(), state.tab) {
        (None, _) => empty(palette, "尚未打开文档"),
        (Some(doc), Tab::Outline) if doc.outline.is_empty() => empty(palette, "此文档没有书签"),
        (Some(doc), Tab::Outline) => {
            let active = state.active_row();

            list(state, palette, state.rows.len(), |row| {
                let index = state.rows[row];
                outline_row(state, palette, doc, index, active == Some(index))
            })
        }
        (Some(_), Tab::Pages) => pages_view(app, width),
    };

    container(column![header, body])
        .width(width)
        .height(Fill)
        .style(move |_| theme::floating_panel(palette))
        .into()
}

fn empty<'a>(palette: Palette, message: &'a str) -> Element<'a, Message> {
    container(text(message).size(theme::TEXT_SMALL).color(palette.faint))
        .center_x(Fill)
        .padding(24)
        .into()
}

fn list<'a>(
    state: &State,
    palette: Palette,
    count: usize,
    build: impl Fn(usize) -> Element<'a, Message>,
) -> Element<'a, Message> {
    let visible = state.visible_rows(count);
    let mut rows = column![space().height(visible.start as f32 * OUTLINE_ROW)];

    for row in visible.clone() {
        rows = rows.push(build(row));
    }

    rows = rows.push(space().height((count - visible.end) as f32 * OUTLINE_ROW));

    let content = scrollable(container(rows).padding(iced::Padding {
        top: 0.0,
        right: 12.0,
        bottom: 8.0,
        left: 8.0,
    }))
    .id(Tab::Outline.scroll_id())
    .direction(scrollable::Direction::Vertical(
        scrollable::Scrollbar::new()
            .width(8)
            .scroller_width(8)
            .margin(2),
    ))
    .on_scroll(Message::SidebarScrolled)
    .width(Fill)
    .height(Fill)
    .style(move |_, status| theme::scroller(palette, status))
    .into();
    let maximum = (count as f32 * OUTLINE_ROW + 8.0 - state.height).max(0.0);
    super::wheel_gate::wrap(content, state.offset.clamp(0.0, maximum) - state.offset)
}

fn outline_row<'a>(
    state: &State,
    palette: Palette,
    doc: &'a DocInfo,
    index: usize,
    active: bool,
) -> Element<'a, Message> {
    let item = &doc.outline[index];

    let disclosure: Element<'a, Message> = if has_children(doc, index) {
        let icon = if state.expanded.contains(&index) {
            Icon::ChevronDown
        } else {
            Icon::ChevronRight
        };

        button(container(icon.view(13.0, palette.faint)).center(18))
            .padding(0)
            .on_press(Message::ToggleOutline(index))
            .style(move |_, status| theme::ghost_button(palette, status, false))
            .into()
    } else {
        space().width(18).into()
    };

    let label = container(
        text(item.title.as_str())
            .size(theme::TEXT)
            .wrapping(text::Wrapping::None),
    )
    .width(Fill)
    .height(Fill)
    .align_y(Center)
    .clip(true);

    let number = item.page.map(|page| {
        container(text(page + 1).size(11).color(palette.faint))
            .height(Fill)
            .align_y(Center)
    });

    button(
        row![
            space().width(item.depth.min(8) as f32 * 12.0),
            disclosure,
            label,
        ]
        .push(number)
        .spacing(6)
        .align_y(Center),
    )
    .padding([0, 6])
    .width(Fill)
    .height(OUTLINE_ROW)
    .on_press_maybe(item.page.map(Message::GoTo))
    .style(move |_, status| theme::row_button(palette, status, active))
    .into()
}

fn pages_view(app: &App, width: f32) -> Element<'_, Message> {
    let mut layout = app
        .viewer
        .document()
        .map_or_else(PageLayout::default, |doc| {
            PageLayout::new(&doc.pages, width, &app.order, &app.viewer_order)
        });
    let external_slot = app.external_drop.slot(file_drop::Zone::Sidebar);
    if let Some(slot) = external_slot {
        for top in layout.tops.iter_mut().skip(slot as usize) {
            *top += EXTERNAL_GAP;
        }
        layout.height += EXTERNAL_GAP;
    }
    canvas::Canvas::new(Canvas {
        external_slot,
        external_label: app.external_drop.label(),
        preserve_reorder: app
            .viewer
            .document()
            .is_some_and(|doc| app.external_drop.settling_doc == Some(doc.id)),
        viewer: &app.viewer,
        state: &app.sidebar,
        order: &app.order,
        document_order: &app.viewer_order,
        layout,
        view_mode: app.view_mode,
        current: match app.view_mode {
            ViewMode::Reader => app.viewer_order.source_at(app.viewer.current_page()),
            ViewMode::Organizer => app
                .organizer
                .focused()
                .unwrap_or_else(|| app.viewer_order.source_at(app.viewer.current_page())),
        },
        selected: (app.view_mode == ViewMode::Organizer).then_some(&app.organizer.selected),
        palette: app.palette,
        animation: app.edit_animation.as_ref(),
    })
    .width(Fill)
    .height(Fill)
    .into()
}

struct Canvas<'a> {
    viewer: &'a Viewer,
    state: &'a State,
    order: &'a PageOrder,
    document_order: &'a PageOrder,
    layout: PageLayout,
    view_mode: ViewMode,
    current: u32,
    selected: Option<&'a HashSet<u32>>,
    palette: Palette,
    animation: Option<&'a EditAnimation>,
    external_slot: Option<u32>,
    external_label: &'a str,
    preserve_reorder: bool,
}

#[derive(Default)]
struct CanvasState {
    size: Size,
    bounds: Rectangle,
    pressed: Option<Pressed>,
    previous: Option<iced::advanced::mouse::click::Click>,
    scroll_drag: Option<ScrollDrag>,
    drag_target: Option<u32>,
    previous_doc: Option<u64>,
    previous_view: Option<ViewMode>,
    previous_order: Vec<u32>,
    previous_tops: Vec<f32>,
    slides: HashMap<u32, Slide>,
    last_frame: Option<Instant>,
    animating: bool,
}

struct Pressed {
    slot: u32,
    source: u32,
    position: Point,
    button: mouse::Button,
}

struct ScrollDrag {
    position: Point,
    offset: f32,
}

impl CanvasState {
    fn advance_slides(
        &mut self,
        now: Instant,
        layout: &PageLayout,
        order: &PageOrder,
        animation: Option<&EditAnimation>,
    ) -> bool {
        if !self.previous_order.is_empty()
            && (self.previous_order != order.as_sources() || self.previous_tops != layout.tops)
        {
            let old_slots: HashMap<u32, u32> = self
                .previous_order
                .iter()
                .copied()
                .enumerate()
                .map(|(slot, source)| (source, slot as u32))
                .collect();

            for (slot, source) in order.as_sources().iter().copied().enumerate() {
                let old_slot = animation
                    .filter(|animation| animation.kind == EditKind::Delete)
                    .and_then(|animation| animation.previous_slot(slot as u32))
                    .or_else(|| old_slots.get(&source).copied());
                let Some(old_slot) = old_slot else {
                    continue;
                };
                let old_top = animation
                    .filter(|animation| animation.kind == EditKind::Delete)
                    .and_then(|animation| animation.sidebar_tops.get(old_slot as usize).copied())
                    .or_else(|| self.previous_tops.get(old_slot as usize).copied());
                let Some(old_top) = old_top else {
                    continue;
                };
                let dy = old_top - layout.slot_top(slot as u32);
                self.slides.entry(source).or_default().shift(0.0, dy);
            }
        }

        self.previous_order = order.as_sources().to_vec();
        self.previous_tops = layout.tops.clone();
        let dt = self
            .last_frame
            .map_or(0.0, |last| now.duration_since(last).as_secs_f32());
        self.last_frame = Some(now);
        self.slides.retain(|_, slide| slide.advance(dt));
        !self.slides.is_empty()
    }
}

impl canvas::Program<Message> for Canvas<'_> {
    type State = CanvasState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<Action<Message>> {
        match event {
            iced::Event::Window(iced::window::Event::RedrawRequested(now)) => {
                let resized = state.bounds != bounds;
                state.bounds = bounds;
                state.size = bounds.size();
                let doc = self.viewer.document().map(|doc| doc.id);
                if state.previous_doc != doc {
                    state.previous_doc = doc;
                    if self.animation.is_none() && !self.preserve_reorder {
                        state.previous_order.clear();
                        state.slides.clear();
                        state.last_frame = None;
                    }
                }
                if state.previous_view != Some(self.view_mode) {
                    state.previous_view = Some(self.view_mode);
                    state.animating = false;
                }
                let moving = state.advance_slides(*now, &self.layout, self.order, self.animation);

                if resized || moving != state.animating {
                    state.animating = moving;
                    return Some(Action::publish(Message::SidebarPagesFrame {
                        size: bounds.size(),
                        bounds,
                        animating: moving,
                    }));
                }
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                if *button != mouse::Button::Left {
                    return None;
                }
                let position = cursor.position_over(bounds)?;
                let point = local(position, bounds);

                if let Some(scrollbar) =
                    scrollbar_rect(bounds.size(), self.state.offset, &self.layout)
                    && scrollbar.contains(point)
                {
                    state.scroll_drag = Some(ScrollDrag {
                        position,
                        offset: self.state.offset,
                    });
                    return Some(Action::capture());
                }

                if let Some(slot) = hit_slot(point, bounds.size(), self.state.offset, &self.layout)
                {
                    state.pressed = Some(Pressed {
                        slot,
                        source: self.order.source_at(slot),
                        position,
                        button: *button,
                    });
                    state.drag_target = None;
                    return Some(Action::capture());
                }
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                if let Some(drag) = state.scroll_drag.as_ref() {
                    let offset = scroll_offset(*position, bounds.height, &self.layout, drag);
                    return Some(
                        Action::publish(Message::SidebarPagesScrolled(offset - self.state.offset))
                            .and_capture(),
                    );
                }

                if let Some(pressed) = state.pressed.as_ref()
                    && pressed.button == mouse::Button::Left
                    && pressed.position.distance(*position) > DRAG_SLACK
                {
                    let point = local(*position, bounds);
                    let slot = drop_slot(point.y + self.state.offset, &self.layout);
                    let scroll = if point.y < AUTO_SCROLL_EDGE {
                        -AUTO_SCROLL_STEP
                    } else if point.y > bounds.height - AUTO_SCROLL_EDGE {
                        AUTO_SCROLL_STEP
                    } else {
                        0.0
                    };
                    state.drag_target = Some(slot);

                    return Some(
                        Action::publish(Message::SidebarPageDrag {
                            source: pressed.source,
                            slot,
                            scroll,
                            commit: false,
                        })
                        .and_capture(),
                    );
                }
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(button)) => {
                let Some(position) = cursor.position() else {
                    state.pressed = None;
                    state.scroll_drag = None;
                    state.drag_target = None;
                    return None;
                };

                if let Some(drag) = state.scroll_drag.take() {
                    let offset = scroll_offset(position, bounds.height, &self.layout, &drag);
                    return Some(Action::publish(Message::SidebarPagesScrolled(
                        offset - self.state.offset,
                    )));
                }

                let pressed = state.pressed.take()?;

                if pressed.position.distance(position) > DRAG_SLACK {
                    let slot = state.drag_target.take().unwrap_or(pressed.slot);
                    return Some(Action::publish(Message::SidebarPageDrag {
                        source: pressed.source,
                        slot,
                        scroll: 0.0,
                        commit: true,
                    }));
                }
                state.drag_target = None;

                if pressed.button == *button {
                    let click =
                        iced::advanced::mouse::click::Click::new(position, *button, state.previous);
                    state.previous = Some(click);

                    return Some(Action::publish(Message::SidebarPageClicked {
                        source: pressed.source,
                    }));
                }
            }
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_over(bounds)?;

                return Some(
                    Action::publish(Message::SidebarWheel(wheel_scroll(delta))).and_capture(),
                );
            }
            _ => {}
        }

        None
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &iced::Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry<iced::Renderer>> {
        let mut frame = Frame::new(renderer, bounds.size());
        let layout = &self.layout;
        let page_count = self.viewer.page_count();
        let dragging = state.pressed.as_ref().is_some_and(|pressed| {
            cursor
                .position()
                .is_some_and(|position| pressed.position.distance(position) > DRAG_SLACK)
        });
        if let Some(doc) = self.viewer.document() {
            let current = self.current;
            let hovered = cursor
                .position_over(bounds)
                .and_then(|position| {
                    hit_slot(
                        local(position, bounds),
                        bounds.size(),
                        self.state.offset,
                        &self.layout,
                    )
                })
                .map(|slot| self.order.source_at(slot));
            let first = (layout.slot_at(self.state.offset) as usize).saturating_sub(OVERSCAN);
            let last = (layout.slot_at(self.state.offset + bounds.height) as usize + OVERSCAN + 2)
                .min(page_count as usize);

            for slot in first.saturating_sub(1)..last.min(page_count as usize) {
                let slot = slot as u32;
                let source = self.order.source_at(slot);
                let rotating = self
                    .animation
                    .and_then(|animation| animation.rotation(slot))
                    .filter(|page| page.snapshot.thumbnail.is_some());
                if dragging
                    && state
                        .pressed
                        .as_ref()
                        .is_some_and(|pressed| pressed.source == source)
                {
                    continue;
                }

                let dy = state
                    .slides
                    .get(&source)
                    .map(|slide| slide.offset().1)
                    .unwrap_or(0.0);
                let y = layout.slot_top(slot) - self.state.offset + dy;
                let row_height = layout.row_height(slot);
                let selected = self
                    .selected
                    .is_some_and(|selected| selected.contains(&source));
                let active = current == source;

                if active || selected {
                    let row = Rectangle {
                        x: 4.0,
                        y,
                        width: bounds.width - SCROLLBAR_WIDTH - 12.0,
                        height: row_height - 8.0,
                    };
                    let path = canvas::Path::rounded_rectangle(
                        row.position(),
                        row.size(),
                        border::radius(theme::SIDEBAR_ROW_RADIUS),
                    );
                    frame.fill(&path, self.palette.accent_soft);
                } else if hovered == Some(source) {
                    frame.fill_rectangle(
                        Point::new(4.0, y),
                        Size::new(bounds.width - SCROLLBAR_WIDTH - 12.0, row_height - 8.0),
                        self.palette.hover,
                    );
                }

                let max_width = (bounds.width - 32.0).clamp(80.0, PAGE_MAX_WIDTH);
                let document_slot = self.document_order.slot_of(source).unwrap_or(slot);
                let size = doc.pages[document_slot as usize];
                let (width, height) = fit_page(size, max_width, PAGE_MAX_HEIGHT);
                let picture = Rectangle {
                    x: (bounds.width - SCROLLBAR_WIDTH - width) / 2.0,
                    y: y + PAGE_PADDING_Y,
                    width,
                    height,
                };
                if rotating.is_none() {
                    if let Some(handle) = self
                        .document_order
                        .slot_of(source)
                        .and_then(|slot| self.viewer.thumbnail(slot))
                    {
                        frame.draw_image(picture, image::Image::new(handle.clone()));
                    } else {
                        frame.fill_rectangle(picture.position(), picture.size(), theme::PAPER);
                    }
                }

                frame.fill_text(canvas::Text {
                    content: (slot + 1).to_string(),
                    position: Point::new(
                        (bounds.width - SCROLLBAR_WIDTH) / 2.0,
                        y + PAGE_PADDING_Y + height + PAGE_NUMBER_GAP,
                    ),
                    max_width: bounds.width - SCROLLBAR_WIDTH,
                    color: if active || selected {
                        self.palette.text
                    } else {
                        self.palette.muted
                    },
                    size: 11.0.into(),
                    line_height: LineHeight::default(),
                    font: font(),
                    align_x: Alignment::Center,
                    align_y: iced::alignment::Vertical::Top,
                    shaping: Shaping::Auto,
                });
            }

            if let Some(slot) = self.external_slot {
                let y = layout.slot_top(slot) - EXTERNAL_GAP - self.state.offset;
                file_drop::marker(
                    &mut frame,
                    Rectangle {
                        x: 4.0,
                        y,
                        width: bounds.width - SCROLLBAR_WIDTH - 12.0,
                        height: EXTERNAL_GAP - 6.0,
                    },
                    self.external_label,
                    self.palette,
                );
            }
            if let Some(animation) = self.animation {
                let max_width = (bounds.width - 32.0).clamp(80.0, PAGE_MAX_WIDTH);
                for page in &animation.pages {
                    if page.snapshot.thumbnail.is_none() {
                        continue;
                    }
                    let Some(old_top) = animation.sidebar_tops.get(page.from_slot as usize) else {
                        continue;
                    };
                    let old_y = old_top - animation.sidebar_offset;
                    let (old_width, old_height) =
                        fit_page(page.snapshot.size, max_width, PAGE_MAX_HEIGHT);
                    let old = Rectangle {
                        x: (bounds.width - SCROLLBAR_WIDTH - old_width) / 2.0,
                        y: old_y + PAGE_PADDING_Y,
                        width: old_width,
                        height: old_height,
                    };
                    let target = match animation.kind {
                        EditKind::Delete => old,
                        EditKind::Rotate => {
                            let Some(slot) = page.to_slot else {
                                continue;
                            };
                            let Some(size) = doc.pages.get(slot as usize) else {
                                continue;
                            };
                            let y = layout.slot_top(slot) - self.state.offset;
                            let (width, height) = fit_page(*size, max_width, PAGE_MAX_HEIGHT);
                            Rectangle {
                                x: (bounds.width - SCROLLBAR_WIDTH - width) / 2.0,
                                y: y + PAGE_PADDING_Y,
                                width,
                                height,
                            }
                        }
                    };
                    animation.draw_thumbnail(&mut frame, page, target);
                }
            }

            if dragging && let Some(target) = state.drag_target {
                frame.fill_rectangle(
                    Point::new(6.0, layout.slot_top(target) - self.state.offset),
                    Size::new(3.0, (layout.row_height(target) - 10.0).max(1.0)),
                    self.palette.accent,
                );
            }

            if dragging
                && let (Some(pressed), Some(position)) = (state.pressed.as_ref(), cursor.position())
            {
                let point = local(position, bounds);
                let document_slot = self
                    .document_order
                    .slot_of(pressed.source)
                    .unwrap_or(pressed.slot);
                let size = doc.pages[document_slot as usize];
                let (width, height) = fit_page(size, PAGE_MAX_WIDTH, PAGE_MAX_HEIGHT);
                let picture = Rectangle {
                    x: point.x - width / 2.0,
                    y: point.y - height / 2.0,
                    width,
                    height,
                };
                if let Some(handle) = self
                    .document_order
                    .slot_of(pressed.source)
                    .and_then(|slot| self.viewer.thumbnail(slot))
                {
                    frame.draw_image(picture, image::Image::new(handle.clone()));
                } else {
                    frame.fill_rectangle(picture.position(), picture.size(), theme::PAPER);
                }
            }

            if let Some(scrollbar) = scrollbar_rect(bounds.size(), self.state.offset, &self.layout)
            {
                let path = canvas::Path::rounded_rectangle(
                    scrollbar.position(),
                    scrollbar.size(),
                    border::radius(theme::SCROLLBAR_RADIUS),
                );
                frame.fill(&path, self.palette.scrollbar);
            }
        }

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.position_over(bounds).is_none() {
            return mouse::Interaction::None;
        }

        if cursor
            .position_over(bounds)
            .and_then(|position| {
                hit_slot(
                    local(position, bounds),
                    bounds.size(),
                    self.state.offset,
                    &self.layout,
                )
            })
            .is_some()
        {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::Idle
        }
    }
}

fn fit_page(size: crate::engine::PageSize, max_width: f32, max_height: f32) -> (f32, f32) {
    let scale = (max_width / size.width).min(max_height / size.height);
    (
        (size.width * scale).round().max(1.0),
        (size.height * scale).round().max(1.0),
    )
}

fn wheel_scroll(delta: &mouse::ScrollDelta) -> f32 {
    let lines = match delta {
        mouse::ScrollDelta::Lines { y, .. } => *y,
        mouse::ScrollDelta::Pixels { y, .. } => *y / 56.0,
    };

    -lines * 56.0
}

fn local(point: Point, bounds: Rectangle) -> Point {
    Point::new(point.x - bounds.x, point.y - bounds.y)
}

fn hit_slot(point: Point, size: Size, offset: f32, layout: &PageLayout) -> Option<u32> {
    if point.x < 0.0
        || point.x > size.width - SCROLLBAR_WIDTH
        || point.y < 0.0
        || point.y > size.height
        || layout.tops.is_empty()
    {
        return None;
    }

    Some(layout.slot_at(point.y + offset))
}

fn drop_slot(y: f32, layout: &PageLayout) -> u32 {
    layout.slot_at(y)
}

fn scrollbar_rect(size: Size, offset: f32, layout: &PageLayout) -> Option<Rectangle> {
    let content = layout.height;

    if content <= size.height || size.height <= 0.0 {
        return None;
    }

    let height = size.height - SCROLLBAR_INSET * 2.0;
    let thumb = (height * size.height / content).max(36.0).min(height);
    let travel = height - thumb;
    let y = SCROLLBAR_INSET + travel * (offset / (content - size.height)).clamp(0.0, 1.0);

    Some(Rectangle {
        x: size.width - SCROLLBAR_WIDTH - 4.0,
        y,
        width: SCROLLBAR_WIDTH,
        height: thumb,
    })
}

fn scroll_offset(point: Point, height: f32, layout: &PageLayout, drag: &ScrollDrag) -> f32 {
    let content = layout.height;
    let travel = (height - SCROLLBAR_INSET * 2.0 - 36.0).max(1.0);
    (drag.offset + (point.y - drag.position.y) * (content - height) / travel)
        .clamp(0.0, (content - height).max(0.0))
}

#[cfg(test)]
mod tests {
    use iced::widget::canvas::Program;

    use super::*;

    #[test]
    fn sidebar_wheel_animates_and_direct_scroll_cancels_it() {
        let mut order = PageOrder::default();
        order.reset(20);
        let mut state = State {
            pages_size: Size::new(220.0, 400.0),
            page_sizes: vec![
                PageSize {
                    width: 612.0,
                    height: 792.0
                };
                20
            ],
            ..State::default()
        };
        state.wheel_by(120.0, &order, &order);
        assert_eq!(state.offset, 0.0);
        state.advance_scroll(Instant::now() + Duration::from_millis(64));
        assert!(state.offset > 0.0 && state.offset < 120.0);
        state.scroll_by(40.0, &order, &order);
        assert!(!state.scrolling());
    }

    #[test]
    fn page_rows_follow_each_page_aspect_ratio() {
        let pages = [
            PageSize {
                width: 612.0,
                height: 792.0,
            },
            PageSize {
                width: 792.0,
                height: 612.0,
            },
        ];
        let mut order = PageOrder::default();
        order.reset(2);
        let layout = PageLayout::new(&pages, 280.0, &order, &order);

        assert_eq!(layout.row_height(0), 182.0);
        assert_eq!(layout.row_height(1), 129.0);
        assert_eq!(layout.slot_top(1), 182.0);
        assert_eq!(layout.slot_at(181.0), 0);
        assert_eq!(layout.slot_at(182.0), 1);
    }

    #[test]
    fn preview_rows_keep_each_sources_document_geometry() {
        let portrait = PageSize {
            width: 612.0,
            height: 792.0,
        };
        let landscape = PageSize {
            width: 792.0,
            height: 612.0,
        };
        let pages = [portrait, landscape, portrait];
        let mut document_order = PageOrder::default();
        document_order.reset(3);
        document_order.move_source(0, 1);
        let mut preview = document_order.clone();
        preview.move_source(0, 0);

        let layout = PageLayout::new(&pages, 280.0, &preview, &document_order);

        assert_eq!(layout.row_height(0), 129.0);
        assert_eq!(layout.row_height(1), 182.0);
    }

    #[test]
    fn page_rows_slide_when_rotation_changes_their_offsets() {
        let portrait = PageSize {
            width: 612.0,
            height: 792.0,
        };
        let landscape = PageSize {
            width: 792.0,
            height: 612.0,
        };
        let mut order = PageOrder::default();
        order.reset(2);
        let old_layout = PageLayout::new(&[portrait, portrait], 280.0, &order, &order);
        let new_layout = PageLayout::new(&[landscape, portrait], 280.0, &order, &order);
        let mut state = CanvasState::default();
        let now = Instant::now();

        state.advance_slides(now, &old_layout, &order, None);
        state.advance_slides(now + Duration::from_millis(16), &new_layout, &order, None);

        assert_eq!(
            state.slides.get(&0).map_or(0.0, |slide| slide.offset().1),
            0.0
        );
        assert!(state.slides[&1].offset().1 > 0.0);
    }

    #[test]
    fn canvas_reports_motion_without_scheduling_its_own_frames() {
        let doc = std::sync::Arc::new(DocInfo {
            id: 1,
            path: std::path::PathBuf::new(),
            title: String::new(),
            pages: vec![
                PageSize {
                    width: 612.0,
                    height: 792.0,
                };
                3
            ],
            outline: Vec::new(),
        });
        let mut viewer = Viewer::default();
        viewer.set_document(doc);
        let sidebar = State::default();
        let mut order = PageOrder::default();
        order.reset(3);
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(280.0, 600.0));
        let mut state = CanvasState::default();
        let now = Instant::now();
        let redraw = |state: &mut CanvasState, order: &PageOrder, now: Instant| {
            let layout = PageLayout::new(
                &viewer.document().expect("document").pages,
                bounds.width,
                order,
                order,
            );
            let canvas = Canvas {
                viewer: &viewer,
                state: &sidebar,
                order,
                document_order: order,
                layout,
                view_mode: ViewMode::Reader,
                current: 0,
                selected: None,
                palette: Palette::DARK,
                animation: None,
                external_slot: None,
                external_label: "",
                preserve_reorder: false,
            };
            let action = canvas
                .update(
                    state,
                    &iced::Event::Window(iced::window::Event::RedrawRequested(now)),
                    bounds,
                    mouse::Cursor::Unavailable,
                )
                .expect("canvas frame report");
            let (message, redraw, _) = action.into_inner();
            assert!(matches!(redraw, iced::window::RedrawRequest::Wait));
            message
        };

        let _ = redraw(&mut state, &order, now);
        order.move_slot(0, 2);
        assert!(matches!(
            redraw(&mut state, &order, now + Duration::from_millis(16)),
            Some(Message::SidebarPagesFrame {
                animating: true,
                ..
            })
        ));
        state.slides.clear();
        assert!(matches!(
            redraw(&mut state, &order, now + Duration::from_millis(32)),
            Some(Message::SidebarPagesFrame {
                animating: false,
                ..
            })
        ));
    }

    #[test]
    fn document_replacement_discards_finished_reorder_slides() {
        let pages = vec![
            PageSize {
                width: 612.0,
                height: 792.0,
            };
            3
        ];
        let mut order = PageOrder::default();
        order.reset(3);
        let old_layout = PageLayout::new(&pages, 280.0, &order, &order);
        let mut state = CanvasState::default();
        let now = Instant::now();
        state.advance_slides(now, &old_layout, &order, None);

        order.move_slot(0, 2);
        let preview_layout = PageLayout::new(&pages, 280.0, &order, &order);
        state.advance_slides(
            now + Duration::from_millis(16),
            &preview_layout,
            &order,
            None,
        );
        assert!(!state.slides.is_empty());

        let doc = std::sync::Arc::new(DocInfo {
            id: 2,
            path: std::path::PathBuf::new(),
            title: String::new(),
            pages,
            outline: Vec::new(),
        });
        let mut viewer = Viewer::default();
        viewer.set_document(doc);
        let sidebar = State::default();
        let canvas = Canvas {
            viewer: &viewer,
            state: &sidebar,
            order: &order,
            document_order: &order,
            layout: preview_layout,
            view_mode: ViewMode::Reader,
            current: 0,
            selected: None,
            palette: Palette::DARK,
            animation: None,
            external_slot: None,
            external_label: "",
            preserve_reorder: false,
        };
        state.previous_doc = Some(1);

        let _ = canvas.update(
            &mut state,
            &iced::Event::Window(iced::window::Event::RedrawRequested(
                now + Duration::from_millis(32),
            )),
            Rectangle::new(Point::ORIGIN, Size::new(280.0, 600.0)),
            mouse::Cursor::Unavailable,
        );

        assert!(state.slides.is_empty());
    }

    #[test]
    fn visible_page_slots_use_variable_row_offsets() {
        let mut pages = vec![
            PageSize {
                width: 612.0,
                height: 792.0,
            };
            10
        ];
        pages[1] = PageSize {
            width: 792.0,
            height: 612.0,
        };
        let mut order = PageOrder::default();
        order.reset(10);
        let state = State {
            pages_size: Size::new(280.0, 200.0),
            page_sizes: pages,
            offset: 400.0,
            ..State::default()
        };

        let visible = state.visible_page_slots(&order, &order);

        assert!(visible.start <= 2);
        assert!(visible.contains(&2));
        assert!(visible.end > 2);
    }

    #[test]
    fn pages_wheel_uses_the_natural_scroll_direction() {
        assert_eq!(
            wheel_scroll(&mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 }),
            -56.0
        );
        assert_eq!(
            wheel_scroll(&mouse::ScrollDelta::Lines { x: 0.0, y: -3.0 }),
            168.0
        );
        assert_eq!(
            wheel_scroll(&mouse::ScrollDelta::Pixels { x: 0.0, y: 112.0 }),
            -112.0
        );
    }

    #[test]
    fn pages_wheel_only_captures_events_inside_its_bounds() {
        let viewer = Viewer::default();
        let state = State::default();
        let order = PageOrder::default();
        let canvas = Canvas {
            viewer: &viewer,
            state: &state,
            order: &order,
            document_order: &order,
            layout: PageLayout::default(),
            view_mode: ViewMode::Reader,
            current: 0,
            selected: None,
            palette: Palette::DARK,
            animation: None,
            external_slot: None,
            external_label: "",
            preserve_reorder: false,
        };
        let event = iced::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: 1.0 },
        });
        let bounds = Rectangle::new(Point::new(20.0, 20.0), Size::new(280.0, 600.0));
        let mut canvas_state = CanvasState::default();

        for cursor in [
            mouse::Cursor::Unavailable,
            mouse::Cursor::Available(Point::new(400.0, 300.0)),
        ] {
            assert!(
                canvas
                    .update(&mut canvas_state, &event, bounds, cursor)
                    .is_none()
            );
        }

        let action = canvas
            .update(
                &mut canvas_state,
                &event,
                bounds,
                mouse::Cursor::Available(Point::new(100.0, 100.0)),
            )
            .expect("wheel inside the page list is handled");
        let (message, _, _) = action.into_inner();

        assert!(matches!(message, Some(Message::SidebarWheel(-56.0))));
    }

    #[test]
    fn scrolling_maps_screen_rows_to_display_slots() {
        let pages = vec![
            PageSize {
                width: 612.0,
                height: 792.0,
            };
            10
        ];
        let mut order = PageOrder::default();
        order.reset(10);
        let layout = PageLayout::new(&pages, 280.0, &order, &order);

        assert_eq!(
            hit_slot(
                Point::new(20.0, 20.0),
                Size::new(280.0, 600.0),
                layout.row_height(0),
                &layout,
            ),
            Some(1)
        );
    }

    #[test]
    fn reveal_centers_a_reordered_source() {
        let mut order = PageOrder::default();
        order.reset(20);
        order.move_source(10, 2);
        let mut state = State {
            open: true,
            tab: Tab::Pages,
            pages_size: Size::new(166.0, 300.0),
            page_sizes: vec![
                PageSize {
                    width: 612.0,
                    height: 792.0,
                };
                20
            ],
            page_count: 20,
            ..State::default()
        };
        state.reveal_source(10, &order, &order);
        let layout = PageLayout::new(&state.page_sizes, state.pages_size.width, &order, &order);

        assert!(state.offset > 0.0);
        assert!(state.offset < layout.slot_top(2) + 100.0);
    }
}
