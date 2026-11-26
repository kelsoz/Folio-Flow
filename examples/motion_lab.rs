//! Native Iced motion laboratory. This example deliberately uses synthetic pages and does not
//! import the PDF engine, so animation and layout experiments cannot regress document handling.
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::ops::Range;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use iced::advanced::image;
use iced::mouse;
use iced::widget::canvas::{self, Canvas};
use iced::widget::container;
use iced::{
    Color, Element, Event, Length, Point, Rectangle, Size, Task, Theme, Vector, border, window,
};

#[allow(dead_code)]
#[path = "../src/motion.rs"]
mod motion;

use ::image as image_crate;
use motion::{FrameBudget, Spring, Tween};

const BREAKPOINT: f32 = 1280.0;
const TOP: f32 = 76.0;
const SIDEBAR: f32 = 264.0;
const GAP: f32 = 14.0;
const OVERSCAN: f32 = 320.0;
const DRAG_THRESHOLD: f32 = 7.0;
const FLIP_DURATION: Duration = Duration::from_millis(320);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    Reader,
    Organizer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Control {
    Auto,
    ToggleSidebar,
    ToggleView,
    Undo,
    Redo,
    Delete,
    ZoomIn,
    ZoomOut,
    Minimize,
    Maximize,
    Close,
}

#[derive(Debug, Clone)]
struct Page {
    id: u32,
    handle: image::Handle,
}

#[derive(Debug, Clone)]
struct HistoryEntry {
    before: Vec<u32>,
    after: Vec<u32>,
}

#[derive(Debug, Clone, Copy)]
struct Flip {
    page: u32,
    from: Rectangle,
    to: Rectangle,
    progress: Tween,
    active: bool,
}

#[derive(Debug, Clone)]
struct Drag {
    page: u32,
    start: Point,
    current: Point,
    offset: Point,
    active: bool,
    before: Vec<u32>,
    last_slot: Option<usize>,
}

#[derive(Debug)]
struct LabScene {
    pages: Vec<Page>,
    order: Vec<u32>,
    slots: HashMap<u32, usize>,
    selected: u32,
    view: View,
    compact: bool,
    reduced_motion: bool,
    window: Size,
    sidebar: Spring,
    grid_cell: Spring,
    grid_scroll: Spring,
    reader_zoom: Spring,
    reader_scroll: Spring,
    positions: HashMap<u32, (Spring, Spring)>,
    press: Spring,
    pressed: Option<Control>,
    hovered: Option<Control>,
    tooltip: Tween,
    cursor: Point,
    drag: Option<Drag>,
    flip: Option<Flip>,
    history: Vec<HistoryEntry>,
    future: Vec<HistoryEntry>,
    invalid_states: usize,
}

#[derive(Debug, Clone, Copy)]
struct GridGeometry {
    columns: usize,
    rows: usize,
    cell_width: f32,
    cell_height: f32,
    pitch_x: f32,
    pitch_y: f32,
    max_scroll: f32,
}

impl LabScene {
    fn new(page_count: usize, compact: bool, reduced_motion: bool, window: Size) -> Self {
        let handles: Vec<image::Handle> = (0..12)
            .map(|variant| image::Handle::from_rgba(96, 128, synthetic_page(variant)))
            .collect();
        let pages: Vec<Page> = (0..page_count)
            .map(|index| Page {
                id: index as u32,
                handle: handles[index % handles.len()].clone(),
            })
            .collect();
        let order: Vec<u32> = pages.iter().map(|page| page.id).collect();
        let slots = order
            .iter()
            .enumerate()
            .map(|(slot, id)| (*id, slot))
            .collect();
        let selected = (page_count / 3).min(23) as u32;
        let reader_height = 660.0;
        let content_height = (window.height - TOP - 16.0).max(1.0);
        let reader_scroll = (selected as f32 * (reader_height + GAP)
            - (content_height - reader_height) / 2.0)
            .max(0.0);
        let mut scene = Self {
            pages,
            order,
            slots,
            selected,
            view: View::Reader,
            compact,
            reduced_motion,
            window,
            sidebar: Spring::new(SIDEBAR),
            grid_cell: Spring::new(214.0),
            grid_scroll: Spring::new(0.0),
            reader_zoom: Spring::new(1.0),
            reader_scroll: Spring::new(reader_scroll),
            positions: HashMap::new(),
            press: Spring::new(0.0),
            pressed: None,
            hovered: None,
            tooltip: Tween::new(0.0),
            cursor: Point::ORIGIN,
            drag: None,
            flip: None,
            history: Vec::new(),
            future: Vec::new(),
            invalid_states: 0,
        };
        scene.layout(window);
        scene
    }

    fn page(&self, id: u32) -> &Page {
        &self.pages[id as usize]
    }

    fn content_bounds(&self) -> Rectangle {
        let x = if self.view == View::Organizer {
            16.0
        } else {
            16.0 + self.sidebar.value
        };
        Rectangle::new(
            Point::new(x, TOP),
            Size::new(
                (self.window.width - x - 16.0).max(1.0),
                (self.window.height - TOP - 16.0).max(1.0),
            ),
        )
    }

    fn grid_geometry(&self) -> GridGeometry {
        let bounds = self.content_bounds();
        let columns = ((bounds.width + GAP) / (self.grid_cell.value + GAP))
            .floor()
            .max(1.0) as usize;
        let cell_width = ((bounds.width - (columns - 1) as f32 * GAP) / columns as f32).max(72.0);
        let cell_height = cell_width * 1.34 + 30.0;
        let rows = self.order.len().div_ceil(columns);
        let content_height = rows as f32 * cell_height + (rows.saturating_sub(1)) as f32 * GAP;
        GridGeometry {
            columns,
            rows,
            cell_width,
            cell_height,
            pitch_x: cell_width + GAP,
            pitch_y: cell_height + GAP,
            max_scroll: (content_height - bounds.height).max(0.0),
        }
    }

    fn reader_metrics(&self) -> (f32, f32, f32, f32) {
        let zoom = self.reader_zoom.value;
        let width = 510.0 * zoom;
        let height = 660.0 * zoom;
        let pitch = height + GAP;
        let content_height = self.pages.len() as f32 * pitch - GAP;
        let bounds = self.content_bounds();
        (
            width,
            height,
            pitch,
            (content_height - bounds.height).max(0.0),
        )
    }

    fn grid_target(&self, slot: usize, geometry: GridGeometry) -> Rectangle {
        let bounds = self.content_bounds();
        let row = slot / geometry.columns;
        let column = slot % geometry.columns;
        let extra = (bounds.width
            - geometry.columns as f32 * geometry.cell_width
            - (geometry.columns - 1) as f32 * GAP)
            .max(0.0)
            / 2.0;
        Rectangle::new(
            Point::new(
                bounds.x + extra + column as f32 * geometry.pitch_x,
                bounds.y + row as f32 * geometry.pitch_y - self.grid_scroll.value,
            ),
            Size::new(geometry.cell_width, geometry.cell_height),
        )
    }

    fn grid_rect(&self, id: u32, geometry: GridGeometry) -> Rectangle {
        let slot = self.slots.get(&id).copied().unwrap_or(0);
        let target = self.grid_target(slot, geometry);
        let position = self.positions.get(&id);
        let point = position.map_or(target.position(), |position| {
            Point::new(position.0.value, position.1.value)
        });
        Rectangle::new(point, target.size())
    }

    fn reader_rect(&self, id: u32) -> Rectangle {
        let slot = self.slots.get(&id).copied().unwrap_or(0);
        let bounds = self.content_bounds();
        let (width, height, pitch, _) = self.reader_metrics();
        Rectangle::new(
            Point::new(
                bounds.x + (bounds.width - width) / 2.0,
                bounds.y + slot as f32 * pitch - self.reader_scroll.value,
            ),
            Size::new(width, height),
        )
    }

    fn ensure_position(&mut self, id: u32, slot: usize, geometry: GridGeometry) {
        let target = self.grid_target(slot, geometry);
        let position = self
            .positions
            .entry(id)
            .or_insert_with(|| (Spring::new(target.x), Spring::new(target.y)));
        if self.reduced_motion {
            position.0.value = target.x;
            position.1.value = target.y;
            position.0.velocity = 0.0;
            position.1.velocity = 0.0;
        }
        position.0.retarget(target.x);
        position.1.retarget(target.y);
    }

    fn rebuild_slots(&mut self) {
        self.slots = self
            .order
            .iter()
            .enumerate()
            .map(|(slot, id)| (*id, slot))
            .collect();
    }

    fn layout(&mut self, window: Size) {
        self.window = window;
        let geometry = self.grid_geometry();
        let grid_target = self.grid_scroll.target.clamp(0.0, geometry.max_scroll);
        LabScene::retarget(self.reduced_motion, &mut self.grid_scroll, grid_target);
        let (_, _, _, reader_max) = self.reader_metrics();
        let reader_target = self.reader_scroll.target.clamp(0.0, reader_max);
        LabScene::retarget(self.reduced_motion, &mut self.reader_scroll, reader_target);

        if self.view != View::Organizer {
            self.positions.clear();
            return;
        }

        let visible = self.visible_slots();
        let mut keep = HashSet::with_capacity(visible.len() + 4);
        for slot in visible.clone() {
            let id = self.order[slot];
            keep.insert(id);
            self.ensure_position(id, slot, geometry);
        }
        let special = [
            Some(self.selected),
            self.drag.as_ref().map(|drag| drag.page),
            self.flip.as_ref().map(|flip| flip.page),
        ];
        for id in special.into_iter().flatten() {
            if let Some(slot) = self.slots.get(&id).copied()
                && keep.insert(id)
            {
                self.ensure_position(id, slot, geometry);
            }
        }
        self.positions.retain(|id, _| keep.contains(id));
    }

    fn retarget(reduced_motion: bool, spring: &mut Spring, target: f32) {
        spring.retarget(target);
        if reduced_motion {
            spring.value = target;
            spring.velocity = 0.0;
        }
    }

    fn advance(&mut self, dt: f32, now: Instant) {
        self.sidebar.advance(dt);
        self.grid_cell.advance(dt);
        self.grid_scroll.advance(dt);
        self.reader_zoom.advance(dt);
        self.reader_scroll.advance(dt);
        self.press.advance(dt);
        self.tooltip.advance(now);

        if let Some(flip) = &mut self.flip
            && !flip.progress.advance(now)
        {
            flip.active = false;
        }

        for position in self.positions.values_mut() {
            position.0.advance(dt);
            position.1.advance(dt);
        }

        if let Some(drag) = &self.drag
            && drag.active
        {
            self.scroll_drag_edge(dt);
        }
        self.layout(self.window);
    }

    fn is_active(&self) -> bool {
        self.sidebar.is_active()
            || self.grid_cell.is_active()
            || self.grid_scroll.is_active()
            || self.reader_zoom.is_active()
            || self.reader_scroll.is_active()
            || self.press.is_active()
            || self.tooltip.is_active(Instant::now())
            || self
                .positions
                .values()
                .any(|position| position.0.is_active() || position.1.is_active())
            || self.drag.as_ref().is_some_and(|drag| drag.active)
            || self.flip.as_ref().is_some_and(|flip| flip.active)
    }

    fn validate(&mut self) {
        let unique: HashSet<u32> = self.order.iter().copied().collect();
        if unique.len() != self.pages.len()
            || self.order.len() != self.pages.len()
            || self.slots.len() != self.order.len()
        {
            self.invalid_states += 1;
        }
        if !self.sidebar.value.is_finite()
            || !self.grid_cell.value.is_finite()
            || !self.grid_scroll.value.is_finite()
            || !self.reader_scroll.value.is_finite()
        {
            self.invalid_states += 1;
        }
    }

    fn visible_slots(&self) -> Range<usize> {
        let geometry = self.grid_geometry();
        let bounds = self.content_bounds();
        let first_row = ((self.grid_scroll.value - OVERSCAN) / geometry.pitch_y)
            .floor()
            .max(0.0) as usize;
        let last_row = ((self.grid_scroll.value + bounds.height + OVERSCAN) / geometry.pitch_y)
            .ceil() as usize;
        let first = (first_row * geometry.columns).min(self.order.len());
        let last = (last_row * geometry.columns).min(self.order.len());
        first..last
    }

    fn visible_reader_pages(&self) -> Range<usize> {
        let bounds = self.content_bounds();
        let (_, _, pitch, _) = self.reader_metrics();
        let first = ((self.reader_scroll.value - OVERSCAN) / pitch)
            .floor()
            .max(0.0) as usize;
        let last = ((self.reader_scroll.value + bounds.height + OVERSCAN) / pitch).ceil() as usize;
        first.min(self.order.len())..last.min(self.order.len())
    }

    fn switch_view(&mut self, view: View) {
        if self.view == view {
            return;
        }
        let from = match self.view {
            View::Reader => self.reader_rect(self.selected),
            View::Organizer => {
                let geometry = self.grid_geometry();
                self.grid_rect(self.selected, geometry)
            }
        };
        self.view = view;
        let selected_slot = self.slots.get(&self.selected).copied().unwrap_or(0);
        if view == View::Reader {
            let (_, height, pitch, _) = self.reader_metrics();
            let bounds = self.content_bounds();
            let target = (selected_slot as f32 * pitch - (bounds.height - height) / 2.0).max(0.0);
            LabScene::retarget(self.reduced_motion, &mut self.reader_scroll, target);
            if self.reduced_motion {
                self.reader_scroll.value = target;
            }
        } else {
            let geometry = self.grid_geometry();
            let slot = self.slots.get(&self.selected).copied().unwrap_or(0);
            let target = self.grid_target(slot, geometry);
            let bounds = self.content_bounds();
            if target.y < bounds.y || target.y + target.height > bounds.y + bounds.height {
                let desired = (slot / geometry.columns) as f32 * geometry.pitch_y - 32.0;
                LabScene::retarget(
                    self.reduced_motion,
                    &mut self.grid_scroll,
                    desired.clamp(0.0, geometry.max_scroll),
                );
            }
        }
        self.layout(self.window);
        let to = match view {
            View::Reader => {
                let bounds = self.content_bounds();
                let (width, height, pitch, _) = self.reader_metrics();
                Rectangle::new(
                    Point::new(
                        bounds.x + (bounds.width - width) / 2.0,
                        bounds.y + selected_slot as f32 * pitch - self.reader_scroll.target,
                    ),
                    Size::new(width, height),
                )
            }
            View::Organizer => {
                let geometry = self.grid_geometry();
                let slot = self.slots.get(&self.selected).copied().unwrap_or(0);
                let mut target = self.grid_target(slot, geometry);
                target.y -= self.grid_scroll.target;
                target
            }
        };
        if self.reduced_motion {
            self.flip = None;
        } else {
            let mut progress = Tween::new(0.0);
            progress.retarget(1.0, FLIP_DURATION);
            self.flip = Some(Flip {
                page: self.selected,
                from,
                to,
                progress,
                active: true,
            });
        }
    }

    fn begin_drag(&mut self, page: u32, point: Point, rect: Rectangle) {
        self.selected = page;
        self.drag = Some(Drag {
            page,
            start: point,
            current: point,
            offset: Point::new(point.x - rect.x, point.y - rect.y),
            active: false,
            before: self.order.clone(),
            last_slot: None,
        });
    }

    fn drag_to(&mut self, point: Point) {
        let Some(drag) = &mut self.drag else {
            return;
        };
        drag.current = point;
        if !drag.active && (point.x - drag.start.x).hypot(point.y - drag.start.y) >= DRAG_THRESHOLD
        {
            drag.active = true;
        }
        if drag.active {
            self.reorder_drag();
            self.scroll_drag_edge(1.0 / 60.0);
        }
    }

    fn scroll_drag_edge(&mut self, dt: f32) {
        let Some(drag) = &self.drag else {
            return;
        };
        let bounds = self.content_bounds();
        let edge = 56.0;
        let speed = if drag.current.y < bounds.y + edge {
            -(bounds.y + edge - drag.current.y) / edge * 520.0
        } else if drag.current.y > bounds.y + bounds.height - edge {
            (drag.current.y - (bounds.y + bounds.height - edge)) / edge * 520.0
        } else {
            0.0
        };
        if speed != 0.0 {
            let geometry = self.grid_geometry();
            let target = (self.grid_scroll.target + speed * dt).clamp(0.0, geometry.max_scroll);
            LabScene::retarget(self.reduced_motion, &mut self.grid_scroll, target);
            self.reorder_drag();
        }
    }

    fn reorder_drag(&mut self) {
        let geometry = self.grid_geometry();
        let bounds = self.content_bounds();
        let (page, current, last_slot) = {
            let Some(drag) = &mut self.drag else {
                return;
            };
            (drag.page, drag.current, drag.last_slot)
        };
        let column = ((current.x - bounds.x) / geometry.pitch_x).floor().max(0.0) as usize;
        let row = ((current.y - bounds.y + self.grid_scroll.value) / geometry.pitch_y)
            .floor()
            .max(0.0) as usize;
        let slot = (row * geometry.columns + column).min(self.order.len().saturating_sub(1));
        if last_slot == Some(slot) {
            return;
        }
        if let Some(drag) = &mut self.drag {
            drag.last_slot = Some(slot);
        }
        let Some(from) = self.slots.get(&page).copied() else {
            return;
        };
        let insertion = if from < slot { slot - 1 } else { slot };
        if from == insertion {
            return;
        }
        self.order.remove(from);
        self.order.insert(insertion.min(self.order.len()), page);
        self.rebuild_slots();
        self.layout(self.window);
    }

    fn finish_drag(&mut self) {
        let Some(drag) = self.drag.take() else {
            return;
        };
        if drag.active && drag.before != self.order {
            self.history.push(HistoryEntry {
                before: drag.before,
                after: self.order.clone(),
            });
            self.future.clear();
        }
        self.layout(self.window);
    }

    fn delete_selected(&mut self) {
        let Some(index) = self.slots.get(&self.selected).copied() else {
            return;
        };
        let before = self.order.clone();
        self.order.remove(index);
        self.rebuild_slots();
        let next = self.order.get(index).or_else(|| self.order.last()).copied();
        if let Some(next) = next {
            self.selected = next;
        }
        self.history.push(HistoryEntry {
            before,
            after: self.order.clone(),
        });
        self.future.clear();
        self.layout(self.window);
    }

    fn undo(&mut self) {
        let Some(entry) = self.history.pop() else {
            return;
        };
        self.future.push(entry.clone());
        self.order = entry.before;
        self.rebuild_slots();
        if !self.order.contains(&self.selected) {
            self.selected = self.order.first().copied().unwrap_or(0);
        }
        self.layout(self.window);
    }

    fn redo(&mut self) {
        let Some(entry) = self.future.pop() else {
            return;
        };
        self.history.push(entry.clone());
        self.order = entry.after;
        self.rebuild_slots();
        self.layout(self.window);
    }

    fn hover(&mut self, control: Option<Control>, now: Instant) {
        if self.hovered == control {
            return;
        }
        self.hovered = control;
        let duration = if self.reduced_motion {
            Duration::ZERO
        } else {
            Duration::from_millis(120)
        };
        self.tooltip
            .retarget(if control.is_some() { 1.0 } else { 0.0 }, duration);
        let _ = now;
    }

    fn auto_drag_goal(&self) -> Option<(Point, Point)> {
        let geometry = self.grid_geometry();
        let slot = self.slots.get(&self.selected).copied()?;
        let start = self.grid_rect(self.selected, geometry);
        let target_slot = (slot + 2).min(self.order.len().saturating_sub(1));
        let target = self.grid_target(target_slot, geometry);
        Some((
            Point::new(start.x + start.width / 2.0, start.y + start.height / 2.0),
            Point::new(
                target.x + target.width / 2.0,
                target.y + target.height / 2.0,
            ),
        ))
    }
}

fn synthetic_page(variant: usize) -> Vec<u8> {
    let mut pixels = Vec::with_capacity(96 * 128 * 4);
    for y in 0..128 {
        for x in 0..96 {
            let band = ((x / 12 + y / 16 + variant) % 3) as u8;
            pixels.extend_from_slice(&[
                244 - band * 5,
                242 - y as u8 / 8,
                238 - variant as u8 * 2,
                255,
            ]);
            if y % 18 == 0 && (8..88).contains(&x) {
                let start = pixels.len() - 4;
                pixels[start..].copy_from_slice(&[55, 63, 78, 70]);
            }
        }
    }
    pixels
}

const fn color(hex: u32, alpha: f32) -> Color {
    Color {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a: alpha,
    }
}

const SURFACE: Color = color(0x202226, 0.97);
const RAISED: Color = color(0x2b2e33, 1.0);
const CANVAS: Color = color(0x15171b, 1.0);
const TEXT: Color = color(0xf1f2f4, 1.0);
const MUTED: Color = color(0x9da3ad, 1.0);
const ACCENT: Color = color(0xffffff, 1.0);
const SELECTION: Color = color(0xffffff, 0.14);

fn fill(
    frame: &mut canvas::Frame<iced::Renderer>,
    bounds: Rectangle,
    background: Color,
    radius: f32,
) {
    frame.fill(
        &canvas::Path::rounded_rectangle(bounds.position(), bounds.size(), border::radius(radius)),
        background,
    );
}

fn label(
    frame: &mut canvas::Frame<iced::Renderer>,
    content: &str,
    position: Point,
    size: f32,
    tint: Color,
    _clip: Rectangle,
) {
    frame.fill_text(canvas::Text {
        content: content.to_string(),
        position,
        max_width: 1200.0,
        color: tint,
        size: size.into(),
        line_height: iced::advanced::text::LineHeight::default(),
        font: iced::Font::default(),
        align_x: iced::advanced::text::Alignment::Left,
        align_y: iced::alignment::Vertical::Top,
        shaping: iced::advanced::text::Shaping::Auto,
    });
}

fn controls(window: Size, compact: bool) -> Vec<(Control, Rectangle, &'static str)> {
    let small = compact || window.width < BREAKPOINT;
    let mut result = Vec::new();
    if !small {
        let items = [
            (Control::Auto, "Run"),
            (Control::ToggleSidebar, "Sidebar"),
            (Control::ToggleView, "View"),
            (Control::Undo, "Undo"),
            (Control::Redo, "Redo"),
        ];
        let mut x = 18.0;
        for (control, name) in items {
            let width = match control {
                Control::ToggleSidebar => 86.0,
                Control::ToggleView => 68.0,
                _ => 58.0,
            };
            result.push((
                control,
                Rectangle::new(Point::new(x, 18.0), Size::new(width, 40.0)),
                name,
            ));
            x += width + 8.0;
        }
    } else {
        result.push((
            Control::ToggleView,
            Rectangle::new(Point::new(18.0, 18.0), Size::new(68.0, 40.0)),
            "View",
        ));
    }

    let right_items = [
        (Control::ZoomOut, "−"),
        (Control::ZoomIn, "+"),
        (Control::Delete, "Delete"),
    ];
    let mut right = window.width - 304.0;
    for (control, name) in right_items {
        let width = if control == Control::Delete {
            68.0
        } else {
            36.0
        };
        result.push((
            control,
            Rectangle::new(Point::new(right, 18.0), Size::new(width, 40.0)),
            name,
        ));
        right += width + 6.0;
    }
    result.push((
        Control::Minimize,
        Rectangle::new(
            Point::new(window.width - 140.0, 18.0),
            Size::new(36.0, 40.0),
        ),
        "—",
    ));
    result.push((
        Control::Maximize,
        Rectangle::new(Point::new(window.width - 96.0, 18.0), Size::new(44.0, 40.0)),
        "□",
    ));
    result.push((
        Control::Close,
        Rectangle::new(Point::new(window.width - 44.0, 18.0), Size::new(36.0, 40.0)),
        "×",
    ));
    result
}

fn hit_control(point: Point, window: Size, compact: bool) -> Option<Control> {
    controls(window, compact)
        .into_iter()
        .find(|(_, bounds, _)| bounds.contains(point))
        .map(|(control, _, _)| control)
}

fn hit_page(scene: &LabScene, point: Point) -> Option<(u32, Rectangle)> {
    if scene.view != View::Organizer {
        return None;
    }
    let geometry = scene.grid_geometry();
    for slot in scene.visible_slots() {
        let id = scene.order[slot];
        let rect = scene.grid_rect(id, geometry);
        if rect.contains(point) {
            return Some((id, rect));
        }
    }
    None
}

fn drag_ghost_rect(scene: &LabScene, geometry: GridGeometry) -> Option<Rectangle> {
    let drag = scene.drag.as_ref().filter(|drag| drag.active)?;
    let source = scene.grid_rect(drag.page, geometry);
    Some(Rectangle::new(
        Point::new(
            drag.current.x - drag.offset.x,
            drag.current.y - drag.offset.y,
        ),
        source.size(),
    ))
}

fn flip_rect(flip: &Flip) -> Rectangle {
    let t = flip.progress.value;
    Rectangle::new(
        Point::new(
            flip.from.x + (flip.to.x - flip.from.x) * t,
            flip.from.y + (flip.to.y - flip.from.y) * t,
        ),
        Size::new(
            flip.from.width + (flip.to.width - flip.from.width) * t,
            flip.from.height + (flip.to.height - flip.from.height) * t,
        ),
    )
}

fn draw_page(
    frame: &mut canvas::Frame<iced::Renderer>,
    page: &Page,
    bounds: Rectangle,
    selected: bool,
    clip: Rectangle,
) {
    frame.with_clip(clip, |frame| {
        if selected {
            fill(
                frame,
                Rectangle::new(
                    bounds.position() - Vector::new(2.0, 2.0),
                    Size::new(bounds.width + 4.0, bounds.height + 4.0),
                ),
                ACCENT,
                7.0,
            );
        }
        fill(frame, bounds, Color::WHITE, 5.0);
        frame.draw_image(bounds, image::Image::new(page.handle.clone()));
    });
}

struct LabCanvas {
    scene: Rc<RefCell<LabScene>>,
    redraws: Rc<Cell<usize>>,
}

impl canvas::Program<Message> for LabCanvas {
    type State = ();

    fn update(
        &self,
        _state: &mut Self::State,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        if matches!(event, Event::Window(window::Event::RedrawRequested(_))) {
            self.redraws.set(self.redraws.get() + 1);
            return None;
        }
        let Event::Mouse(mouse_event) = event else {
            return None;
        };
        let mut scene = self.scene.borrow_mut();
        let window = Size::new(bounds.width, bounds.height);
        match mouse_event {
            mouse::Event::CursorMoved { position } => {
                scene.cursor = *position;
                let control = hit_control(*position, window, scene.compact);
                scene.hover(control, Instant::now());
                if scene.drag.as_ref().is_some_and(|drag| drag.active) {
                    scene.drag_to(*position);
                }
                Some(canvas::Action::publish(Message::SceneChanged))
            }
            mouse::Event::CursorEntered => Some(canvas::Action::publish(Message::SceneChanged)),
            mouse::Event::CursorLeft => {
                scene.hover(None, Instant::now());
                if scene.drag.as_ref().is_some_and(|drag| drag.active) {
                    scene.finish_drag();
                }
                Some(canvas::Action::publish(Message::SceneChanged))
            }
            mouse::Event::ButtonPressed(mouse::Button::Left) => {
                let position = cursor.position()?;
                if let Some(control) = hit_control(position, window, scene.compact) {
                    scene.pressed = Some(control);
                    LabScene::retarget(scene.reduced_motion, &mut scene.press, 1.0);
                    return Some(canvas::Action::publish(Message::Control(control)));
                }
                if let Some((page, rect)) = hit_page(&scene, position) {
                    scene.begin_drag(page, position, rect);
                    return Some(canvas::Action::publish(Message::SceneChanged));
                }
                None
            }
            mouse::Event::ButtonReleased(mouse::Button::Left) => {
                scene.pressed = None;
                LabScene::retarget(scene.reduced_motion, &mut scene.press, 0.0);
                if scene.drag.is_some() {
                    scene.finish_drag();
                }
                Some(canvas::Action::publish(Message::SceneChanged))
            }
            mouse::Event::WheelScrolled { delta } => {
                let amount = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y * 72.0,
                    mouse::ScrollDelta::Pixels { y, .. } => *y,
                };
                match scene.view {
                    View::Organizer => {
                        let max = scene.grid_geometry().max_scroll;
                        let target = (scene.grid_scroll.target - amount).clamp(0.0, max);
                        LabScene::retarget(scene.reduced_motion, &mut scene.grid_scroll, target);
                    }
                    View::Reader => {
                        let max = scene.reader_metrics().3;
                        let target = (scene.reader_scroll.target - amount).clamp(0.0, max);
                        LabScene::retarget(scene.reduced_motion, &mut scene.reader_scroll, target);
                    }
                }
                Some(canvas::Action::publish(Message::SceneChanged))
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry<iced::Renderer>> {
        let scene = self.scene.borrow();
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        fill(&mut frame, bounds, CANVAS, 0.0);
        let window = Size::new(bounds.width, bounds.height);
        let content = scene.content_bounds();
        let geometry = scene.grid_geometry();
        let drag_overlay = drag_ghost_rect(&scene, geometry);
        let flip_overlay = scene
            .flip
            .as_ref()
            .filter(|flip| flip.active)
            .map(flip_rect);

        for (control, rect, name) in controls(window, scene.compact) {
            let active = match control {
                Control::ToggleSidebar => scene.sidebar.value > SIDEBAR / 2.0,
                Control::ToggleView => scene.view == View::Organizer,
                _ => false,
            };
            let hovered = scene.hovered == Some(control);
            let background = if active {
                SELECTION
            } else if hovered {
                color(0xffffff, 0.08 * scene.tooltip.value)
            } else {
                SURFACE
            };
            let scale = if scene.pressed == Some(control) {
                1.0 - scene.press.value * 0.04
            } else {
                1.0
            };
            let scaled = Rectangle::new(
                Point::new(
                    rect.x + (rect.width - rect.width * scale) / 2.0,
                    rect.y + (rect.height - rect.height * scale) / 2.0,
                ),
                rect.size() * scale,
            );
            fill(&mut frame, scaled, background, 20.0);
            label(
                &mut frame,
                name,
                Point::new(scaled.x + 12.0, scaled.y + 12.0),
                13.0,
                if active { ACCENT } else { TEXT },
                scaled,
            );
        }

        if scene.view == View::Reader && scene.sidebar.value > 1.0 {
            let sidebar = Rectangle::new(
                Point::new(16.0, TOP),
                Size::new((scene.sidebar.value - 8.0).max(0.0), content.height),
            );
            fill(&mut frame, sidebar, SURFACE, 18.0);
            if !flip_overlay.is_some_and(|overlay| overlay.intersects(&sidebar)) {
                label(
                    &mut frame,
                    "Pages",
                    Point::new(sidebar.x + 18.0, sidebar.y + 18.0),
                    14.0,
                    TEXT,
                    sidebar,
                );
            }
            let (_, _, pitch, _) = scene.reader_metrics();
            let first = ((scene.reader_scroll.value - 40.0) / pitch)
                .floor()
                .max(0.0) as usize;
            let count = (sidebar.height / 42.0).ceil() as usize + 2;
            for slot in first..(first + count).min(scene.order.len()) {
                let id = scene.order[slot];
                let row = Rectangle::new(
                    Point::new(
                        sidebar.x + 10.0,
                        sidebar.y + 48.0 + (slot - first) as f32 * 42.0,
                    ),
                    Size::new(sidebar.width - 20.0, 36.0),
                );
                let selected = id == scene.selected;
                fill(
                    &mut frame,
                    row,
                    if selected {
                        SELECTION
                    } else {
                        Color::TRANSPARENT
                    },
                    8.0,
                );
                if flip_overlay.is_some_and(|overlay| overlay.intersects(&row)) {
                    continue;
                }
                label(
                    &mut frame,
                    &format!("Page {}", slot + 1),
                    Point::new(row.x + 12.0, row.y + 9.0),
                    12.0,
                    if selected { ACCENT } else { MUTED },
                    row,
                );
            }
        }

        frame.with_clip(content, |frame| {
            if scene.view == View::Reader {
                for slot in scene.visible_reader_pages() {
                    let id = scene.order[slot];
                    let page = scene.page(id);
                    if scene
                        .flip
                        .as_ref()
                        .is_some_and(|flip| flip.active && flip.page == page.id)
                    {
                        continue;
                    }
                    let rect = scene.reader_rect(page.id);
                    if rect.intersects(&content) {
                        draw_page(frame, page, rect, page.id == scene.selected, content);
                    }
                }
            } else {
                let geometry = scene.grid_geometry();
                for slot in scene.visible_slots() {
                    let id = scene.order[slot];
                    if scene
                        .drag
                        .as_ref()
                        .is_some_and(|drag| drag.active && drag.page == id)
                        || scene
                            .flip
                            .as_ref()
                            .is_some_and(|flip| flip.active && flip.page == id)
                    {
                        continue;
                    }
                    let rect = scene.grid_rect(id, geometry);
                    let Some(visible) = rect.intersection(&content) else {
                        continue;
                    };
                    if visible.width <= 0.0 || visible.height <= 0.0 {
                        continue;
                    }
                    let selected = id == scene.selected;
                    fill(frame, rect, if selected { SELECTION } else { RAISED }, 10.0);
                    let image_bounds = Rectangle::new(
                        Point::new(rect.x + 5.0, rect.y + 5.0),
                        Size::new(rect.width - 10.0, rect.height - 34.0),
                    );
                    draw_page(
                        frame,
                        &scene.pages[id as usize],
                        image_bounds,
                        false,
                        content,
                    );
                    if drag_overlay.is_some_and(|overlay| overlay.intersects(&rect))
                        || flip_overlay.is_some_and(|overlay| overlay.intersects(&rect))
                    {
                        continue;
                    }
                    label(
                        frame,
                        &format!("Page {}", id + 1),
                        Point::new(rect.x + 10.0, rect.y + rect.height - 23.0),
                        11.0,
                        if selected { ACCENT } else { MUTED },
                        content,
                    );
                }
            }
        });

        if let Some(drag) = &scene.drag
            && drag.active
        {
            let geometry = scene.grid_geometry();
            if let Some(slot) = drag.last_slot {
                let target =
                    scene.grid_target(slot.min(scene.order.len().saturating_sub(1)), geometry);
                frame.with_clip(content, |frame| {
                    fill(frame, target, color(0xffffff, 0.08), 10.0);
                    frame.stroke(
                        &canvas::Path::rounded_rectangle(
                            target.position(),
                            target.size(),
                            border::radius(10.0),
                        ),
                        canvas::Stroke::default().with_color(ACCENT).with_width(2.0),
                    );
                });
            }
            if let Some(ghost) = drag_overlay {
                frame.with_clip(content, |frame| {
                    fill(
                        frame,
                        Rectangle::new(ghost.position() + Vector::new(0.0, 12.0), ghost.size()),
                        color(0x000000, 0.32),
                        12.0,
                    );
                    fill(frame, ghost, RAISED, 10.0);
                    draw_page(
                        frame,
                        &scene.pages[drag.page as usize],
                        Rectangle::new(
                            ghost.position() + Vector::new(5.0, 5.0),
                            Size::new(ghost.width - 10.0, ghost.height - 34.0),
                        ),
                        true,
                        content,
                    );
                });
            }
        }

        if let Some(flip) = &scene.flip
            && flip.active
        {
            let rect = flip_rect(flip);
            frame.with_clip(content, |frame| {
                fill(
                    frame,
                    Rectangle::new(rect.position() + Vector::new(0.0, 8.0), rect.size()),
                    color(0x000000, 0.28),
                    8.0,
                );
                draw_page(frame, &scene.pages[flip.page as usize], rect, true, content);
            });
        }

        if let Some(control) = scene.hovered
            && let Some((_, rect, name)) = controls(window, scene.compact)
                .into_iter()
                .find(|(item, _, _)| *item == control)
        {
            let tooltip = Rectangle::new(
                Point::new(rect.x, rect.y + rect.height + 8.0),
                Size::new((name.len() as f32 * 7.0 + 24.0).max(64.0), 30.0),
            );
            fill(
                &mut frame,
                tooltip,
                color(0x000000, 0.82 * scene.tooltip.value),
                8.0,
            );
            label(
                &mut frame,
                name,
                Point::new(tooltip.x + 12.0, tooltip.y + 7.0),
                12.0,
                color(0xffffff, scene.tooltip.value),
                tooltip,
            );
        }
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Self::State,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        let scene = self.scene.borrow();
        if scene.drag.as_ref().is_some_and(|drag| drag.active) {
            mouse::Interaction::Grabbing
        } else if scene.hovered.is_some() || hit_page(&scene, scene.cursor).is_some() {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::Idle
        }
    }
}

const STAGE_NAMES: [&str; 9] = [
    "sidebar-collapse",
    "sidebar-retarget",
    "sidebar-expand",
    "reader-to-organizer",
    "organizer-to-reader",
    "prepare-drag",
    "drag-reorder",
    "grid-resize",
    "hover-tooltip",
];

#[derive(Debug)]
struct AutoState {
    stage: usize,
    started: Instant,
    entered: bool,
    midpoint_retargeted: bool,
    duration: Duration,
    drag_start: Option<Point>,
    drag_goal: Option<Point>,
}

impl AutoState {
    fn new(reduced_motion: bool) -> Self {
        Self {
            stage: 0,
            started: Instant::now(),
            entered: false,
            midpoint_retargeted: false,
            duration: if reduced_motion {
                Duration::from_millis(120)
            } else {
                Duration::from_millis(900)
            },
            drag_start: None,
            drag_goal: None,
        }
    }

    fn tick(&mut self, scene: &mut LabScene, now: Instant, dt: f32) -> bool {
        if !self.entered {
            self.enter(scene, now);
            self.entered = true;
        }
        if self.stage >= STAGE_NAMES.len() {
            scene.advance(dt, now);
            return scene.is_active();
        }
        let elapsed = now.duration_since(self.started);
        if elapsed >= self.duration {
            self.stage += 1;
            self.started = now;
            if self.stage >= STAGE_NAMES.len() {
                scene.advance(dt, now);
                return scene.is_active();
            }
            self.enter(scene, now);
        }
        let progress = (now.duration_since(self.started).as_secs_f32()
            / self.duration.as_secs_f32())
        .clamp(0.0, 1.0);
        if self.stage == 1
            && !self.midpoint_retargeted
            && now.duration_since(self.started) >= self.duration / 2
        {
            LabScene::retarget(scene.reduced_motion, &mut scene.sidebar, 0.0);
            self.midpoint_retargeted = true;
        }
        if self.stage == 6
            && let (Some(start), Some(goal)) = (self.drag_start, self.drag_goal)
        {
            let eased = progress * progress * (3.0 - 2.0 * progress);
            scene.drag_to(Point::new(
                start.x + (goal.x - start.x) * eased,
                start.y + (goal.y - start.y) * eased,
            ));
        }
        scene.advance(dt, now);
        self.stage < STAGE_NAMES.len() || scene.is_active()
    }

    fn enter(&mut self, scene: &mut LabScene, now: Instant) {
        self.midpoint_retargeted = false;
        match self.stage {
            0 => LabScene::retarget(scene.reduced_motion, &mut scene.sidebar, 0.0),
            1 | 2 => LabScene::retarget(scene.reduced_motion, &mut scene.sidebar, SIDEBAR),
            3 => scene.switch_view(View::Organizer),
            4 => scene.switch_view(View::Reader),
            5 => {
                scene.switch_view(View::Organizer);
                scene.layout(scene.window);
                let geometry = scene.grid_geometry();
                let rect = scene.grid_rect(scene.selected, geometry);
                scene.begin_drag(
                    scene.selected,
                    Point::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0),
                    rect,
                );
                let goal = scene.auto_drag_goal();
                if let Some((start, goal)) = goal {
                    self.drag_start = Some(start);
                    self.drag_goal = Some(goal);
                }
            }
            6 => {}
            7 => {
                if scene.drag.is_some() {
                    scene.finish_drag();
                }
                let target = if scene.grid_cell.target > 210.0 {
                    154.0
                } else {
                    268.0
                };
                LabScene::retarget(scene.reduced_motion, &mut scene.grid_cell, target);
                let max = scene.grid_geometry().max_scroll;
                let target = (scene.grid_scroll.target + 180.0).clamp(0.0, max);
                LabScene::retarget(scene.reduced_motion, &mut scene.grid_scroll, target);
            }
            8 => {
                scene.hovered = Some(Control::ToggleView);
                scene.hover(Some(Control::ToggleView), now);
            }
            _ => {
                scene.hover(None, now);
                if scene.drag.is_some() {
                    scene.finish_drag();
                }
            }
        }
    }

    fn name(&self) -> &'static str {
        STAGE_NAMES.get(self.stage).copied().unwrap_or("settle")
    }
}

#[derive(Debug, Clone)]
struct RunSettings {
    pages: usize,
    compact: bool,
    reduced_motion: bool,
    auto: bool,
    stage_ms: u64,
    width: f32,
    height: f32,
    report: Option<PathBuf>,
    screenshot: Option<PathBuf>,
    capture_stage: Option<String>,
    capture_after_ms: Option<u64>,
    stage_screenshot: Option<PathBuf>,
    exit_on_complete: bool,
    window: window::Settings,
}

impl RunSettings {
    fn parse() -> Result<Option<Self>, String> {
        let mut settings = Self {
            pages: 2000,
            compact: false,
            reduced_motion: false,
            auto: false,
            stage_ms: 900,
            width: 1360.0,
            height: 870.0,
            report: None,
            screenshot: None,
            capture_stage: None,
            capture_after_ms: None,
            stage_screenshot: None,
            exit_on_complete: false,
            window: window::Settings::default(),
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            let mut value = || -> Result<String, String> {
                args.next()
                    .ok_or_else(|| format!("missing value for {arg}"))
            };
            match arg.as_str() {
                "--help" | "-h" => {
                    println!(
                        "Folio Flow motion lab\n\n--pages N --compact --reduced-motion --auto --stage-ms N\n--width N --height N --report PATH --screenshot PATH --exit-on-complete\n--capture-stage NAME --stage-screenshot PATH [--capture-after-ms N]"
                    );
                    return Ok(None);
                }
                "--pages" => settings.pages = value()?.parse().map_err(|_| "invalid --pages")?,
                "--stage-ms" => {
                    settings.stage_ms = value()?.parse().map_err(|_| "invalid --stage-ms")?
                }
                "--width" => settings.width = value()?.parse().map_err(|_| "invalid --width")?,
                "--height" => settings.height = value()?.parse().map_err(|_| "invalid --height")?,
                "--report" => settings.report = Some(PathBuf::from(value()?)),
                "--screenshot" => settings.screenshot = Some(PathBuf::from(value()?)),
                "--capture-stage" => settings.capture_stage = Some(value()?),
                "--capture-after-ms" => {
                    settings.capture_after_ms =
                        Some(value()?.parse().map_err(|_| "invalid --capture-after-ms")?)
                }
                "--stage-screenshot" => settings.stage_screenshot = Some(PathBuf::from(value()?)),
                "--compact" => settings.compact = true,
                "--reduced-motion" => settings.reduced_motion = true,
                "--auto" => settings.auto = true,
                "--exit-on-complete" => settings.exit_on_complete = true,
                unknown => return Err(format!("unknown argument: {unknown}")),
            }
        }
        settings.pages = settings.pages.clamp(24, 10_000);
        settings.stage_ms = settings.stage_ms.clamp(120, 10_000);
        settings.width = settings.width.clamp(600.0, 7680.0);
        settings.height = settings.height.clamp(380.0, 4320.0);
        if let Some(stage) = &settings.capture_stage
            && !STAGE_NAMES.contains(&stage.as_str())
        {
            return Err(format!("unknown --capture-stage: {stage}"));
        }
        if settings.capture_stage.is_some() != settings.stage_screenshot.is_some() {
            return Err("--capture-stage and --stage-screenshot must be used together".into());
        }
        settings.window.size = Size::new(settings.width, settings.height);
        settings.window.min_size = Some(Size::new(600.0, 380.0));
        Ok(Some(settings))
    }
}

#[derive(Debug)]
enum Message {
    WindowReady(Option<window::Id>),
    WindowEvent(iced::window::Event),
    ScaleFactor(f32),
    Frame(Instant),
    SceneChanged,
    Control(Control),
    IdleElapsed,
    Screenshot(window::Screenshot),
    StageScreenshot(PathBuf, window::Screenshot),
}

struct MotionApp {
    settings: RunSettings,
    scene: Rc<RefCell<LabScene>>,
    window: Option<window::Id>,
    scale_factor: f32,
    auto: Option<AutoState>,
    last_frame: Option<Instant>,
    frames: FrameBudget,
    stages: Vec<String>,
    view_builds: Cell<usize>,
    redraws: Rc<Cell<usize>>,
    idle_start_redraws: usize,
    idle_observed_redraws: usize,
    animating: bool,
    completion_started: bool,
    errors: Vec<String>,
    screenshot_info: Option<(u32, u32, f32)>,
    stage_capture_done: bool,
}

impl MotionApp {
    fn new(settings: RunSettings) -> (Self, Task<Message>) {
        let scene = LabScene::new(
            settings.pages,
            settings.compact,
            settings.reduced_motion,
            Size::new(settings.width, settings.height),
        );
        let app = Self {
            settings,
            scene: Rc::new(RefCell::new(scene)),
            window: None,
            scale_factor: 1.0,
            auto: None,
            last_frame: None,
            frames: FrameBudget::default(),
            stages: Vec::new(),
            view_builds: Cell::new(0),
            redraws: Rc::new(Cell::new(0)),
            idle_start_redraws: 0,
            idle_observed_redraws: 0,
            animating: false,
            completion_started: false,
            errors: Vec::new(),
            screenshot_info: None,
            stage_capture_done: false,
        };
        (app, window::oldest().map(Message::WindowReady))
    }

    fn start_auto(&mut self) {
        if self.auto.is_none() && !self.completion_started {
            self.auto = Some(AutoState::new(self.settings.reduced_motion));
            self.auto.as_mut().expect("auto state").duration =
                Duration::from_millis(self.settings.stage_ms);
            self.last_frame = None;
        }
    }

    fn finish(&mut self) -> Task<Message> {
        if self.completion_started {
            return Task::none();
        }
        if std::env::var_os("MOTION_LAB_TRACE").is_some() {
            eprintln!(
                "motion lab: finish after {} measured frames",
                self.frames.samples.len()
            );
        }
        self.auto = None;
        self.completion_started = true;
        self.scene.borrow_mut().validate();
        self.idle_start_redraws = self.redraws.get();
        Task::perform(
            async {
                tokio::time::sleep(Duration::from_millis(1000)).await;
            },
            |_| Message::IdleElapsed,
        )
    }

    fn write_report(&self) {
        let Some(path) = &self.settings.report else {
            return;
        };
        let scene = self.scene.borrow();
        let values = self.frames.percentiles();
        let geometry = scene.grid_geometry();
        let visible = scene.visible_slots().len();
        let mut json = String::new();
        let _ = writeln!(json, "{{");
        let _ = writeln!(json, "  \"tool\": \"folio-motion-lab\",");
        let _ = writeln!(json, "  \"schema\": 1,");
        let _ = writeln!(
            json,
            "  \"window\": {{\"width\": {}, \"height\": {}, \"scale_factor\": {}}},",
            scene.window.width, scene.window.height, self.scale_factor
        );
        let _ = writeln!(
            json,
            "  \"ui\": {{\"compact\": {}, \"logical_breakpoint\": {}, \"view\": \"{:?}\"}},",
            scene.compact, BREAKPOINT, scene.view
        );
        let _ = writeln!(json, "  \"pages\": {},", scene.pages.len());
        let _ = writeln!(json, "  \"reduced_motion\": {},", scene.reduced_motion);
        let _ = writeln!(
            json,
            "  \"environment\": {{\"backend\": {}, \"present_mode\": {}}},",
            json_string(&std::env::var("WGPU_BACKEND").unwrap_or_else(|_| "default".into())),
            json_string(&std::env::var("ICED_PRESENT_MODE").unwrap_or_else(|_| "default".into()))
        );
        let _ = writeln!(
            json,
            "  \"frames\": {{\"count\": {}, \"p50_ms\": {}, \"p95_ms\": {}, \"p99_ms\": {}, \"max_ms\": {}}},",
            values.count, values.p50, values.p95, values.p99, values.max
        );
        let _ = writeln!(json, "  \"frame_intervals_ms\": {:?},", self.frames.samples);
        let _ = writeln!(json, "  \"stages\": {:?},", self.stages);
        let _ = writeln!(json, "  \"view_builds\": {},", self.view_builds.get());
        let _ = writeln!(json, "  \"window_redraw_events\": {},", self.redraws.get());
        let _ = writeln!(
            json,
            "  \"idle_window_redraw_events\": {},",
            self.idle_observed_redraws
        );
        let _ = writeln!(
            json,
            "  \"grid\": {{\"columns\": {}, \"rows\": {}, \"visible_slots_with_overscan\": {}, \"tracked_position_springs\": {}, \"overscan_px\": {}, \"cell_width\": {}}},",
            geometry.columns,
            geometry.rows,
            visible,
            scene.positions.len(),
            OVERSCAN,
            geometry.cell_width
        );
        let _ = writeln!(json, "  \"history_entries\": {},", scene.history.len());
        let _ = writeln!(json, "  \"invalid_states\": {},", scene.invalid_states);
        let _ = writeln!(json, "  \"errors\": {:?},", self.errors);
        if let Some((width, height, scale)) = self.screenshot_info {
            let _ = writeln!(
                json,
                "  \"screenshot\": {{\"width\": {width}, \"height\": {height}, \"scale_factor\": {scale}}},"
            );
        }
        let _ = writeln!(
            json,
            "  \"notes\": [\"RedrawRequested intervals are not GPU presentation or scanout latency.\", \"view_builds is not an Iced layout-pass count.\", \"Synthetic RGBA pages isolate UI motion from PDF Engine and tile scheduling.\"]"
        );
        let _ = writeln!(json, "}}");
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(error) = std::fs::write(path, json) {
            eprintln!("motion lab: failed to write report: {error}");
        }
    }

    fn after_idle(&mut self) -> Task<Message> {
        self.idle_observed_redraws = self.redraws.get() - self.idle_start_redraws;
        if let (Some(id), Some(path)) = (self.window, self.settings.screenshot.clone()) {
            return window::screenshot(id).map(move |screenshot| {
                if let Some(parent) = path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }
                Message::Screenshot(screenshot)
            });
        }
        self.write_report();
        if self.settings.exit_on_complete
            && let Some(id) = self.window
        {
            return window::close::<Message>(id).discard();
        }
        Task::none()
    }
}

fn json_string(value: &str) -> String {
    let mut output = String::from("\"");
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character < ' ' => {
                let _ = write!(output, "\\u{:04x}", character as u32);
            }
            character => output.push(character),
        }
    }
    output.push('"');
    output
}

fn save_screenshot(
    path: &std::path::Path,
    screenshot: &window::Screenshot,
) -> Result<(u32, u32, f32), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    use image_crate::ImageEncoder;

    let mut file = std::fs::File::create(path).map_err(|error| error.to_string())?;
    image_crate::codecs::png::PngEncoder::new(&mut file)
        .write_image(
            screenshot.rgba.as_ref(),
            screenshot.size.width,
            screenshot.size.height,
            image_crate::ExtendedColorType::Rgba8,
        )
        .map_err(|error| error.to_string())?;
    Ok((
        screenshot.size.width,
        screenshot.size.height,
        screenshot.scale_factor,
    ))
}

impl MotionApp {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::WindowReady(id) => {
                if std::env::var_os("MOTION_LAB_TRACE").is_some() {
                    eprintln!("motion lab: window ready {id:?}");
                }
                self.window = id;
                if self.settings.auto {
                    self.start_auto();
                }
                if let Some(id) = id {
                    return Task::batch([
                        window::set_min_size::<Message>(id, Some(Size::new(600.0, 380.0)))
                            .discard(),
                        window::scale_factor(id).map(Message::ScaleFactor),
                    ]);
                }
            }
            Message::WindowEvent(event) => match event {
                iced::window::Event::Resized(size) => self.scene.borrow_mut().layout(size),
                iced::window::Event::Rescaled(scale) => self.scale_factor = scale,
                _ => {}
            },
            Message::ScaleFactor(scale) => self.scale_factor = scale,
            Message::Frame(now) => {
                if self.completion_started {
                    return Task::none();
                }
                if !self.auto.is_some() && !self.scene.borrow().is_active() {
                    self.animating = false;
                    self.last_frame = None;
                    return Task::none();
                }
                if !self.animating {
                    self.last_frame = None;
                }
                self.animating = true;
                let measured_dt = self
                    .last_frame
                    .map(|last| now.duration_since(last).as_secs_f32());
                let dt = measured_dt.unwrap_or(1.0 / 60.0).clamp(0.0, 0.25);
                self.last_frame = Some(now);
                if let Some(dt) = measured_dt {
                    self.frames.record(dt * 1000.0);
                }
                if std::env::var_os("MOTION_LAB_TRACE").is_some()
                    && self.frames.samples.len().is_multiple_of(60)
                {
                    eprintln!(
                        "motion lab: frame {} stage {:?}",
                        self.frames.samples.len(),
                        self.auto.as_ref().map(|auto| auto.name())
                    );
                }
                if std::env::var_os("MOTION_LAB_TRACE").is_some()
                    && self.frames.samples.len().is_multiple_of(600)
                {
                    let scene = self.scene.borrow();
                    eprintln!(
                        "motion lab: state sidebar={}/{} grid_cell={}/{} grid_scroll={}/{} reader_zoom={}/{} reader_scroll={}/{} press={}/{} tooltip={}/{} active_positions={} drag={:?} flip={:?}",
                        scene.sidebar.value,
                        scene.sidebar.target,
                        scene.grid_cell.value,
                        scene.grid_cell.target,
                        scene.grid_scroll.value,
                        scene.grid_scroll.target,
                        scene.reader_zoom.value,
                        scene.reader_zoom.target,
                        scene.reader_scroll.value,
                        scene.reader_scroll.target,
                        scene.press.value,
                        scene.press.target,
                        scene.tooltip.value,
                        scene.tooltip.to,
                        scene
                            .positions
                            .values()
                            .filter(|position| position.0.is_active() || position.1.is_active())
                            .count(),
                        scene.drag.as_ref().map(|drag| drag.active),
                        scene.flip.as_ref().map(|flip| flip.active)
                    );
                }
                if let Some(auto) = &mut self.auto {
                    self.stages.push(auto.name().to_string());
                }
                let active = if let Some(auto) = &mut self.auto {
                    auto.tick(&mut self.scene.borrow_mut(), now, dt)
                } else {
                    let mut scene = self.scene.borrow_mut();
                    scene.advance(dt, now);
                    scene.is_active()
                };
                let capture_stage = self.auto.as_ref().and_then(|auto| {
                    let delay = self
                        .settings
                        .capture_after_ms
                        .map(Duration::from_millis)
                        .unwrap_or_else(|| auto.duration / 2)
                        .min(auto.duration);
                    (now.duration_since(auto.started) >= delay).then_some(auto.name())
                });
                if !self.stage_capture_done
                    && self.settings.capture_stage.as_deref() == capture_stage
                    && let (Some(id), Some(path)) =
                        (self.window, self.settings.stage_screenshot.clone())
                {
                    self.stage_capture_done = true;
                    return window::screenshot(id)
                        .map(move |screenshot| Message::StageScreenshot(path.clone(), screenshot));
                }
                if !active {
                    self.animating = false;
                    self.last_frame = None;
                    if self.auto.is_some() {
                        return self.finish();
                    }
                }
            }
            Message::SceneChanged => {}
            Message::Control(control) => {
                let mut scene = self.scene.borrow_mut();
                match control {
                    Control::Auto => {
                        drop(scene);
                        self.start_auto();
                    }
                    Control::ToggleSidebar => {
                        let target = if scene.sidebar.target > 0.0 {
                            0.0
                        } else {
                            SIDEBAR
                        };
                        LabScene::retarget(scene.reduced_motion, &mut scene.sidebar, target);
                    }
                    Control::ToggleView => {
                        let target = if scene.view == View::Reader {
                            View::Organizer
                        } else {
                            View::Reader
                        };
                        scene.switch_view(target);
                    }
                    Control::Undo => scene.undo(),
                    Control::Redo => scene.redo(),
                    Control::Delete => scene.delete_selected(),
                    Control::ZoomIn | Control::ZoomOut => {
                        let target = if control == Control::ZoomIn {
                            scene.reader_zoom.target + 0.15
                        } else {
                            scene.reader_zoom.target - 0.15
                        };
                        LabScene::retarget(
                            scene.reduced_motion,
                            &mut scene.reader_zoom,
                            target.clamp(0.25, 2.0),
                        );
                    }
                    Control::Minimize => {
                        drop(scene);
                        if let Some(id) = self.window {
                            return window::minimize::<Message>(id, true).discard();
                        }
                    }
                    Control::Maximize => {
                        drop(scene);
                        if let Some(id) = self.window {
                            return window::toggle_maximize::<Message>(id).discard();
                        }
                    }
                    Control::Close => {
                        drop(scene);
                        if let Some(id) = self.window {
                            return window::close::<Message>(id).discard();
                        }
                    }
                }
            }
            Message::IdleElapsed => {
                if std::env::var_os("MOTION_LAB_TRACE").is_some() {
                    eprintln!("motion lab: idle elapsed");
                }
                return self.after_idle();
            }
            Message::Screenshot(screenshot) => {
                if std::env::var_os("MOTION_LAB_TRACE").is_some() {
                    eprintln!("motion lab: screenshot received");
                }
                if let Some(path) = &self.settings.screenshot {
                    match save_screenshot(path, &screenshot) {
                        Ok(info) => self.screenshot_info = Some(info),
                        Err(error) => self.errors.push(format!("screenshot: {error}")),
                    }
                }
                self.write_report();
                if self.settings.exit_on_complete
                    && let Some(id) = self.window
                {
                    return window::close::<Message>(id).discard();
                }
            }
            Message::StageScreenshot(path, screenshot) => {
                match save_screenshot(&path, &screenshot) {
                    Ok((width, height, scale)) => {
                        if std::env::var_os("MOTION_LAB_TRACE").is_some() {
                            eprintln!(
                                "motion lab: stage screenshot {width}x{height} scale {scale}"
                            );
                        }
                    }
                    Err(error) => self.errors.push(format!("stage screenshot: {error}")),
                }
            }
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Message> {
        self.view_builds.set(self.view_builds.get() + 1);
        let scene = self.scene.clone();
        let redraws = self.redraws.clone();
        container(
            Canvas::new(LabCanvas { scene, redraws })
                .width(Length::Fill)
                .height(Length::Fill),
        )
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_theme| container::Style {
            background: Some(CANVAS.into()),
            ..container::Style::default()
        })
        .into()
    }

    fn subscription(&self) -> iced::Subscription<Message> {
        let mut subscriptions =
            vec![window::events().map(|(_, event)| Message::WindowEvent(event))];
        if !self.completion_started && (self.auto.is_some() || self.scene.borrow().is_active()) {
            subscriptions.push(window::frames().map(Message::Frame));
        }
        iced::Subscription::batch(subscriptions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene() -> LabScene {
        LabScene::new(120, false, false, Size::new(1360.0, 870.0))
    }

    #[test]
    fn initial_reader_selection_is_visible() {
        let scene = LabScene::new(200, false, false, Size::new(1360.0, 870.0));
        let rect = scene.reader_rect(scene.selected);
        assert!(rect.intersects(&scene.content_bounds()));
        assert!(scene.flip.is_none());
    }

    #[test]
    fn compact_breakpoint_hides_first_group() {
        let full = controls(Size::new(1280.0, 870.0), false);
        let small = controls(Size::new(1279.0, 870.0), false);
        assert!(full.iter().any(|(control, _, _)| *control == Control::Auto));
        assert!(
            !small
                .iter()
                .any(|(control, _, _)| *control == Control::Auto)
        );
        assert!(
            small
                .iter()
                .any(|(control, _, _)| *control == Control::ToggleView)
        );
    }

    #[test]
    fn grid_virtualization_is_bounded() {
        let scene = scene();
        let visible = scene.visible_slots().len();
        assert!(visible < 100);
        assert!(visible < scene.pages.len());
        assert_eq!(scene.invalid_states, 0);
    }

    #[test]
    fn sparse_positions_do_not_scale_with_page_count() {
        let mut small = LabScene::new(2000, false, false, Size::new(1360.0, 870.0));
        let mut large = LabScene::new(4000, false, false, Size::new(1360.0, 870.0));
        small.switch_view(View::Organizer);
        large.switch_view(View::Organizer);
        assert_eq!(small.positions.len(), large.positions.len());
        assert!(large.positions.len() < 100);
        assert!(large.positions.len() <= large.visible_slots().len() + 4);
    }

    #[test]
    fn reader_order_follows_drag_reorder() {
        let mut scene = LabScene::new(24, false, true, Size::new(1360.0, 870.0));
        scene.switch_view(View::Organizer);
        let page = scene.selected;
        let from = scene.slots[&page];
        let geometry = scene.grid_geometry();
        let rect = scene.grid_rect(page, geometry);
        let start = Point::new(rect.x + 10.0, rect.y + 10.0);
        scene.begin_drag(page, start, rect);
        scene.drag_to(Point::new(start.x + geometry.pitch_x * 2.0, start.y));
        scene.finish_drag();
        let to = scene.slots[&page];
        assert!(to > from);
        assert_eq!(scene.order[to], page);
        scene.switch_view(View::Reader);
        assert_eq!(scene.order[scene.slots[&page]], page);
        let (_, _, pitch, _) = scene.reader_metrics();
        let expected_y = scene.content_bounds().y + to as f32 * pitch - scene.reader_scroll.value;
        assert!((scene.reader_rect(page).y - expected_y).abs() < 0.001);
    }

    #[test]
    fn drag_threshold_and_single_history_entry() {
        let mut scene = scene();
        scene.switch_view(View::Organizer);
        scene.layout(scene.window);
        let geometry = scene.grid_geometry();
        let rect = scene.grid_rect(0, geometry);
        let start = Point::new(rect.x + 10.0, rect.y + 10.0);
        scene.begin_drag(0, start, rect);
        scene.drag_to(start + Vector::new(6.0, 0.0));
        assert!(!scene.drag.as_ref().expect("drag").active);
        scene.drag_to(start + Vector::new(8.0, 0.0));
        assert!(scene.drag.as_ref().expect("drag").active);
        scene.drag_to(Point::new(rect.x + geometry.pitch_x * 2.0, rect.y + 10.0));
        scene.finish_drag();
        assert_eq!(scene.history.len(), 1);
        assert_eq!(scene.invalid_states, 0);
    }

    #[test]
    fn flip_retarget_keeps_page_identity() {
        let mut scene = scene();
        let page = scene.selected;
        scene.switch_view(View::Organizer);
        assert_eq!(scene.flip.as_ref().expect("flip").page, page);
        scene.switch_view(View::Reader);
        assert_eq!(scene.view, View::Reader);
        assert_eq!(scene.page(page).id, page);
    }

    #[test]
    fn flip_targets_visible_grid_rect_after_auto_scroll() {
        let mut scene = LabScene::new(200, false, false, Size::new(1360.0, 870.0));
        scene.switch_view(View::Organizer);
        let flip = scene.flip.as_ref().expect("flip");
        let geometry = scene.grid_geometry();
        let slot = scene.slots[&scene.selected];
        let mut expected = scene.grid_target(slot, geometry);
        expected.y -= scene.grid_scroll.target;
        assert!((flip.to.x - expected.x).abs() < 0.001);
        assert!((flip.to.y - expected.y).abs() < 0.001);
        assert!(scene.grid_scroll.target > 0.0);
    }

    #[test]
    fn auto_scenario_reaches_idle() {
        let mut scene = LabScene::new(24, false, true, Size::new(1360.0, 870.0));
        let mut auto = AutoState::new(true);
        auto.duration = Duration::ZERO;
        for _ in 0..STAGE_NAMES.len() + 4 {
            if !auto.tick(&mut scene, Instant::now(), 1.0 / 60.0) {
                assert!(!scene.is_active());
                return;
            }
        }
        panic!("auto scenario did not reach idle");
    }

    #[test]
    fn auto_sidebar_retargets_midflight() {
        let mut scene = LabScene::new(120, false, false, Size::new(1360.0, 870.0));
        let mut auto = AutoState::new(false);
        auto.duration = Duration::from_millis(200);
        let now = Instant::now();
        auto.tick(&mut scene, now, 1.0 / 60.0);
        auto.stage = 1;
        auto.started = now;
        auto.entered = false;
        for frame in 1..=5 {
            auto.tick(
                &mut scene,
                now + Duration::from_millis(frame * 16),
                1.0 / 60.0,
            );
        }
        assert_eq!(scene.sidebar.target, SIDEBAR);
        assert!(scene.sidebar.velocity > 0.0);
        auto.tick(&mut scene, now + Duration::from_millis(110), 1.0 / 60.0);
        assert!(auto.midpoint_retargeted);
        assert_eq!(scene.sidebar.target, 0.0);
        assert!(scene.sidebar.is_active());
    }

    #[test]
    fn reduced_motion_settles_immediately() {
        let mut scene = LabScene::new(120, false, true, Size::new(1360.0, 870.0));
        LabScene::retarget(scene.reduced_motion, &mut scene.sidebar, 0.0);
        scene.switch_view(View::Organizer);
        assert_eq!(scene.sidebar.value, 0.0);
        assert!(!scene.sidebar.is_active());
        assert!(scene.flip.is_none());
    }
}

fn main() -> iced::Result {
    let settings = match RunSettings::parse() {
        Ok(Some(settings)) => settings,
        Ok(None) => return Ok(()),
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    let window = settings.window.clone();
    iced::application(
        move || MotionApp::new(settings.clone()),
        MotionApp::update,
        MotionApp::view,
    )
    .subscription(MotionApp::subscription)
    .title(|_: &MotionApp| "Folio Flow · Motion Lab".to_string())
    .window(window)
    .run()
}
