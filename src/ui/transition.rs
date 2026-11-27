//! Match pages by stable document identity while switching reading layouts and the grid.
use super::{theme, viewer::PageSnapshot};
use crate::app::{App, Message, ViewMode};
use crate::motion::Tween;
use iced::advanced::image;
use iced::widget::canvas::{self, Frame, Geometry};
use iced::{Color, Element, Fill, Point, Rectangle, Vector, mouse};
use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

pub(crate) struct Transition {
    from: HashMap<u32, Rectangle>,
    images: HashMap<u32, PageSnapshot>,
    from_opacity: HashMap<u32, f32>,
    pub progress: Tween,
}

pub(crate) fn origin(app: &App) -> Vector {
    if app.immersive {
        return Vector::ZERO;
    }
    match app.view_mode {
        ViewMode::Reader => {
            let padding = theme::reader_insets(app.window_size.width, app.motion.sidebar.value);
            Vector::new(padding.left, padding.top)
        }
        ViewMode::Organizer => Vector::new(0.0, theme::content_top(app.window_size.width)),
    }
}

fn target(app: &App, source: u32) -> Option<Rectangle> {
    let slot = app.viewer_order.slot_of(source)?;
    let rect = match app.view_mode {
        ViewMode::Reader => app.viewer.page_rect(slot),
        ViewMode::Organizer => app.organizer.transition_rect(
            app.order.slot_of(source)?,
            app.viewer.document()?.pages[slot as usize],
        ),
    };
    Some(rect + origin(app))
}

fn visible(app: &App) -> Vec<u32> {
    match app.view_mode {
        ViewMode::Reader => app
            .viewer
            .visible_pages()
            .map(|slot| app.viewer_order.source_at(slot))
            .collect(),
        ViewMode::Organizer => app
            .organizer
            .wanted_pages(app.viewer.page_count(), &app.order, &app.viewer_order)
            .into_iter()
            .map(|slot| app.viewer_order.source_at(slot))
            .collect(),
    }
}

fn mix(from: Rectangle, to: Rectangle, t: f32) -> Rectangle {
    Rectangle {
        x: from.x + (to.x - from.x) * t,
        y: from.y + (to.y - from.y) * t,
        width: from.width + (to.width - from.width) * t,
        height: from.height + (to.height - from.height) * t,
    }
}

impl Transition {
    pub fn capture(app: &App) -> Option<Self> {
        app.viewer.document()?;
        if app.viewer_order.as_sources().is_empty() {
            return None;
        }
        let viewport = Rectangle::new(Point::ORIGIN, app.window_size);
        let mut from_opacity = HashMap::new();
        let mut from = HashMap::new();
        for source in app.viewer_order.as_sources() {
            if let Some(rect) = target(app, *source) {
                from.insert(*source, rect);
                from_opacity.insert(*source, if rect.intersects(&viewport) { 1.0 } else { 0.0 });
            }
        }
        let mut images = HashMap::new();
        for source in visible(app) {
            if let Some(snapshot) = app
                .viewer_order
                .slot_of(source)
                .and_then(|slot| app.viewer.snapshot(slot))
            {
                images.insert(source, snapshot);
            }
        }
        // Continue from the last displayed geometry when interrupted, rather than restarting at an endpoint.
        if let Some(previous) = &app.view_transition {
            for (source, old) in &previous.from {
                if let Some(to) = target(app, *source) {
                    from.insert(*source, mix(*old, to, previous.progress.value));
                    from_opacity.insert(*source, previous.opacity(*source, to, viewport));
                }
            }
            for (source, snapshot) in &previous.images {
                images.insert(*source, snapshot.clone());
            }
        }
        let mut progress = Tween::new(0.0);
        progress.start(1.0, Duration::from_millis(460));
        Some(Self {
            from,
            images,
            from_opacity,
            progress,
        })
    }

    pub fn advance(&mut self, now: Instant) -> bool {
        self.progress.advance(now)
    }

    pub fn sources(&self) -> impl Iterator<Item = u32> + '_ {
        self.images.keys().copied()
    }

    fn opacity(&self, source: u32, to: Rectangle, viewport: Rectangle) -> f32 {
        let from = self.from_opacity.get(&source).copied().unwrap_or(0.0);
        let target = if to.intersects(&viewport) { 1.0 } else { 0.0 };
        from + (target - from) * self.progress.value
    }

    pub(crate) fn rect(&self, source: u32, to: Rectangle) -> Rectangle {
        mix(
            self.from.get(&source).copied().unwrap_or(to),
            to,
            self.progress.value,
        )
    }
}

pub(crate) fn view<'a>(app: &'a App, transition: &'a Transition) -> Element<'a, Message> {
    canvas::Canvas::new(Overlay { app, transition })
        .width(Fill)
        .height(Fill)
        .into()
}

struct Overlay<'a> {
    app: &'a App,
    transition: &'a Transition,
}

impl canvas::Program<Message> for Overlay<'_> {
    type State = ();
    fn draw(
        &self,
        _: &(),
        renderer: &iced::Renderer,
        _: &iced::Theme,
        bounds: Rectangle,
        _: mouse::Cursor,
    ) -> Vec<Geometry<iced::Renderer>> {
        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), self.app.palette.canvas);
        let viewport = Rectangle::new(Point::ORIGIN, bounds.size());
        let sources: BTreeSet<_> = self
            .transition
            .images
            .keys()
            .copied()
            .chain(visible(self.app))
            .collect();
        for source in sources {
            let Some(to) = target(self.app, source) else {
                continue;
            };
            let rect = self.transition.rect(source, to);
            if !rect.intersects(&viewport) {
                continue;
            }
            let opacity = self.transition.opacity(source, to, viewport);
            let current = if self.transition.images.contains_key(&source) {
                None
            } else {
                self.app
                    .viewer_order
                    .slot_of(source)
                    .and_then(|slot| self.app.viewer.snapshot(slot))
            };
            let snapshot = self.transition.images.get(&source).or(current.as_ref());
            frame.fill_rectangle(
                rect.position(),
                rect.size(),
                Color {
                    a: opacity,
                    ..Color::WHITE
                },
            );
            let thumbnail = self
                .app
                .viewer_order
                .slot_of(source)
                .and_then(|slot| self.app.viewer.thumbnail(slot))
                .or_else(|| snapshot.and_then(|snapshot| snapshot.thumbnail.as_ref()));
            if let Some(thumbnail) = thumbnail {
                frame.draw_image(rect, image::Image::new(thumbnail.clone()).opacity(opacity));
            }
            if let Some(snapshot) = snapshot {
                frame.with_save(|frame| {
                    frame.translate(Vector::new(rect.x, rect.y));
                    frame.scale_nonuniform(Vector::new(
                        rect.width / snapshot.rect.width.max(0.001),
                        rect.height / snapshot.rect.height.max(0.001),
                    ));
                    frame.translate(Vector::new(-snapshot.rect.x, -snapshot.rect.y));
                    for (tile, handle) in &snapshot.tiles {
                        frame.draw_image(*tile, image::Image::new(handle.clone()).opacity(opacity));
                    }
                });
            }
        }
        vec![frame.into_geometry()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Options;
    use crate::engine::{DocInfo, PageSize};
    use std::{path::PathBuf, sync::Arc};

    fn app() -> App {
        let (mut app, _) = App::boot(Options {
            sidebar: Some("hidden".into()),
            ..Options::default()
        });
        app.viewer.set_document(Arc::new(DocInfo {
            id: 1,
            path: PathBuf::new(),
            title: String::new(),
            pages: vec![
                PageSize {
                    width: 612.0,
                    height: 792.0
                };
                40
            ],
            outline: Vec::new(),
        }));
        app.order.reset(40);
        app.viewer_order.reset(40);
        app.organizer.set_document(40);
        app
    }

    fn switch(app: &mut App, mode: u8) {
        let message = match mode {
            0 => Message::SetReadingLayout(false),
            1 => Message::SetReadingLayout(true),
            _ => Message::SetView(ViewMode::Organizer),
        };
        let _ = app.update(message);
    }

    #[test]
    fn all_six_directions_move_the_same_page_to_its_target() {
        for from in 0..3 {
            for to in 0..3 {
                if from == to {
                    continue;
                }
                let mut app = app();
                switch(&mut app, from);
                app.view_transition = None;
                let source = 0;
                let old = target(&app, source).unwrap();
                switch(&mut app, to);
                let destination = target(&app, source).unwrap();
                let transition = app.view_transition.as_mut().unwrap();
                assert_eq!(transition.rect(source, destination), old);
                assert_ne!(old, destination);
                let start = transition.progress.started;
                transition.advance(start + Duration::from_millis(230));
                let middle = transition.rect(source, destination);
                assert_eq!(middle, mix(old, destination, 0.5));
                let _ = app.update(Message::Animate(start + Duration::from_millis(500)));
                assert!(app.view_transition.is_none());
            }
        }
    }

    #[test]
    fn interrupted_transition_preserves_position_and_opacity() {
        let mut app = app();
        switch(&mut app, 1);
        let now =
            app.view_transition.as_ref().unwrap().progress.started + Duration::from_millis(190);
        app.view_transition.as_mut().unwrap().advance(now);
        let destination = target(&app, 0).unwrap();
        let old = app.view_transition.as_ref().unwrap().rect(0, destination);
        let viewport = Rectangle::new(Point::ORIGIN, app.window_size);
        let alpha = app
            .view_transition
            .as_ref()
            .unwrap()
            .opacity(0, destination, viewport);
        switch(&mut app, 2);
        let transition = app.view_transition.as_ref().unwrap();
        let destination = target(&app, 0).unwrap();
        assert_eq!(transition.rect(0, destination), old);
        assert_eq!(transition.opacity(0, destination, viewport), alpha);
    }

    #[test]
    fn organizer_selection_and_reordered_identity_follow_the_document_page() {
        let mut app = app();
        app.order.move_slot(0, 3);
        switch(&mut app, 2);
        let grid = target(&app, 0).unwrap();
        let expected = app
            .organizer
            .transition_rect(3, app.viewer.document().unwrap().pages[0])
            + origin(&app);
        assert_eq!(grid, expected);
        app.organizer
            .select(12, iced::keyboard::Modifiers::default(), &app.order);
        switch(&mut app, 1);
        assert_eq!(app.viewer.current_page(), 12);
    }

    #[test]
    fn no_op_and_direct_manipulation_do_not_leave_an_overlay_running() {
        let mut app = app();
        switch(&mut app, 0);
        assert!(app.view_transition.is_none());
        switch(&mut app, 1);
        assert!(app.view_transition.is_some());
        let _ = app.update(Message::Viewer(
            super::super::viewer::Interaction::Scrolled { dx: 0.0, dy: 56.0 },
        ));
        assert!(app.view_transition.is_none());
        switch(&mut app, 2);
        let _ = app.update(Message::WindowSize(iced::Size::new(1100.0, 800.0)));
        assert!(app.view_transition.is_none());
    }
}
