//! A virtualized page grid backed by the viewer's real PDF thumbnails.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
#[cfg(test)]
use std::time::Duration;
use std::time::Instant;

use iced::advanced::image;
use iced::advanced::text::{Alignment, LineHeight, Shaping};
use iced::keyboard;
use iced::mouse;
use iced::widget::canvas::{self, Action, Frame, Geometry};
use iced::{Element, Fill, Point, Rectangle, Size, border};

use crate::app::{App, EditAnimation, EditKind, Message, ViewMode};
use crate::motion::Slide;
use crate::ui::order::PageOrder;
use crate::ui::viewer::Viewer;
use crate::ui::{file_drop, font, theme};

const SCROLLBAR_WIDTH: f32 = 8.0;
const SCROLLBAR_INSET: f32 = 12.0;
const LABEL_HEIGHT: f32 = 22.0;
const CARD_PADDING: f32 = 12.0;
pub const DEFAULT_TRACK: f32 = 190.0;
pub const MIN_TRACK: f32 = 160.0;
pub const MAX_TRACK: f32 = 360.0;
const OVERSCAN_ROWS: usize = 1;
const DRAG_SLACK: f32 = 6.0;
const AUTO_SCROLL_EDGE: f32 = 28.0;
const AUTO_SCROLL_STEP: f32 = 18.0;

#[derive(Debug)]
pub struct State {
    pub size: Size,
    pub drop_bounds: Rectangle,
    pub offset: f32,
    pub track: f32,
    pub selected: HashSet<u32>,
    pub anchor: Option<u32>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            size: Size::ZERO,
            drop_bounds: Rectangle::default(),
            offset: 0.0,
            track: DEFAULT_TRACK,
            selected: HashSet::new(),
            anchor: None,
        }
    }
}

impl State {
    pub fn set_document(&mut self, page_count: u32) {
        self.offset = 0.0;
        self.selected.clear();
        self.anchor = Some(0);

        if page_count > 0 {
            self.selected.insert(0);
        }
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }

    pub fn select_all(&mut self, page_count: u32, order: &PageOrder) {
        self.selected.clear();
        self.selected.extend(order.as_sources().iter().copied());
        self.anchor = (page_count > 0).then(|| order.source_at(0));
    }

    pub fn select(&mut self, page: u32, modifiers: keyboard::Modifiers, order: &PageOrder) {
        if order.slot_of(page).is_none() {
            return;
        }

        if modifiers.command() {
            if !self.selected.remove(&page) {
                self.selected.insert(page);
            }
            self.anchor = Some(page);
        } else if modifiers.shift() {
            if let Some(anchor) = self.anchor
                && let Some(from) = order.slot_of(anchor)
                && let Some(to) = order.slot_of(page)
            {
                self.selected.clear();
                self.selected.extend(order.sources_in(from, to));
            } else {
                self.selected.clear();
                self.selected.insert(page);
                self.anchor = Some(page);
            }
        } else {
            self.selected.clear();
            self.selected.insert(page);
            self.anchor = Some(page);
        }
    }

    pub fn focused(&self) -> Option<u32> {
        self.anchor
            .filter(|page| self.selected.contains(page))
            .or_else(|| self.selected.iter().min().copied())
    }

    pub fn move_by(&mut self, delta: i32, page_count: u32, order: &PageOrder) {
        if page_count == 0 {
            return;
        }

        let current = self
            .focused()
            .and_then(|page| order.slot_of(page))
            .unwrap_or(0) as i32;
        let slot = (current + delta).clamp(0, page_count as i32 - 1) as u32;
        let page = order.source_at(slot);

        self.selected.clear();
        self.selected.insert(page);
        self.anchor = Some(page);
        self.reveal_source(page, page_count, order);
    }

    pub fn set_size(&mut self, size: Size, page_count: u32, order: &PageOrder) {
        let first = self.size == Size::ZERO;
        self.size = size;
        self.clamp(page_count);

        if first && let Some(page) = self.focused() {
            self.reveal_source(page, page_count, order);
        }
    }

    pub fn scroll_by(&mut self, delta: f32, page_count: u32) {
        self.offset += delta;
        self.clamp(page_count);
    }

    pub fn scroll_to(&mut self, offset: f32, page_count: u32) {
        self.offset = offset;
        self.clamp(page_count);
    }

    pub fn clamp(&mut self, page_count: u32) {
        let content = self.geometry().content_height(page_count);
        self.offset = self
            .offset
            .clamp(0.0, (content - self.size.height).max(0.0));
    }

    pub fn reveal_source(&mut self, source: u32, page_count: u32, order: &PageOrder) {
        if self.size == Size::ZERO {
            return;
        }

        let Some(slot) = order.slot_of(source) else {
            return;
        };
        let geometry = self.geometry();
        let rect = geometry.page_rect(slot);

        if rect.y < 0.0 {
            self.offset += rect.y;
        } else if rect.y + rect.height > self.size.height {
            self.offset += rect.y + rect.height - self.size.height;
        }

        self.clamp(page_count);
    }

    pub fn wanted_pages(
        &self,
        page_count: u32,
        order: &PageOrder,
        document_order: &PageOrder,
    ) -> Vec<u32> {
        self.geometry()
            .visible_range(page_count)
            .filter_map(|slot| document_order.slot_of(order.source_at(slot as u32)))
            .collect()
    }

    pub fn columns(&self) -> usize {
        self.geometry().columns
    }

    pub fn insertion_slot(&self, point: Point, page_count: u32, preview: Option<u32>) -> u32 {
        let geometry = self.geometry();
        if page_count == 0 || point.y + self.offset < geometry.inset {
            return 0;
        }
        let row = ((point.y + self.offset - geometry.inset) / geometry.row_height)
            .floor()
            .max(0.0) as u32;
        let column = ((point.x - geometry.inset + geometry.gap_x / 2.0)
            / (geometry.track + geometry.gap_x))
            .floor()
            .clamp(0.0, geometry.columns.saturating_sub(1) as f32) as u32;
        let slot = row * geometry.columns as u32 + column;
        if preview == Some(slot) {
            return slot.min(page_count);
        }
        let cell = geometry.cell_rect(slot);
        let source_slot = slot.saturating_sub(u32::from(preview.is_some_and(|gap| slot > gap)));
        (source_slot + u32::from(point.x > cell.center_x())).min(page_count)
    }

    fn geometry(&self) -> GridGeometry {
        GridGeometry::new(self.size, self.track, self.offset)
    }
}

struct GridGeometry {
    size: Size,
    offset: f32,
    columns: usize,
    track: f32,
    gap_x: f32,
    gap_y: f32,
    inset: f32,
    thumbnail: f32,
    thumbnail_height: f32,
    row_height: f32,
}

impl GridGeometry {
    fn new(size: Size, minimum_track: f32, offset: f32) -> Self {
        let compact = size.width < 850.0;
        let inset = if compact { 16.0 } else { 32.0 };
        let gap_x = if compact { 14.0 } else { 20.0 };
        let gap_y = if compact { 12.0 } else { 14.0 };
        let available = (size.width - inset * 2.0 - SCROLLBAR_WIDTH).max(1.0);
        let minimum_track = minimum_track.clamp(MIN_TRACK, MAX_TRACK);
        let columns = ((available + gap_x) / (minimum_track + gap_x))
            .floor()
            .max(1.0) as usize;
        let track =
            ((available - gap_x * (columns - 1) as f32) / columns as f32).max(minimum_track);
        let thumbnail = (track - CARD_PADDING * 2.0).clamp(112.0, 320.0);
        let thumbnail_height = thumbnail.clamp(144.0, 240.0);

        Self {
            size,
            offset,
            columns,
            track,
            gap_x,
            gap_y,
            inset,
            thumbnail,
            thumbnail_height,
            row_height: thumbnail_height + LABEL_HEIGHT + gap_y,
        }
    }

    fn rows(&self, page_count: u32) -> usize {
        (page_count as usize).div_ceil(self.columns)
    }

    fn content_height(&self, page_count: u32) -> f32 {
        self.inset * 2.0
            + self.rows(page_count).saturating_sub(1) as f32 * self.row_height
            + self.thumbnail_height
            + LABEL_HEIGHT
    }

    fn cell_rect(&self, slot: u32) -> Rectangle {
        let column = slot as usize % self.columns;
        let row = slot as usize / self.columns;
        let x = self.inset + column as f32 * (self.track + self.gap_x);
        let y = self.inset + row as f32 * self.row_height;

        Rectangle {
            x,
            y,
            width: self.track,
            height: self.row_height - self.gap_y,
        }
    }

    fn page_rect(&self, slot: u32) -> Rectangle {
        let mut rect = self.cell_rect(slot);
        rect.y -= self.offset;
        rect
    }

    fn thumbnail_rect(&self, slot: u32) -> Rectangle {
        let cell = self.cell_rect(slot);
        Rectangle {
            x: cell.x + (cell.width - self.thumbnail) / 2.0,
            y: cell.y,
            width: self.thumbnail,
            height: self.thumbnail_height,
        }
    }

    fn visible_range(&self, page_count: u32) -> Range<usize> {
        if page_count == 0 || self.size.height <= 0.0 {
            return 0..0;
        }

        let first_row =
            ((self.offset / self.row_height).floor() as usize).saturating_sub(OVERSCAN_ROWS);
        let last_row = (((self.offset + self.size.height) / self.row_height).ceil() as usize
            + OVERSCAN_ROWS)
            .min(self.rows(page_count));
        let start = (first_row * self.columns).min(page_count as usize);
        let end = (last_row * self.columns).min(page_count as usize);

        start..end
    }

    fn hit_test(&self, point: Point, page_count: u32) -> Option<u32> {
        if point.x < self.inset
            || point.x > self.size.width - self.inset - SCROLLBAR_WIDTH
            || point.y < 0.0
            || point.y > self.size.height
        {
            return None;
        }

        let column = ((point.x - self.inset) / (self.track + self.gap_x)) as usize;
        let row = ((point.y + self.offset - self.inset) / self.row_height) as usize;

        if row >= self.rows(page_count) || column >= self.columns {
            return None;
        }

        let slot = row * self.columns + column;
        let cell = self.cell_rect(slot as u32);
        let y = cell.y - self.offset;

        (slot < page_count as usize && point.x <= cell.x + cell.width && point.y <= y + cell.height)
            .then_some(slot as u32)
    }

    fn drop_slot(&self, point: Point, page_count: u32) -> u32 {
        if page_count == 0 {
            return 0;
        }

        let column = ((point.x - self.inset) / (self.track + self.gap_x))
            .floor()
            .clamp(0.0, self.columns.saturating_sub(1) as f32) as usize;
        let row = ((point.y + self.offset - self.inset) / self.row_height)
            .floor()
            .max(0.0) as usize;
        ((row * self.columns + column) as u32).min(page_count - 1)
    }

    fn scrollbar_rect(&self, page_count: u32) -> Option<Rectangle> {
        let content = self.content_height(page_count);

        if content <= self.size.height || self.size.height <= 0.0 {
            return None;
        }

        let height = self.size.height - SCROLLBAR_INSET * 2.0;
        let thumb = (height * self.size.height / content).max(36.0).min(height);
        let travel = height - thumb;
        let y =
            SCROLLBAR_INSET + travel * (self.offset / (content - self.size.height)).clamp(0.0, 1.0);

        Some(Rectangle {
            x: self.size.width - SCROLLBAR_WIDTH - 4.0,
            y,
            width: SCROLLBAR_WIDTH,
            height: thumb,
        })
    }
}

pub fn view(app: &App) -> Element<'_, Message> {
    canvas::Canvas::new(Canvas {
        viewer: &app.viewer,
        state: &app.organizer,
        order: &app.order,
        document_order: &app.viewer_order,
        view_mode: app.view_mode,
        palette: app.palette,
        animation: app.edit_animation.as_ref(),
        external_slot: app.external_drop.slot(file_drop::Zone::Organizer),
        external_label: app.external_drop.label(),
        preserve_reorder: app
            .viewer
            .document()
            .is_some_and(|doc| app.external_drop.settling_doc == Some(doc.id)),
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
    view_mode: ViewMode,
    palette: theme::Palette,
    animation: Option<&'a EditAnimation>,
    external_slot: Option<u32>,
    external_label: &'a str,
    preserve_reorder: bool,
}

#[derive(Default)]
struct CanvasState {
    size: Size,
    bounds: Rectangle,
    previous_external: Option<u32>,
    modifiers: keyboard::Modifiers,
    pressed: Option<Pressed>,
    previous: Option<iced::advanced::mouse::click::Click>,
    scroll_drag: Option<ScrollDrag>,
    drag_target: Option<u32>,
    previous_order: Vec<u32>,
    previous_doc: Option<u64>,
    previous_view: Option<ViewMode>,
    previous_size: Size,
    previous_track: f32,
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
    fn reset_slides(&mut self) {
        self.previous_order.clear();
        self.previous_external = None;
        self.slides.clear();
        self.last_frame = None;
    }

    fn advance_slides(
        &mut self,
        now: Instant,
        previous: &GridGeometry,
        current: &GridGeometry,
        order: &PageOrder,
        animation: Option<&EditAnimation>,
        external_slot: Option<u32>,
    ) -> bool {
        if !self.previous_order.is_empty()
            && (self.previous_order != order.as_sources()
                || self.previous_size != previous.size
                || self.previous_track != previous.track
                || self.previous_external != external_slot)
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
                let old_slot = old_slot
                    + u32::from(
                        self.previous_external
                            .is_some_and(|target| old_slot >= target),
                    );
                let new_slot = slot as u32
                    + u32::from(external_slot.is_some_and(|target| slot as u32 >= target));
                let old = previous.cell_rect(old_slot).position();
                let new = current.cell_rect(new_slot).position();
                self.slides
                    .entry(source)
                    .or_default()
                    .shift(old.x - new.x, old.y - new.y);
            }
        }

        self.previous_external = external_slot;
        self.previous_order = order.as_sources().to_vec();
        self.previous_size = current.size;
        self.previous_track = current.track;

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
                        state.reset_slides();
                    }
                }
                if state.previous_view != Some(self.view_mode) {
                    state.previous_view = Some(self.view_mode);
                    state.animating = false;
                }
                let previous = GridGeometry::new(state.previous_size, state.previous_track, 0.0);
                let current = GridGeometry::new(bounds.size(), self.state.track, 0.0);
                let moving = state.advance_slides(
                    *now,
                    &previous,
                    &current,
                    self.order,
                    self.animation,
                    self.external_slot,
                );

                if resized || moving != state.animating {
                    state.animating = moving;
                    return Some(Action::publish(Message::OrganizerFrame {
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

                if let Some(scrollbar) = self
                    .state
                    .geometry()
                    .scrollbar_rect(self.viewer.page_count())
                    && scrollbar.contains(point)
                {
                    state.scroll_drag = Some(ScrollDrag {
                        position,
                        offset: self.state.offset,
                    });
                    return Some(Action::capture());
                }

                if let Some(slot) = self
                    .state
                    .geometry()
                    .hit_test(point, self.viewer.page_count())
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
                    let geometry = self.state.geometry();
                    let content = geometry.content_height(self.viewer.page_count());
                    let travel = (geometry.size.height - SCROLLBAR_INSET * 2.0 - 36.0).max(1.0);
                    let offset = drag.offset
                        + (position.y - drag.position.y) * (content - bounds.height) / travel;

                    return Some(
                        Action::publish(Message::OrganizerScrolledTo(offset)).and_capture(),
                    );
                }

                if let Some(pressed) = state.pressed.as_ref()
                    && pressed.button == mouse::Button::Left
                    && pressed.position.distance(*position) > DRAG_SLACK
                {
                    let point = local(*position, bounds);
                    let slot = self
                        .state
                        .geometry()
                        .drop_slot(point, self.viewer.page_count());
                    let scroll = if point.y < AUTO_SCROLL_EDGE {
                        -AUTO_SCROLL_STEP
                    } else if point.y > bounds.height - AUTO_SCROLL_EDGE {
                        AUTO_SCROLL_STEP
                    } else {
                        0.0
                    };
                    state.drag_target = Some(slot);

                    return Some(
                        Action::publish(Message::OrganizerDrag {
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
                    let geometry = self.state.geometry();
                    let content = geometry.content_height(self.viewer.page_count());
                    let travel = (geometry.size.height - SCROLLBAR_INSET * 2.0 - 36.0).max(1.0);

                    return Some(Action::publish(Message::OrganizerScrolledTo(
                        drag.offset
                            + (position.y - drag.position.y) * (content - bounds.height) / travel,
                    )));
                }

                let pressed = state.pressed.take()?;

                if pressed.position.distance(position) > DRAG_SLACK {
                    let slot = state.drag_target.take().unwrap_or(pressed.slot);
                    return Some(Action::publish(Message::OrganizerDrag {
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

                    return Some(Action::publish(Message::OrganizerClicked {
                        slot: pressed.slot,
                        modifiers: state.modifiers,
                        double_click: click.kind() == iced::advanced::mouse::click::Kind::Double,
                    }));
                }
            }
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => *y / 56.0,
                };

                return Some(Action::publish(Message::OrganizerScrolled(-lines * 56.0)));
            }
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(modifiers)) => {
                state.modifiers = *modifiers;
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
        let geometry = GridGeometry::new(bounds.size(), self.state.track, self.state.offset);
        let page_count = self.viewer.page_count();
        let dragging = state.pressed.as_ref().is_some_and(|pressed| {
            cursor
                .position()
                .is_some_and(|position| pressed.position.distance(position) > DRAG_SLACK)
        });

        frame.fill_rectangle(Point::ORIGIN, bounds.size(), self.palette.canvas);

        if let Some(doc) = self.viewer.document() {
            for visible_slot in
                geometry.visible_range(page_count + u32::from(self.external_slot.is_some()))
            {
                let visible_slot = visible_slot as u32;
                if self.external_slot == Some(visible_slot) {
                    continue;
                }
                let slot = visible_slot
                    - u32::from(
                        self.external_slot
                            .is_some_and(|target| visible_slot > target),
                    );
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

                let (dx, dy) = state
                    .slides
                    .get(&source)
                    .map(Slide::offset)
                    .unwrap_or((0.0, 0.0));
                let mut cell = geometry.cell_rect(visible_slot);
                cell.y -= self.state.offset;
                cell.x += dx;
                cell.y += dy;
                let selected = self.state.selected.contains(&source);

                if selected {
                    let path = canvas::Path::rounded_rectangle(
                        cell.position(),
                        cell.size(),
                        border::radius(theme::CARD_RADIUS),
                    );
                    frame.fill(&path, self.palette.hover);
                }

                let max_picture = geometry.thumbnail_rect(visible_slot);
                let document_slot = self.document_order.slot_of(source).unwrap_or(slot);
                let picture = fit_page(
                    doc.pages[document_slot as usize],
                    max_picture.width,
                    max_picture.height,
                );
                let picture = Rectangle {
                    x: max_picture.x + (max_picture.width - picture.0) / 2.0 + dx,
                    y: max_picture.y - self.state.offset
                        + (max_picture.height - picture.1) / 2.0
                        + dy,
                    width: picture.0,
                    height: picture.1,
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
                        max_picture.center_x() + dx,
                        max_picture.y + max_picture.height + 7.0 - self.state.offset + dy,
                    ),
                    max_width: cell.width,
                    color: if selected {
                        self.palette.text
                    } else {
                        self.palette.muted
                    },
                    size: 12.0.into(),
                    line_height: LineHeight::default(),
                    font: font(),
                    align_x: Alignment::Center,
                    align_y: iced::alignment::Vertical::Top,
                    shaping: Shaping::Auto,
                });
            }

            if let Some(animation) = self.animation {
                for page in &animation.pages {
                    if page.snapshot.thumbnail.is_none() {
                        continue;
                    }
                    let mut old_max = geometry.thumbnail_rect(page.from_slot);
                    old_max.y -= self.state.offset;
                    let (old_width, old_height) =
                        fit_page(page.snapshot.size, old_max.width, old_max.height);
                    let old = Rectangle {
                        x: old_max.x + (old_max.width - old_width) / 2.0,
                        y: old_max.y + (old_max.height - old_height) / 2.0,
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
                            let mut max = geometry.thumbnail_rect(slot);
                            max.y -= self.state.offset;
                            let (width, height) = fit_page(*size, max.width, max.height);
                            Rectangle {
                                x: max.x + (max.width - width) / 2.0,
                                y: max.y + (max.height - height) / 2.0,
                                width,
                                height,
                            }
                        }
                    };
                    animation.draw_thumbnail(&mut frame, page, target);
                }
            }

            if let Some(target) = self.external_slot {
                let mut rect = geometry.page_rect(target);
                rect.height = geometry.thumbnail_height;
                file_drop::marker(&mut frame, rect, self.external_label, self.palette);
            }
            if dragging && let Some(target) = state.drag_target {
                let mut cell = geometry.cell_rect(target);
                cell.y -= self.state.offset;
                frame.fill_rectangle(
                    Point::new(cell.x - 6.0, cell.y),
                    Size::new(3.0, cell.height),
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
                let (width, height) = fit_page(size, geometry.thumbnail, geometry.thumbnail_height);
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

            if let Some(scrollbar) = geometry.scrollbar_rect(page_count) {
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
                self.state
                    .geometry()
                    .hit_test(local(position, bounds), self.viewer.page_count())
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

fn local(point: Point, bounds: Rectangle) -> Point {
    Point::new(point.x - bounds.x, point.y - bounds.y)
}

#[cfg(test)]
mod tests {
    use iced::widget::canvas::Program;

    use super::*;
    use crate::engine::PageSize;

    #[test]
    fn reference_width_has_six_compact_columns() {
        let geometry = GridGeometry::new(Size::new(1360.0, 870.0), DEFAULT_TRACK, 0.0);
        assert_eq!(geometry.columns, 6);
        assert_eq!(geometry.gap_x, 20.0);
        assert_eq!(geometry.gap_y, 14.0);
        assert!(geometry.track < 200.0);
        assert!(geometry.thumbnail > 170.0);
    }

    #[test]
    fn compact_width_has_two_columns() {
        let geometry = GridGeometry::new(Size::new(600.0, 800.0), DEFAULT_TRACK, 0.0);
        assert_eq!(geometry.columns, 2);
        assert_eq!(geometry.gap_x, 14.0);
        assert_eq!(geometry.gap_y, 12.0);
    }

    #[test]
    fn landscape_pages_keep_their_real_shape() {
        let (width, height) = fit_page(
            PageSize {
                width: 960.0,
                height: 540.0,
            },
            156.0,
            190.0,
        );
        assert_eq!(width, 156.0);
        assert_eq!(height, 88.0);
        assert!((width / height - 16.0 / 9.0).abs() < 0.01);
    }

    #[test]
    fn settled_reorder_does_not_start_a_second_slide() {
        let size = Size::new(1360.0, 870.0);
        let mut order = PageOrder::default();
        order.reset(3);
        let mut state = CanvasState::default();
        let now = Instant::now();
        let geometry = GridGeometry::new(size, DEFAULT_TRACK, 0.0);

        state.advance_slides(now, &geometry, &geometry, &order, None, None);
        order.move_slot(0, 2);
        state.advance_slides(
            now + Duration::from_millis(16),
            &geometry,
            &geometry,
            &order,
            None,
            None,
        );
        assert!(!state.slides.is_empty());

        state.reset_slides();
        order.reset(3);
        state.advance_slides(
            now + Duration::from_millis(32),
            &geometry,
            &geometry,
            &order,
            None,
            None,
        );
        assert!(state.slides.is_empty());
        assert_eq!(state.previous_order, [0, 1, 2]);
    }

    #[test]
    fn keyboard_navigation_uses_display_slots() {
        let mut order = PageOrder::default();
        order.reset(3);
        order.move_source(2, 0);
        let mut state = State::default();
        state.set_document(3);
        state.select(2, keyboard::Modifiers::default(), &order);
        state.move_by(1, 3, &order);

        assert_eq!(state.focused(), Some(0));
    }

    #[test]
    fn reveal_before_layout_does_not_change_the_scroll_offset() {
        let mut order = PageOrder::default();
        order.reset(20);
        let mut state = State::default();
        state.set_document(20);

        state.reveal_source(0, 20, &order);

        assert_eq!(state.offset, 0.0);
    }

    #[test]
    fn canvas_reports_motion_to_the_app_frame_clock() {
        let doc = std::sync::Arc::new(crate::engine::DocInfo {
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
        let organizer = State::default();
        let mut order = PageOrder::default();
        order.reset(3);
        let bounds = Rectangle::new(Point::ORIGIN, Size::new(1360.0, 870.0));
        let mut state = CanvasState::default();
        let now = Instant::now();
        let redraw = |state: &mut CanvasState, order: &PageOrder, now: Instant| {
            let canvas = Canvas {
                viewer: &viewer,
                state: &organizer,
                order,
                document_order: order,
                view_mode: ViewMode::Organizer,
                palette: theme::Palette::DARK,
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
            Some(Message::OrganizerFrame {
                animating: true,
                ..
            })
        ));
        state.slides.clear();
        assert!(matches!(
            redraw(&mut state, &order, now + Duration::from_millis(32)),
            Some(Message::OrganizerFrame {
                animating: false,
                ..
            })
        ));
    }

    #[test]
    fn selection_tracks_sources_after_deletion() {
        let mut order = PageOrder::default();
        order.reset(4);
        order.retain(|source| source != 0);
        order.move_source(3, 0);
        let mut state = State::default();
        state.set_document(3);

        state.select_all(3, &order);
        assert_eq!(state.selected, HashSet::from([1, 2, 3]));
        state.select(3, keyboard::Modifiers::default(), &order);
        assert_eq!(state.focused(), Some(3));
    }
}
#[test]
fn external_insertion_hits_both_sides_and_the_document_end() {
    let state = State {
        size: Size::new(1360.0, 700.0),
        ..State::default()
    };
    let geometry = state.geometry();
    let first = geometry.page_rect(0);
    assert_eq!(
        state.insertion_slot(Point::new(first.x + 1.0, first.center_y()), 7, None),
        0
    );
    assert_eq!(
        state.insertion_slot(
            Point::new(first.x + first.width - 1.0, first.center_y()),
            7,
            None
        ),
        1
    );
    let last = geometry.page_rect(6);
    assert_eq!(
        state.insertion_slot(
            Point::new(last.x + last.width - 1.0, last.center_y()),
            7,
            None
        ),
        7
    );
    assert_eq!(state.insertion_slot(Point::new(30.0, 690.0), 7, None), 7);
    let gap = geometry.page_rect(1);
    assert_eq!(state.insertion_slot(gap.center(), 7, Some(1)), 1);
    let shifted = geometry.page_rect(2);
    assert_eq!(
        state.insertion_slot(Point::new(shifted.x + 1.0, shifted.center_y()), 7, Some(1)),
        1
    );
}
