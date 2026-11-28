//! Application state and the message loop.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use iced::advanced::image as iced_image;
use iced::futures::StreamExt;
use iced::futures::channel::{mpsc, oneshot};
use iced::keyboard::{self, Key, key::Named};
use iced::widget::canvas;
use iced::widget::scrollable;
use iced::window;
use iced::{Element, Point, Rectangle, Size, Subscription, Task, Vector};

use crate::engine::{self, DocInfo, Engine, JobKind, Target, print};
use crate::motion::{Motion, Tween};
use crate::trace;
use crate::ui::order::{EditHistory, EditState, PageOrder};
use crate::ui::theme::{self, Palette};
use crate::ui::viewer::{Fit, Interaction, PageSnapshot, Viewer};
use crate::ui::{self, dialog, file_drop, icon, organizer, sidebar};
use crate::web;

/// Options taken from the command line.
#[derive(Debug, Clone, Default)]
pub struct Options {
    pub file: Option<PathBuf>,
    pub light: bool,
    /// Development aid: once the first view has rendered, save it as a PNG
    /// and quit.
    pub screenshot: Option<PathBuf>,
    pub page: Option<u32>,
    pub zoom: Option<f32>,
    /// `outline`, `pages` or `hidden`.
    pub sidebar: Option<String>,
    /// Development aid: start with the whole document selected.
    pub select_all: bool,
    /// Show the print dialog as soon as the document has loaded.
    pub print: bool,
    /// Development aid: start with the find bar open on this query.
    pub find: Option<String>,
    /// Development aid: once the first view has rendered, scroll down for
    /// this many frames, report how they went to the trace, and quit.
    pub scroll_test: Option<u32>,
    /// Device pixels per logical pixel on the display the window is likely
    /// to open on, when that can be told before there is a window.
    pub scale_factor: Option<f32>,
    /// Development aid: start in the thumbnail organizer.
    pub organizer: bool,
    /// Development aid: open at a specific logical window size.
    pub window_size: Option<Size>,
}

/// Carries a value that cannot be cloned through a message, which must be.
pub struct Once<T>(Arc<Mutex<Option<T>>>);

impl<T> Once<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(Mutex::new(Some(value))))
    }

    pub fn take(&self) -> Option<T> {
        self.0.lock().ok()?.take()
    }
}

impl<T> Clone for Once<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> std::fmt::Debug for Once<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Once")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Reader,
    Organizer,
}

#[derive(Debug, Clone, Copy)]
pub enum WindowAction {
    Minimize,
    Maximize,
    Close,
}

#[derive(Debug, Clone)]
enum LeaveAction {
    Open(PathBuf),
    Close,
}

#[derive(Debug, Clone, Copy)]
pub enum LeaveChoice {
    Save,
    Discard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolbarControl {
    Open,
    Undo,
    Redo,
    Sidebar,
    Reader,
    DoublePage,
    Organizer,
    Zoom,
    FitWidth,
    FitPage,
    Grid,
    RotateLeft,
    RotateRight,
    Extract,
    Delete,
    Search,
    Print,
    Compress,
    Export,
    Theme,
    Minimize,
    Maximize,
    Close,
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The engine has events waiting.
    Engine,
    Viewer(Interaction),
    SetView(ViewMode),
    SetReadingLayout(bool),
    ToggleImmersive,
    OrganizerFrame {
        size: Size,
        bounds: Rectangle,
        animating: bool,
    },
    OrganizerScrolled(f32),
    OrganizerScrolledTo(f32),
    OrganizerClicked {
        slot: u32,
        modifiers: keyboard::Modifiers,
        double_click: bool,
    },
    OrganizerDrag {
        source: u32,
        slot: u32,
        scroll: f32,
        commit: bool,
    },
    SidebarPagesFrame {
        size: Size,
        bounds: Rectangle,
        animating: bool,
    },
    SidebarPageClicked {
        source: u32,
    },
    SidebarPageDrag {
        source: u32,
        slot: u32,
        scroll: f32,
        commit: bool,
    },
    SidebarPagesScrolled(f32),
    SidebarWheel(f32),
    SidebarStopWheel,
    GridSize(f32),
    DragWindow,
    WindowSize(Size),
    WindowAction(WindowAction),
    WindowMaximized(bool),
    OpenDialog,
    Open(Option<PathBuf>),
    ToggleSidebar,
    ShowTab(sidebar::Tab),
    ToggleOutline(usize),
    GoTo(u32),
    SidebarScrolled(scrollable::Viewport),
    PageInput(String),
    PageSubmit,
    ZoomTo(f32),
    ZoomReset,
    Fit(Fit),
    Undo,
    Redo,
    Rotate(i8),
    DeletePages,
    Extract,
    ExtractTo {
        path: Option<PathBuf>,
        pages: Vec<u32>,
    },
    Save,
    SaveAs,
    SaveTo(Option<PathBuf>),
    Compact,
    DialogClose,
    LeaveChoice(LeaveChoice),
    DialogIgnore,
    DialogConfirm,
    DialogFormat(dialog::ExportFormat),
    DialogScale(f32),
    DialogImagePdf(bool),
    DialogDpi(f32),
    DialogQuality(u8),
    DialogDestination {
        doc: u64,
        serial: u64,
        operation: dialog::Operation,
        path: Option<PathBuf>,
    },
    ToggleMore,
    Export,
    Print,
    PrintOwner(isize),
    PrintTarget(Once<Result<Option<print::Target>, String>>),
    ToggleTheme,
    ShowSettings,
    ShowAbout,
    ToggleRemember,
    ClearReading,
    OpenProject(bool),
    DefaultPdf,
    DefaultPdfResult(Result<(), String>),
    Find,
    FindInput(String),
    /// Enter in the find field.
    FindSubmit,
    FindStep(i8),
    Escape,
    Key(keyboard::Event),
    FileDropped(PathBuf),
    FileHovered(PathBuf),
    FileHoverLeft,
    ExternalPoll,
    ExternalPosition(Option<Point>),
    DropAt {
        path: PathBuf,
        point: Option<Point>,
    },
    ScaleFactor(f32),
    ToolbarHover {
        control: ToolbarControl,
        hovered: bool,
    },
    Tick(Instant),
    Animate(Instant),
    /// A frame of the scroll test.
    Frame(Instant),
    DismissToast,
    Screenshot(Once<window::Screenshot>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Error,
}

#[derive(Debug, Clone)]
pub struct Toast {
    pub text: String,
    pub tone: Tone,
    /// `None` while the work it reports on is still running.
    pub expires: Option<Instant>,
    pub progress: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditKind {
    Rotate,
    Delete,
}

pub(crate) struct PageEditAnimation {
    pub from_slot: u32,
    pub to_slot: Option<u32>,
    pub turns: i8,
    pub snapshot: PageSnapshot,
}

pub(crate) struct EditTransform {
    pub center: Point,
    pub angle: f32,
    pub scale: f32,
    pub opacity: f32,
}

pub(crate) struct EditAnimation {
    pub kind: EditKind,
    pub pages: Vec<PageEditAnimation>,
    pub sidebar_offset: f32,
    pub sidebar_tops: Vec<f32>,
    reader_rects: Vec<Rectangle>,
    old_page_count: u32,
    progress: Tween,
}

impl EditAnimation {
    fn new(
        kind: EditKind,
        pages: Vec<PageEditAnimation>,
        old_page_count: u32,
        sidebar_offset: f32,
        sidebar_tops: Vec<f32>,
        reader_rects: Vec<Rectangle>,
    ) -> Self {
        Self {
            kind,
            pages,
            sidebar_offset,
            sidebar_tops,
            reader_rects,
            old_page_count,
            progress: Tween::new(0.0),
        }
    }

    fn restart(&mut self) {
        let duration = match self.kind {
            EditKind::Rotate => Duration::from_millis(360),
            EditKind::Delete => Duration::from_millis(240),
        };
        self.progress.start(1.0, duration);
    }

    fn advance(&mut self, now: Instant) {
        self.progress.advance(now);
    }

    fn is_active(&self, now: Instant) -> bool {
        self.progress.value < self.progress.to || self.progress.is_active(now)
    }

    pub(crate) fn rotation(&self, slot: u32) -> Option<&PageEditAnimation> {
        (self.kind == EditKind::Rotate)
            .then(|| self.pages.iter().find(|page| page.to_slot == Some(slot)))
            .flatten()
    }

    pub(crate) fn previous_slot(&self, slot: u32) -> Option<u32> {
        if self.kind != EditKind::Delete {
            return None;
        }

        let mut previous = slot;
        for page in &self.pages {
            if page.from_slot <= previous {
                previous += 1;
            }
        }
        (previous < self.old_page_count).then_some(previous)
    }

    pub(crate) fn reader_rect(&self, slot: u32, target: Rectangle) -> Rectangle {
        let previous = match self.kind {
            EditKind::Rotate => slot,
            EditKind::Delete => self.previous_slot(slot).unwrap_or(slot),
        };
        let Some(old) = self.reader_rects.get(previous as usize) else {
            return target;
        };
        let progress = self.progress.value;

        Rectangle {
            x: old.x + (target.x - old.x) * progress,
            y: old.y + (target.y - old.y) * progress,
            width: old.width + (target.width - old.width) * progress,
            height: old.height + (target.height - old.height) * progress,
        }
    }

    pub(crate) fn transform(&self, page: &PageEditAnimation, target: Rectangle) -> EditTransform {
        let progress = self.progress.value;
        let old = page.snapshot.rect;
        let center = Point::new(
            old.center_x() + (target.center_x() - old.center_x()) * progress,
            old.center_y() + (target.center_y() - old.center_y()) * progress,
        );

        let (angle, scale, opacity) = match self.kind {
            EditKind::Rotate => {
                let angle = page.turns as f32 * std::f32::consts::FRAC_PI_2 * progress;
                let sin = angle.sin().abs();
                let cos = angle.cos().abs();
                let width = old.width * cos + old.height * sin;
                let height = old.width * sin + old.height * cos;
                let fit = (target.width / width.max(0.001)).min(target.height / height.max(0.001));
                (
                    angle,
                    1.0 + (fit - 1.0) * progress,
                    1.0 - 0.06 * (progress * std::f32::consts::PI).sin(),
                )
            }
            EditKind::Delete => (0.0, 1.0 - 0.08 * progress, 1.0 - progress),
        };

        EditTransform {
            center,
            angle,
            scale,
            opacity,
        }
    }

    pub(crate) fn draw_thumbnail(
        &self,
        frame: &mut canvas::Frame,
        page: &PageEditAnimation,
        target: Rectangle,
    ) {
        let Some(handle) = &page.snapshot.thumbnail else {
            return;
        };
        let transform = self.transform(page, target);
        let old = page.snapshot.rect;

        frame.with_save(|frame| {
            frame.translate(Vector::new(transform.center.x, transform.center.y));
            frame.rotate(iced::Radians(transform.angle));
            frame.scale(transform.scale);
            frame.translate(Vector::new(-old.center_x(), -old.center_y()));
            frame.draw_image(
                old,
                iced_image::Image::new(handle.clone()).opacity(transform.opacity),
            );
        });
    }
}

impl PageEditAnimation {
    pub(crate) fn has_image(&self) -> bool {
        !self.snapshot.tiles.is_empty() || self.snapshot.thumbnail.is_some()
    }
}

/// The find bar.
#[derive(Debug, Default)]
pub struct Find {
    pub open: bool,
    pub query: String,
    /// Identifies the latest search, so results of earlier ones are dropped.
    id: u64,
    pub searching: bool,
    /// The search stopped at the engine's limit: there are more matches.
    pub capped: bool,
}

impl Find {
    /// Identifies the find field for focus operations.
    pub const FIELD: &'static str = "find";
}

pub struct App {
    engine: Engine,
    events: std::sync::mpsc::Receiver<engine::Event>,
    /// Set by the engine when it adds to `events`; while it is set, a
    /// [`Message::Engine`] is on its way.
    pending: Arc<AtomicBool>,
    pub viewer: Viewer,
    pub view_mode: ViewMode,
    pub(crate) window_size: Size,
    pub(crate) view_transition: Option<ui::transition::Transition>,
    pub order: PageOrder,
    pub edits: EditHistory,
    /// Physical source order currently loaded in the viewer.
    pub(crate) viewer_order: PageOrder,
    pub organizer: organizer::State,
    pub sidebar: sidebar::State,
    reader_sidebar_open: bool,
    pub palette: Palette,
    pub motion: Motion,
    pub immersive: bool,
    pub toolbar_hover_control: Option<ToolbarControl>,
    pub toolbar_hover_amount: Tween,
    pub window_maximized: bool,
    pub page_input: String,
    pub find: Find,
    pub toast: Option<Toast>,
    pub opening: Option<PathBuf>,
    pub more_open: bool,
    pub remember_reading: bool,
    reading_store: crate::reading::Store,
    reading_path: Option<PathBuf>,
    reading_suppressed: bool,
    pub external_drop: file_drop::State,
    pub dialog: Option<dialog::State>,
    pub dialog_motion: Tween,
    dialog_serial: u64,
    estimate_revision: u64,
    document_busy: bool,
    leave_action: Option<LeaveAction>,
    leave_after_save: bool,
    pending_edit_previous: Option<EditHistory>,
    pending_edit_animation: Option<EditAnimation>,
    pending_edit_focus: Option<u32>,
    pending_edit_selection: Vec<u32>,
    pub(crate) edit_animation: Option<EditAnimation>,
    sidebar_animating: bool,
    organizer_animating: bool,
    last_wanted: Option<(u64, engine::Wanted)>,
    modifiers: keyboard::Modifiers,
    options: Options,
    /// Frames to wait before taking the requested screenshot.
    screenshot_countdown: Option<u8>,
    scroll_test: Option<ScrollTest>,
    /// When the scroll test finished scrolling.
    resting_since: Option<Instant>,
}

/// The size the window opens at, in logical pixels.
const WINDOW: Size = Size::new(1360.0, 870.0);

/// Duration of the subtle toolbar hover response.
const TOOLBAR_HOVER_DURATION: Duration = Duration::from_millis(120);

/// Distance the scroll test moves per frame, in logical pixels.
const SCROLL_TEST_STEP: f32 = 30.0;

/// How long the scroll test waits after scrolling before it reports the
/// memory in use at rest.
const SCROLL_TEST_REST: Duration = Duration::from_secs(5);

/// A scroll test in progress.
struct ScrollTest {
    last: Instant,
    /// Milliseconds between frames.
    intervals: Vec<f32>,
    /// Whether tiles in view are missing right now.
    missing: bool,
    /// Frames drawn while they were.
    incomplete: u32,
}

impl App {
    pub fn boot(options: Options) -> (Self, Task<Message>) {
        // Events are queued here and taken in a batch; the stream only
        // carries the news that there are some. A burst of tiles is then one
        // update of the interface rather than one each.
        let (sender, events) = std::sync::mpsc::channel();
        let (waker, woken) = mpsc::unbounded();
        let pending = Arc::new(AtomicBool::new(false));

        let engine = Engine::spawn({
            let pending = pending.clone();

            move |event| {
                let _ = sender.send(event);

                if !pending.swap(true, Ordering::AcqRel) {
                    let _ = waker.unbounded_send(());
                }
            }
        });

        let mut app = Self {
            engine,
            events,
            pending,
            viewer: Viewer::default(),
            view_mode: ViewMode::Reader,
            window_size: options.window_size.unwrap_or(WINDOW),
            view_transition: None,
            order: PageOrder::default(),
            edits: EditHistory::default(),
            viewer_order: PageOrder::default(),
            organizer: organizer::State::default(),
            sidebar: sidebar::State::default(),
            reader_sidebar_open: true,
            palette: if options.light || crate::preferences::load_light() {
                Palette::LIGHT
            } else {
                Palette::DARK
            },
            motion: Motion::default(),
            immersive: false,
            toolbar_hover_control: None,
            toolbar_hover_amount: Tween::new(0.0),
            window_maximized: false,
            page_input: String::new(),
            find: Find::default(),
            toast: None,
            opening: None,
            more_open: false,
            remember_reading: !cfg!(test) && crate::preferences::load_remember(),
            reading_store: if cfg!(test) {
                crate::reading::Store::default()
            } else {
                crate::preferences::directory()
                    .map(|p| crate::reading::Store::load(&p.join("reading.json")))
                    .unwrap_or_default()
            },
            reading_path: if cfg!(test) {
                None
            } else {
                crate::preferences::directory().map(|p| p.join("reading.json"))
            },
            reading_suppressed: false,
            external_drop: file_drop::State::default(),
            dialog: None,
            dialog_motion: Tween::new(0.0),
            dialog_serial: 0,
            estimate_revision: 0,
            document_busy: false,
            leave_action: None,
            leave_after_save: false,
            pending_edit_previous: None,
            pending_edit_animation: None,
            pending_edit_focus: None,
            pending_edit_selection: Vec::new(),
            edit_animation: None,
            sidebar_animating: false,
            organizer_animating: false,
            last_wanted: None,
            modifiers: keyboard::Modifiers::default(),
            options,
            screenshot_countdown: None,
            scroll_test: None,
            resting_since: None,
        };

        trace::mark("boot");

        // The window does not exist yet, but where the page view will be in
        // it is known. Assuming that much lets the engine render the first
        // screen while the graphics device is still starting; if the guess
        // is off, the view corrects it when it is first laid out.
        if let Some(scale_factor) = app.options.scale_factor {
            app.viewer.set_scale_factor(scale_factor);
            icon::set_scale_factor(scale_factor);
        }

        app.sidebar.open = !matches!(app.options.sidebar.as_deref(), Some("hidden"));
        app.reader_sidebar_open = app.sidebar.open;
        app.motion.sidebar = crate::motion::Spring::new(if app.sidebar.open { 1.0 } else { 0.0 });

        let window_size = app.options.window_size.unwrap_or(WINDOW);
        let view = ui::page_view_size(window_size, app.motion.sidebar.value);

        app.viewer.set_viewport(view);
        app.sidebar.height =
            (window_size.height - theme::content_top(window_size.width) - theme::SIDEBAR_BOTTOM)
                .max(0.0)
                - sidebar::HEADER;
        app.view_mode = if app.options.organizer {
            ViewMode::Organizer
        } else {
            ViewMode::Reader
        };
        if app.view_mode == ViewMode::Organizer {
            app.sidebar.open = false;
            app.motion.sidebar = crate::motion::Spring::new(0.0);
        }

        if let Some(path) = app.options.file.clone() {
            app.open(path);
        } else if app.options.screenshot.is_some() {
            app.screenshot_countdown = Some(3);
        }

        let events = Task::stream(woken.map(|()| Message::Engine));
        let scale = window::oldest()
            .and_then(window::scale_factor)
            .map(Message::ScaleFactor);
        let maximized = window::oldest()
            .and_then(window::is_maximized)
            .map(Message::WindowMaximized);

        (app, Task::batch([events, scale, maximized]))
    }

    pub fn title(&self) -> String {
        match self.viewer.document() {
            Some(doc) if !doc.title.is_empty() => format!(
                "{}{} — Folio Flow",
                if self.edits.dirty() { "● " } else { "" },
                doc.title
            ),
            _ => "Folio Flow".into(),
        }
    }

    pub fn theme(&self) -> iced::Theme {
        self.palette.iced()
    }

    pub fn toolbar_hover(&self, control: ToolbarControl) -> f32 {
        if self.toolbar_hover_control == Some(control) {
            self.toolbar_hover_amount.value
        } else {
            0.0
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subscriptions = vec![
            keyboard::listen().map(Message::Key),
            iced::event::listen_with(escape),
            iced::event::listen_with(window_event),
        ];

        if self.toast.is_some()
            || self.screenshot_countdown.is_some()
            || self.resting_since.is_some()
        {
            subscriptions.push(iced::time::every(Duration::from_millis(200)).map(Message::Tick));
        }

        if self.external_drop.hovering {
            subscriptions
                .push(iced::time::every(Duration::from_millis(16)).map(|_| Message::ExternalPoll));
            #[cfg(not(windows))]
            subscriptions.push(iced::event::listen_with(|event, _, _| match event {
                iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
                    Some(Message::ExternalPosition(Some(position)))
                }
                _ => None,
            }));
        }
        let now = Instant::now();
        if self.dialog_motion.is_active(now)
            || self.motion.is_active(now)
            || self.viewer.scrolling()
            || self.sidebar.scrolling()
            || self.organizer.scrolling()
            || self.view_transition.is_some()
            || self.toolbar_hover_amount.is_active(now)
            || self.sidebar_animating
            || self.organizer_animating
            || self
                .edit_animation
                .as_ref()
                .is_some_and(|animation| animation.is_active(now))
        {
            subscriptions.push(window::frames().map(Message::Animate));
        }

        if self.scroll_test.is_some() {
            subscriptions.push(window::frames().map(Message::Frame));
        }

        Subscription::batch(subscriptions)
    }

    pub fn view(&self) -> Element<'_, Message> {
        trace::mark("first view");
        ui::view(self)
    }

    fn save_reading(&mut self) {
        if !self.remember_reading
            || self.reading_suppressed
            || self.edits.dirty()
            || self.document_busy
        {
            return;
        }
        let Some(doc) = self.document() else { return };
        let path = doc.path.clone();
        let position = self.viewer.reading_position();
        if let Some(record_path) = &self.reading_path {
            self.reading_store = crate::reading::Store::load(record_path);
        }
        if self.reading_store.set(&path, position.clone())
            && let Some(record_path) = &self.reading_path
        {
            match self.reading_store.save(record_path) {
                Ok(()) => trace::note(|| {
                    format!(
                        "reading saved: page={}, top={}",
                        position.page + 1,
                        position.top
                    )
                }),
                Err(error) => self.notify(format!("无法保存阅读记录：{error}"), Tone::Error),
            }
        }
    }

    fn open(&mut self, path: PathBuf) {
        self.save_reading();
        self.reading_suppressed = false;
        self.view_transition = None;
        self.external_drop = file_drop::State::default();
        self.dialog = None;
        self.dialog_motion = Tween::new(0.0);
        self.dialog_serial += 1;
        self.opening = Some(path.clone());
        self.document_busy = false;
        if let Some(previous) = self.pending_edit_previous.take() {
            self.edits.restore(previous);
            self.order = self.edits.current().order.clone();
        }
        self.pending_edit_animation = None;
        self.pending_edit_focus = None;
        self.pending_edit_selection.clear();
        self.edit_animation = None;
        self.sidebar_animating = false;
        self.organizer_animating = false;
        self.last_wanted = None;
        self.more_open = false;
        self.engine.open(path, Some(self.plan()));
    }

    /// Works out what a document about to be opened will be asked for
    /// first, by laying it out in a view the shape of this one.
    fn plan(&self) -> engine::Plan {
        let mut options = self.options.clone();
        let viewport = self.viewer.viewport();
        let scale_factor = self.viewer.scale_factor();
        let (open, height) = (self.sidebar.open, self.sidebar.height);
        let organizer = self.view_mode == ViewMode::Organizer;
        let double_page = self.viewer.is_double_page();
        let remember = self.remember_reading;
        let reading_path = self.reading_path.clone();

        Box::new(move |doc| {
            let mut viewer = Viewer::default();
            let mut sidebar = sidebar::State::default();

            viewer.set_double_page(double_page);
            viewer.set_scale_factor(scale_factor);
            viewer.set_viewport(viewport);
            sidebar.open = open;
            sidebar.height = height;

            let position = if remember
                && options.page.is_none()
                && options.zoom.is_none()
                && options.screenshot.is_none()
            {
                reading_path
                    .as_ref()
                    .and_then(|path| crate::reading::Store::load(path).get(&doc.path))
            } else {
                None
            };
            present(&mut viewer, &mut sidebar, &mut options, doc.clone());
            if let Some(position) = position {
                viewer.restore_reading_position(&position);
            }

            let mut order = PageOrder::default();
            order.reset(viewer.page_count());
            let thumbnails = sidebar_thumbnails(&sidebar, &order, &order);

            if organizer {
                let mut pages: Vec<u32> = (0..viewer.page_count().min(12)).collect();
                pages.extend(thumbnails);
                pages.sort_unstable();
                pages.dedup();
                viewer.wanted_thumbnails(&pages)
            } else {
                reader_wanted(&mut viewer, &thumbnails)
            }
        })
    }

    /// Takes in everything the engine has produced since last asked.
    fn drain(&mut self) -> Task<Message> {
        self.pending.store(false, Ordering::Release);

        let mut tasks = Vec::new();

        while let Ok(event) = self.events.try_recv() {
            tasks.push(self.engine_event(event));
        }

        Task::batch(tasks)
    }

    fn notify(&mut self, text: impl Into<String>, tone: Tone) {
        self.toast = Some(Toast {
            text: text.into(),
            tone,
            expires: Some(
                Instant::now()
                    + Duration::from_millis(if tone == Tone::Error { 6000 } else { 2600 }),
            ),
            progress: None,
        });
    }

    /// Goes where a link leads.
    fn follow(&mut self, target: Target) {
        self.set_view(ViewMode::Reader);
        // What the press that clicked the link began to select.
        self.viewer.clear_selection();

        match target {
            Target::Page { page, top } => self.viewer.go_to(page, top),
            Target::Uri(uri) => match web::Address::parse(&uri) {
                Some(address) => {
                    self.notify(format!("正在打开 {}", address.place()), Tone::Info);
                    address.open();
                }
                None => self.notify("Folio Flow 仅支持打开网页和电子邮件链接。", Tone::Error),
            },
        }
    }

    fn update_drop_target(&mut self, point: Option<Point>, autoscroll: bool) {
        let previous = self.external_drop.target;
        self.external_drop.target = None;
        if self.document().is_none() || self.document_busy || self.dialog.is_some() {
            return;
        }
        let Some(point) = point else {
            return;
        };
        if self.sidebar.open
            && self.sidebar.tab == sidebar::Tab::Pages
            && self.sidebar.drop_bounds.contains(point)
        {
            let local = point - self.sidebar.drop_bounds.position();
            if autoscroll {
                let height = self.sidebar.drop_bounds.height;
                let delta = if local.y < 26.0 {
                    -10.0
                } else if local.y > height - 26.0 {
                    10.0
                } else {
                    0.0
                };
                self.sidebar
                    .scroll_for_insertion(delta, &self.order, &self.viewer_order);
            }
            let slot = self.sidebar.insertion_slot(
                local.y,
                &self.order,
                &self.viewer_order,
                previous
                    .filter(|target| target.zone == file_drop::Zone::Sidebar)
                    .map(|target| target.slot),
            );
            self.external_drop.target = Some(file_drop::Target {
                zone: file_drop::Zone::Sidebar,
                slot,
            });
        } else if self.view_mode == ViewMode::Organizer
            && self.organizer.drop_bounds.contains(point)
        {
            let local = point - self.organizer.drop_bounds.position();
            if autoscroll {
                let height = self.organizer.drop_bounds.height;
                let delta = if local.y < 28.0 {
                    -10.0
                } else if local.y > height - 28.0 {
                    10.0
                } else {
                    0.0
                };
                self.organizer
                    .scroll_by(delta, self.viewer.page_count() + 1);
            }
            self.external_drop.target = Some(file_drop::Target {
                zone: file_drop::Zone::Organizer,
                slot: self.organizer.insertion_slot(
                    Point::new(local.x, local.y),
                    self.viewer.page_count(),
                    previous
                        .filter(|target| target.zone == file_drop::Zone::Organizer)
                        .map(|target| target.slot),
                ),
            });
        }
    }

    fn request_leave(&mut self, action: LeaveAction) -> Task<Message> {
        if self.document_busy {
            self.notify("请等待当前操作完成后再关闭或切换文档", Tone::Info);
            return Task::none();
        }
        if self.edits.dirty() {
            self.show_dialog(dialog::Kind::Leave);
            self.leave_action = Some(action);
            return Task::none();
        }
        self.finish_leave(action)
    }

    fn finish_leave(&mut self, action: LeaveAction) -> Task<Message> {
        self.leave_action = None;
        self.leave_after_save = false;
        match action {
            LeaveAction::Open(path) => {
                self.open(path);
                Task::none()
            }
            LeaveAction::Close => {
                self.save_reading();
                window::oldest()
                    .and_then(window::close::<Message>)
                    .discard()
            }
        }
    }

    fn show_dialog(&mut self, kind: dialog::Kind) {
        if !matches!(kind, dialog::Kind::Settings | dialog::Kind::About)
            && (self.document_busy
                || self.opening.is_some()
                || self.document().is_none_or(|doc| doc.pages.is_empty()))
        {
            return;
        }
        self.more_open = false;
        self.dialog_serial += 1;
        let mut state = dialog::State::new(kind);
        state.original_bytes = self
            .document()
            .and_then(|doc| std::fs::metadata(&doc.path).ok())
            .map(|metadata| metadata.len());
        self.dialog = Some(state);
        self.dialog_motion = Tween::new(0.0);
        self.dialog_motion.retarget(1.0, Duration::from_millis(200));
    }

    fn estimate_dialog(&mut self) {
        self.estimate_revision += 1;
        if let Some(dialog) = &mut self.dialog {
            dialog.estimate = None;
            if let dialog::Operation::Raster(settings) = dialog.operation()
                && settings.format == engine::raster::Format::Pdf
            {
                self.engine
                    .estimate_raster(self.estimate_revision, settings);
            }
        }
    }

    fn document(&self) -> Option<&Arc<DocInfo>> {
        self.viewer.document()
    }

    pub fn document_busy(&self) -> bool {
        self.document_busy
    }

    fn edit_slot(&self, source: u32) -> Option<u32> {
        self.edits.current().order.slot_of(source)
    }

    fn edit_source(&self, slot: u32) -> u32 {
        self.edits.current().order.source_at(slot)
    }

    fn viewer_slot(&self, source: u32) -> Option<u32> {
        self.viewer_order.slot_of(source)
    }

    fn viewer_source(&self, slot: u32) -> u32 {
        self.viewer_order.source_at(slot)
    }

    pub fn target_slots(&self) -> Vec<u32> {
        let count = self.viewer.page_count();
        match self.view_mode {
            ViewMode::Reader => {
                if count > 0 {
                    vec![self.viewer.current_page()]
                } else {
                    Vec::new()
                }
            }
            ViewMode::Organizer => {
                let mut slots: Vec<_> = self
                    .organizer
                    .selected
                    .iter()
                    .filter_map(|source| self.edit_slot(*source))
                    .collect();
                slots.sort_unstable();
                slots
            }
        }
    }

    pub fn can_delete(&self) -> bool {
        let targets = self.target_slots();
        !targets.is_empty() && targets.len() < self.viewer.page_count() as usize
    }

    fn selected_sources(&self) -> Vec<u32> {
        self.target_slots()
            .iter()
            .map(|slot| self.edit_source(*slot))
            .collect()
    }

    fn focus_source(&self) -> Option<u32> {
        match self.view_mode {
            ViewMode::Reader => (self.viewer.page_count() > 0)
                .then(|| self.viewer_source(self.viewer.current_page())),
            ViewMode::Organizer => self.organizer.focused(),
        }
    }

    fn commit_edit(&mut self, next: EditState) {
        if self.document_busy || self.opening.is_some() || self.document().is_none() {
            return;
        }

        let previous = self.edits.clone();
        let selection = self.selected_sources();
        let focus = self.focus_source();
        if self.edits.commit(next) {
            self.start_engine_edit(previous, selection, focus);
        }
    }

    fn capture_edit_animation(
        &self,
        previous: &EditHistory,
        next: &EditState,
    ) -> Option<EditAnimation> {
        let (kind, changes) = edit_changes(previous.current(), next);
        if changes.is_empty() {
            return None;
        }

        let pages: Vec<_> = changes
            .into_iter()
            .filter_map(|(from_slot, to_slot, turns)| {
                Some(PageEditAnimation {
                    from_slot,
                    to_slot,
                    turns,
                    snapshot: self.viewer.snapshot(from_slot)?,
                })
            })
            .collect();
        if pages.is_empty() {
            return None;
        }

        Some(EditAnimation::new(
            kind,
            pages,
            previous.current().order.as_sources().len() as u32,
            self.sidebar.offset,
            self.sidebar
                .page_tops(&previous.current().order, &previous.current().order),
            self.viewer.page_rects(),
        ))
    }

    fn start_engine_edit(
        &mut self,
        previous: EditHistory,
        selection: Vec<u32>,
        focus: Option<u32>,
    ) {
        self.view_transition = None;
        let Some(doc) = self.document().cloned() else {
            self.edits.restore(previous);
            return;
        };
        let state = self.edits.current().clone();
        self.pending_edit_animation = self.capture_edit_animation(&previous, &state);
        self.pending_edit_previous = Some(previous);
        self.pending_edit_selection = selection;
        self.pending_edit_focus = focus;
        self.document_busy = true;
        self.more_open = false;
        self.engine.apply_edit(engine::EditRequest {
            doc: doc.id,
            sources: state.order.as_sources().to_vec(),
            rotations: state.rotations().collect(),
        });
    }

    fn undo_edit(&mut self) {
        if self.document_busy || self.opening.is_some() {
            return;
        }
        let previous = self.edits.clone();
        let selection = self.selected_sources();
        let focus = self.focus_source();
        if self.edits.undo() {
            self.start_engine_edit(previous, selection, focus);
        }
    }

    fn redo_edit(&mut self) {
        if self.document_busy || self.opening.is_some() {
            return;
        }
        let previous = self.edits.clone();
        let selection = self.selected_sources();
        let focus = self.focus_source();
        if self.edits.redo() {
            self.start_engine_edit(previous, selection, focus);
        }
    }

    fn set_scale_factor(&mut self, scale_factor: f32) {
        self.viewer.set_scale_factor(scale_factor);
        icon::set_scale_factor(scale_factor);
    }

    fn set_view(&mut self, mode: ViewMode) {
        self.change_view(mode, None);
    }

    fn change_view(&mut self, mode: ViewMode, double_page: Option<bool>) {
        if mode == self.view_mode
            && double_page.is_none_or(|double| double == self.viewer.is_double_page())
        {
            return;
        }
        let transition = ui::transition::Transition::capture(self);
        let focus = (self.view_mode == ViewMode::Organizer && mode == ViewMode::Reader)
            .then(|| {
                self.organizer
                    .focused()
                    .and_then(|source| self.viewer_order.slot_of(source))
            })
            .flatten();
        self.viewer.stop_scroll();
        self.organizer.clamp(self.viewer.page_count());

        if mode != self.view_mode && mode == ViewMode::Organizer {
            self.reader_sidebar_open = self.sidebar.open;
            self.sidebar.open = false;
            self.sidebar_animating = false;
            self.motion.set_sidebar(false);
        } else if mode != self.view_mode {
            self.organizer_animating = false;
            if self.reader_sidebar_open && !self.sidebar.open {
                self.sidebar.open = true;
                self.motion.set_sidebar(true);
            }
        }

        self.view_mode = mode;
        if let Some(double_page) = double_page {
            self.viewer.set_double_page(double_page);
        }
        let size = self.window_size;
        match mode {
            ViewMode::Reader => self.viewer.set_viewport(if self.immersive {
                size
            } else {
                ui::page_view_size(size, self.motion.sidebar.value)
            }),
            ViewMode::Organizer => self.organizer.set_size(
                Size::new(
                    size.width,
                    (size.height
                        - if self.immersive {
                            0.0
                        } else {
                            theme::content_top(size.width)
                        })
                    .max(0.0),
                ),
                self.viewer.page_count(),
                &self.order,
            ),
        }
        if let Some(page) = focus {
            self.viewer.go_to_page(page);
        }
        if mode == ViewMode::Organizer
            && let Some(source) = self.organizer.focused()
        {
            self.organizer
                .reveal_source(source, self.viewer.page_count(), &self.order);
        }
        if transition.is_some() {
            self.motion.view = Tween::new(1.0);
        } else {
            self.motion.switch_view();
        }
        self.view_transition = transition;
    }

    /// Tells the engine what the current view is missing. Called after
    /// anything that changes what is on screen.
    fn settle_edit_animation(&mut self, sidebar_pages: &[u32], content_ready: bool) {
        let Some(animation) = &self.edit_animation else {
            return;
        };
        let now = Instant::now();
        if animation.is_active(now) {
            return;
        }
        if animation.kind == EditKind::Delete {
            self.edit_animation = None;
            return;
        }

        let main_pages: Vec<u32> = match self.view_mode {
            ViewMode::Reader => self.viewer.visible_pages().collect(),
            ViewMode::Organizer => self.organizer.wanted_pages(
                self.viewer.page_count(),
                &self.order,
                &self.viewer_order,
            ),
        };
        let visible = animation.pages.iter().any(|page| {
            page.to_slot
                .is_some_and(|slot| main_pages.contains(&slot) || sidebar_pages.contains(&slot))
        });
        if !visible || content_ready {
            self.edit_animation = None;
        }
    }

    fn sync(&mut self) {
        let mut sidebar_pages = sidebar_thumbnails(&self.sidebar, &self.order, &self.viewer_order);
        if let Some(transition) = &self.view_transition {
            sidebar_pages.extend(
                transition
                    .sources()
                    .filter_map(|source| self.viewer_order.slot_of(source)),
            );
            sidebar_pages.sort_unstable();
            sidebar_pages.dedup();
        }
        let wanted = match self.view_mode {
            ViewMode::Reader => reader_wanted(&mut self.viewer, &sidebar_pages),
            ViewMode::Organizer => {
                let mut pages = self.organizer.wanted_pages(
                    self.viewer.page_count(),
                    &self.order,
                    &self.viewer_order,
                );
                pages.extend(sidebar_pages.iter().copied());
                pages.sort_unstable();
                pages.dedup();
                self.viewer.wanted_thumbnails(&pages)
            }
        };

        self.settle_edit_animation(
            &sidebar_pages,
            wanted.tiles.is_empty() && wanted.thumbnails.is_empty(),
        );

        if self.view_mode == ViewMode::Reader
            && self.document().is_some()
            && self.viewer.has_viewport()
            && self.viewer.is_complete()
        {
            trace::mark("view complete");
        }

        if self.view_mode == ViewMode::Reader
            && let Some(test) = self.scroll_test.as_mut()
        {
            test.missing = !self.viewer.is_complete();
        }

        let has_view = match self.view_mode {
            ViewMode::Reader => self.viewer.has_viewport(),
            ViewMode::Organizer => self.organizer.size != Size::ZERO,
        };
        let settled = self.opening.is_none()
            && !self.find.searching
            && has_view
            && wanted.tiles.is_empty()
            && wanted.texts.is_empty()
            && wanted.thumbnails.is_empty();

        if settled && self.screenshot_countdown.is_none() && self.options.screenshot.is_some() {
            self.screenshot_countdown = Some(2);
        }

        if settled
            && self.document().is_some()
            && self.view_mode == ViewMode::Reader
            && self.scroll_test.is_none()
            && self.options.scroll_test.is_some()
        {
            trace::memory("before scrolling");
            self.scroll_test = Some(ScrollTest {
                last: Instant::now(),
                intervals: Vec::new(),
                missing: false,
                incomplete: 0,
            });
        }

        let page = match self.view_mode {
            ViewMode::Reader => self.viewer.current_page(),
            ViewMode::Organizer => self
                .organizer
                .focused()
                .and_then(|source| self.viewer_slot(source))
                .unwrap_or_else(|| self.viewer.current_page()),
        };

        if let Some(doc) = self.document().cloned() {
            let changed = self
                .last_wanted
                .as_ref()
                .is_none_or(|(last_doc, last)| *last_doc != doc.id || last != &wanted);
            if changed {
                let last = engine::Wanted {
                    received: wanted.received,
                    tiles: wanted.tiles.clone(),
                    texts: wanted.texts.clone(),
                    thumbnails: wanted.thumbnails.clone(),
                };
                self.engine.want(doc.id, wanted);
                self.last_wanted = Some((doc.id, last));
            }
            self.sidebar.follow(Some(&doc), page);
        } else {
            self.last_wanted = None;
        }
    }

    /// Searches the document for the find bar's query, from where the reader
    /// is. A blank query only clears the last search.
    fn search(&mut self) {
        let from = self.viewer.begin_search();

        self.find.id += 1;
        self.find.searching = !self.find.query.trim().is_empty();
        self.find.capped = false;
        trace::note(|| format!("search for {:?}", self.find.query));
        self.engine
            .search(self.find.id, self.find.query.clone(), from);
    }

    fn open_find(&mut self) -> Task<Message> {
        if self.document().is_none() {
            return Task::none();
        }

        // Reopening brings the last query's matches back.
        if !self.find.open {
            self.find.open = true;
            self.motion.set_find(true);
            self.search();
        }

        Task::batch([
            iced::widget::operation::focus(Find::FIELD),
            iced::widget::operation::select_all(Find::FIELD),
        ])
    }

    fn close_find(&mut self) -> Task<Message> {
        self.find.open = false;
        self.motion.set_find(false);
        self.find.id += 1;
        self.find.searching = false;
        self.engine.search(self.find.id, String::new(), 0);
        self.viewer.clear_matches();

        // Hand the keyboard back to the document: focusing an id nothing
        // carries takes focus away from the field.
        iced::widget::operation::focus(iced::widget::Id::new("document"))
    }

    fn refresh_page_input(&mut self) {
        self.page_input = if self.document().is_some() {
            (self.viewer.current_page() + 1).to_string()
        } else {
            String::new()
        };
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        if matches!(message, Message::Animate(_)) {
            return self.handle(message);
        }

        let page_before = self.viewer.current_page();
        let produced = self.drain();
        let task = self.handle(message);

        self.sync();

        let page = self.viewer.current_page();

        if page != page_before {
            self.refresh_page_input();
            self.sidebar
                .reveal_source(self.viewer_source(page), &self.order, &self.viewer_order);
        }

        Task::batch([produced, task])
    }

    fn handle(&mut self, message: Message) -> Task<Message> {
        if matches!(
            message,
            Message::Save | Message::SaveAs | Message::WindowAction(WindowAction::Close)
        ) {
            self.more_open = false;
        }
        if matches!(
            &message,
            Message::Viewer(
                Interaction::Scrolled { .. }
                    | Interaction::Panned { .. }
                    | Interaction::Zoomed { .. }
                    | Interaction::Pressed { .. }
                    | Interaction::Dragged { .. }
                    | Interaction::ScrolledTo { .. }
            ) | Message::OrganizerScrolled(_)
                | Message::OrganizerScrolledTo(_)
                | Message::OrganizerClicked { .. }
                | Message::ZoomTo(_)
                | Message::Fit(_)
                | Message::Rotate(_)
                | Message::DeletePages
        ) {
            self.view_transition = None;
        }
        match message {
            // Every update takes in what the engine has produced.
            Message::Engine => {}
            Message::Animate(now) => {
                if let Some(transition) = &mut self.view_transition
                    && !transition.advance(now)
                {
                    self.view_transition = None;
                }
                if self.organizer.scrolling() {
                    self.organizer.advance_scroll(now);
                    self.sync();
                }
                if self.sidebar.scrolling() {
                    self.sidebar.advance_scroll(now);
                    self.sync();
                }
                if self.viewer.scrolling() {
                    self.viewer.advance_scroll(now);
                    self.sync();
                    self.refresh_page_input();
                }
                self.motion.advance(now);
                self.dialog_motion.advance(now);
                if self.dialog_motion.value <= 0.001 && self.dialog_motion.to == 0.0 {
                    self.dialog = None;
                }
                self.toolbar_hover_amount.advance(now);
                if let Some(animation) = &mut self.edit_animation {
                    animation.advance(now);
                }
                if !self.motion.is_active(now)
                    && !self.toolbar_hover_amount.is_active(now)
                    && self
                        .edit_animation
                        .as_ref()
                        .is_none_or(|animation| !animation.is_active(now))
                {
                    self.sync();
                }
                return self.sidebar.outline_scroll_task();
            }
            Message::ToolbarHover { control, hovered } => {
                if hovered {
                    if self.toolbar_hover_control != Some(control) {
                        self.toolbar_hover_control = Some(control);
                        self.toolbar_hover_amount = Tween::new(0.0);
                    }
                    self.toolbar_hover_amount
                        .retarget(1.0, TOOLBAR_HOVER_DURATION);
                } else if self.toolbar_hover_control == Some(control) {
                    self.toolbar_hover_amount
                        .retarget(0.0, TOOLBAR_HOVER_DURATION);
                }
            }
            Message::Viewer(interaction) => match interaction {
                Interaction::Resized(size) => {
                    // Published while the first frame was being drawn.
                    trace::mark("first frame drawn");
                    self.viewer.set_viewport(size);
                }
                Interaction::Scrolled { dx, dy } => self.viewer.wheel_by(dx, dy),
                Interaction::Panned { dx, dy } => self.viewer.scroll_by(dx, dy),
                Interaction::Zoomed { factor, at } => self.viewer.zoom_by(factor, Some(at)),
                Interaction::Pressed {
                    at,
                    granularity,
                    extend,
                } => self.viewer.press(at, granularity, extend),
                Interaction::Dragged { at } => self.viewer.drag(at),
                Interaction::Released => {}
                Interaction::Clicked { at } => {
                    if let Some(target) = self.viewer.link_at(at).map(|link| link.target.clone()) {
                        self.follow(target);
                    }
                }
                Interaction::ScrolledTo { fraction } => self.viewer.scroll_to_fraction(fraction),
            },
            Message::ToggleImmersive => {
                if self.dialog.is_none() {
                    self.view_transition = None;
                    self.immersive = !self.immersive;
                    self.toolbar_hover_control = None;
                    self.toolbar_hover_amount = Tween::new(0.0);
                    self.more_open = false;
                    self.notify(
                        if self.immersive {
                            "已进入沉浸模式 · 按 Tab 恢复工具栏"
                        } else {
                            "已退出沉浸模式 · 按 Tab 隐藏工具栏"
                        },
                        Tone::Info,
                    );
                }
            }
            Message::SetReadingLayout(double_page) => {
                self.change_view(ViewMode::Reader, Some(double_page));
            }
            Message::SetView(mode) => {
                if mode == ViewMode::Organizer {
                    let source = self.viewer_source(self.viewer.current_page());
                    self.organizer
                        .select(source, keyboard::Modifiers::default(), &self.order);
                    self.organizer
                        .reveal_source(source, self.viewer.page_count(), &self.order);
                }
                self.set_view(mode);
            }
            Message::OrganizerFrame {
                size,
                bounds,
                animating,
            } => {
                self.organizer.drop_bounds = bounds;
                self.organizer_animating = animating;
                self.organizer
                    .set_size(size, self.viewer.page_count(), &self.order);
            }
            Message::OrganizerScrolled(delta) => {
                self.organizer.wheel_by(delta, self.viewer.page_count());
            }
            Message::OrganizerScrolledTo(offset) => {
                self.organizer.scroll_to(offset, self.viewer.page_count());
            }
            Message::OrganizerClicked {
                slot,
                modifiers,
                double_click,
            } => {
                let source = self.order.source_at(slot);
                if double_click {
                    self.set_view(ViewMode::Reader);
                    if let Some(page) = self.viewer_slot(source) {
                        self.viewer.go_to_page(page);
                    }
                } else {
                    self.organizer.select(source, modifiers, &self.order);
                    self.organizer
                        .reveal_source(source, self.viewer.page_count(), &self.order);
                }
            }
            Message::OrganizerDrag {
                source,
                slot,
                scroll,
                commit,
            } => {
                if !self.document_busy {
                    self.order.move_source(source, slot);
                }
                self.organizer.scroll_by(scroll, self.viewer.page_count());
                if commit {
                    let mut next = self.edits.current().clone();
                    next.order.move_source(source, slot);
                    self.commit_edit(next);
                }
            }
            Message::SidebarPagesFrame {
                size,
                bounds,
                animating,
            } => {
                self.sidebar.drop_bounds = bounds;
                self.sidebar_animating = animating;
                self.sidebar.set_pages_size(
                    size,
                    self.viewer.page_count(),
                    &self.order,
                    &self.viewer_order,
                );
            }
            Message::SidebarPageClicked { source } => {
                if self.view_mode == ViewMode::Reader {
                    self.view_transition = None;
                    if let Some(slot) = self.viewer_slot(source) {
                        self.viewer.animate_to_page(slot);
                    }
                } else {
                    self.set_view(ViewMode::Reader);
                    if let Some(slot) = self.viewer_slot(source) {
                        self.viewer.go_to_page(slot);
                    }
                }
            }
            Message::SidebarPageDrag {
                source,
                slot,
                scroll,
                commit,
            } => {
                if !self.document_busy {
                    self.order.move_source(source, slot);
                }
                self.sidebar
                    .scroll_by(scroll, &self.order, &self.viewer_order);
                if commit {
                    let mut next = self.edits.current().clone();
                    next.order.move_source(source, slot);
                    self.commit_edit(next);
                }
            }
            Message::SidebarStopWheel => self.sidebar.stop_scroll(),
            Message::SidebarWheel(delta) => {
                self.sidebar
                    .wheel_by(delta, &self.order, &self.viewer_order);
            }
            Message::SidebarPagesScrolled(delta) => {
                self.sidebar
                    .scroll_by(delta, &self.order, &self.viewer_order);
            }
            Message::GridSize(track) => {
                self.organizer.track = track.clamp(organizer::MIN_TRACK, organizer::MAX_TRACK);
                self.organizer.clamp(self.viewer.page_count());

                if let Some(page) = self.organizer.focused() {
                    self.organizer
                        .reveal_source(page, self.viewer.page_count(), &self.order);
                }
            }
            Message::DragWindow => {
                return window::oldest().and_then(window::drag::<Message>).discard();
            }
            Message::WindowSize(size) => {
                if size != self.window_size {
                    self.view_transition = None;
                }
                self.window_size = size;
                return shape_window_task(window_border_color(self.palette));
            }
            Message::WindowMaximized(maximized) => self.window_maximized = maximized,
            Message::WindowAction(action) => {
                return match action {
                    WindowAction::Minimize => window::oldest()
                        .and_then(|id| window::minimize::<Message>(id, true))
                        .discard(),
                    WindowAction::Maximize => {
                        self.window_maximized = !self.window_maximized;
                        window::oldest()
                            .and_then(window::toggle_maximize::<Message>)
                            .discard()
                    }
                    WindowAction::Close => self.request_leave(LeaveAction::Close),
                };
            }
            Message::OpenDialog => {
                self.more_open = false;
                if self.document_busy {
                    return Task::none();
                }
                return Task::perform(
                    async {
                        rfd::AsyncFileDialog::new()
                            .set_title("打开 PDF")
                            .add_filter("PDF 文档", &["pdf"])
                            .add_filter("所有文件", &["*"])
                            .pick_file()
                            .await
                            .map(|file| file.path().to_path_buf())
                    },
                    Message::Open,
                );
            }
            Message::Open(path) => {
                if let Some(path) = path {
                    return self.request_leave(LeaveAction::Open(path));
                }
            }
            Message::ToggleSidebar => {
                self.sidebar.open = !self.sidebar.open;
                self.motion.set_sidebar(self.sidebar.open);
            }
            Message::ShowTab(tab) => {
                self.sidebar.show(tab);
                if tab != sidebar::Tab::Pages {
                    self.sidebar_animating = false;
                }
                if let Some(source) = self.focus_source() {
                    self.sidebar
                        .reveal_source(source, &self.order, &self.viewer_order);
                }
            }
            Message::ToggleOutline(index) => {
                let doc = self.document().cloned();
                self.sidebar.toggle(doc.as_deref(), index);
            }
            Message::GoTo(page) => {
                self.set_view(ViewMode::Reader);
                self.viewer.go_to_page(page);
            }
            Message::SidebarScrolled(viewport) => {
                if !self.sidebar.scrolling() {
                    self.sidebar.offset = viewport.absolute_offset().y;
                }
                self.sidebar.height = viewport.bounds().height;
            }
            Message::PageInput(value) => {
                self.page_input = value.chars().filter(char::is_ascii_digit).take(7).collect();
            }
            Message::PageSubmit => {
                if let Ok(page) = self.page_input.parse::<u32>() {
                    self.set_view(ViewMode::Reader);
                    self.viewer.go_to_page(page.saturating_sub(1));
                }
                self.refresh_page_input();

                // Hand the keyboard back to the document, as closing the
                // find bar does.
                return iced::widget::operation::focus(iced::widget::Id::new("document"));
            }
            Message::ZoomTo(zoom) => {
                if self.view_mode == ViewMode::Reader {
                    self.viewer.zoom_to(zoom, None);
                }
            }
            Message::ZoomReset => self.viewer.zoom_to(1.0, None),
            Message::Fit(fit) => {
                // Pressing the active mode again releases it.
                let fit = if self.viewer.fit() == fit {
                    Fit::Manual
                } else {
                    fit
                };
                self.viewer.set_fit(fit);
            }
            Message::Undo => self.undo_edit(),
            Message::Redo => self.redo_edit(),
            Message::Rotate(quarter_turns) => {
                let targets = self.target_slots();
                if targets.is_empty() {
                    return Task::none();
                }
                let mut next = self.edits.current().clone();
                next.rotate_slots(
                    &targets,
                    if quarter_turns < 0 {
                        3
                    } else {
                        quarter_turns as u8
                    },
                );
                self.commit_edit(next);
            }
            Message::DeletePages => {
                let targets = self.target_slots();
                if !self.can_delete() {
                    return Task::none();
                }
                let mut next = self.edits.current().clone();
                next.delete_slots(&targets);
                self.commit_edit(next);
            }
            Message::Extract => {
                self.more_open = false;
                let Some(doc) = self.document().cloned() else {
                    return Task::none();
                };
                let pages = self.target_slots();
                if pages.is_empty() || self.document_busy {
                    return Task::none();
                }
                let stem = doc
                    .path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "document".into());
                let name = if pages.len() == 1 {
                    format!("{stem}-page-{}.pdf", pages[0] + 1)
                } else {
                    format!("{stem}-extracted.pdf")
                };
                let directory = doc.path.parent().map(PathBuf::from);

                return Task::perform(
                    async move {
                        let mut dialog = rfd::AsyncFileDialog::new()
                            .set_title("提取页面")
                            .set_file_name(name)
                            .add_filter("PDF 文档", &["pdf"]);
                        if let Some(directory) = directory {
                            dialog = dialog.set_directory(directory);
                        }
                        dialog
                            .save_file()
                            .await
                            .map(|file| file.path().to_path_buf())
                    },
                    move |path| Message::ExtractTo { path, pages },
                );
            }
            Message::ExtractTo { path, pages } => {
                if let Some(path) = path {
                    if self.document().is_none() || self.document_busy {
                        return Task::none();
                    }
                    self.document_busy = true;
                    self.more_open = false;
                    self.toast = Some(Toast {
                        text: "正在提取页面…".into(),
                        tone: Tone::Info,
                        expires: None,
                        progress: Some(0.0),
                    });
                    self.engine.extract(path, pages);
                }
            }
            Message::Save => {
                self.more_open = false;
                let Some(doc) = self.document().cloned() else {
                    self.notify("请先打开 PDF 再保存。", Tone::Error);
                    return Task::none();
                };
                if self.document_busy {
                    return Task::none();
                }
                if !self.edits.dirty() {
                    self.notify("没有未保存的更改", Tone::Info);
                    return Task::none();
                }
                self.document_busy = true;
                self.more_open = false;
                self.toast = Some(Toast {
                    text: "正在保存 PDF…".into(),
                    tone: Tone::Info,
                    expires: None,
                    progress: None,
                });
                self.engine.save(doc.path.clone());
            }
            Message::SaveAs => {
                let Some(doc) = self.document().cloned() else {
                    return Task::none();
                };
                if self.document_busy {
                    return Task::none();
                }
                self.more_open = false;
                let name = doc
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "document.pdf".into());
                let directory = doc.path.parent().map(PathBuf::from);

                return Task::perform(
                    async move {
                        let mut dialog = rfd::AsyncFileDialog::new()
                            .set_title("PDF 另存为")
                            .set_file_name(name)
                            .add_filter("PDF 文档", &["pdf"]);
                        if let Some(directory) = directory {
                            dialog = dialog.set_directory(directory);
                        }
                        dialog
                            .save_file()
                            .await
                            .map(|file| file.path().to_path_buf())
                    },
                    Message::SaveTo,
                );
            }
            Message::SaveTo(path) => {
                if let Some(path) = path {
                    if self.document().is_none() || self.document_busy {
                        return Task::none();
                    }
                    self.document_busy = true;
                    self.more_open = false;
                    self.toast = Some(Toast {
                        text: "正在保存 PDF…".into(),
                        tone: Tone::Info,
                        expires: None,
                        progress: None,
                    });
                    self.engine.save(path);
                }
            }
            Message::Compact => self.show_dialog(dialog::Kind::Compress),
            Message::LeaveChoice(choice) => {
                if self.document_busy || self.leave_action.is_none() {
                    return Task::none();
                }
                self.dialog_motion.retarget(0.0, Duration::from_millis(160));
                match choice {
                    LeaveChoice::Discard => {
                        let action = self.leave_action.take().unwrap();
                        return self.finish_leave(action);
                    }
                    LeaveChoice::Save => {
                        if !self.edits.dirty() {
                            let action = self.leave_action.take().unwrap();
                            return self.finish_leave(action);
                        }
                        self.leave_after_save = true;
                        return self.handle(Message::Save);
                    }
                }
            }
            Message::DialogIgnore => {}
            Message::DialogClose => {
                if self
                    .dialog
                    .as_ref()
                    .is_some_and(|dialog| dialog.kind == dialog::Kind::Leave)
                {
                    self.leave_action = None;
                    self.leave_after_save = false;
                }
                self.dialog_serial += 1;
                self.dialog_motion.retarget(0.0, Duration::from_millis(160));
            }
            Message::DialogFormat(format) => {
                if let Some(dialog) = &mut self.dialog {
                    dialog.format = format;
                }
            }
            Message::DialogScale(scale) => {
                if [1.0, 1.5, 2.0, 3.0].contains(&scale)
                    && let Some(dialog) = &mut self.dialog
                {
                    dialog.scale = scale;
                }
            }
            Message::DialogImagePdf(value) => {
                if let Some(dialog) = &mut self.dialog {
                    dialog.image_pdf = value;
                }
                self.estimate_dialog();
            }
            Message::DialogDpi(dpi) => {
                if [72.0, 144.0, 216.0].contains(&dpi) {
                    if let Some(dialog) = &mut self.dialog {
                        dialog.dpi = dpi;
                    }
                    self.estimate_dialog();
                }
            }
            Message::DialogQuality(quality) => {
                if [60, 75, 90].contains(&quality) {
                    if let Some(dialog) = &mut self.dialog {
                        dialog.quality = quality;
                    }
                    self.estimate_dialog();
                }
            }
            Message::DialogConfirm => {
                if self.dialog.as_ref().is_some_and(|d| {
                    matches!(
                        d.kind,
                        dialog::Kind::Settings | dialog::Kind::About | dialog::Kind::Leave
                    )
                }) {
                    return Task::none();
                }
                let Some(doc) = self.document().cloned() else {
                    return Task::none();
                };
                if self.document_busy || self.opening.is_some() {
                    return Task::none();
                }
                let Some(dialog) = &mut self.dialog else {
                    return Task::none();
                };
                if dialog.choosing || self.dialog_motion.to == 0.0 {
                    return Task::none();
                }
                let operation = dialog.operation();
                if let dialog::Operation::Raster(settings) = operation
                    && let Some(error) = doc
                        .pages
                        .iter()
                        .find_map(|page| settings.dimensions(*page).err())
                {
                    self.notify(error, Tone::Error);
                    return Task::none();
                }
                dialog.choosing = true;
                let serial = self.dialog_serial;
                let stem = doc
                    .path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let directory = doc.path.parent().map(PathBuf::from);
                return Task::perform(
                    async move {
                        let mut picker = rfd::AsyncFileDialog::new();
                        if let Some(directory) = directory {
                            picker = picker.set_directory(directory);
                        }
                        let path = match operation {
                            dialog::Operation::Raster(settings)
                                if settings.format != engine::raster::Format::Pdf =>
                            {
                                picker
                                    .set_title("选择图片导出位置")
                                    .pick_folder()
                                    .await
                                    .map(|folder| {
                                        let stamp = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_millis();
                                        folder.path().join(format!("{stem}-图片-{stamp}"))
                                    })
                            }
                            _ => {
                                let (title, name, filter, extension) = match operation {
                                    dialog::Operation::Markdown => (
                                        "导出 Markdown",
                                        format!("{stem}.md"),
                                        "Markdown 文档",
                                        "md",
                                    ),
                                    dialog::Operation::Compact => (
                                        "保存压缩副本",
                                        format!("{stem}-压缩.pdf"),
                                        "PDF 文档",
                                        "pdf",
                                    ),
                                    _ => (
                                        "保存纯图 PDF",
                                        format!("{stem}-纯图.pdf"),
                                        "PDF 文档",
                                        "pdf",
                                    ),
                                };
                                picker
                                    .set_title(title)
                                    .set_file_name(name)
                                    .add_filter(filter, &[extension])
                                    .save_file()
                                    .await
                                    .map(|file| file.path().to_path_buf())
                            }
                        };
                        Message::DialogDestination {
                            doc: doc.id,
                            serial,
                            operation,
                            path,
                        }
                    },
                    |message| message,
                );
            }
            Message::DialogDestination {
                doc,
                serial,
                operation,
                path,
            } => {
                if self.document().is_none_or(|current| current.id != doc)
                    || self.dialog_serial != serial
                {
                    return Task::none();
                }
                if let Some(dialog) = &mut self.dialog {
                    dialog.choosing = false;
                }
                let Some(path) = path else {
                    return Task::none();
                };
                if self.document_busy || self.opening.is_some() {
                    return Task::none();
                }
                if let Some(current) = self.document()
                    && std::fs::canonicalize(&path)
                        .ok()
                        .zip(std::fs::canonicalize(&current.path).ok())
                        .is_some_and(|(a, b)| a == b)
                {
                    self.notify("请选择其他路径，导出和压缩均保存为副本。", Tone::Error);
                    return Task::none();
                }
                self.document_busy = true;
                self.dialog_motion.retarget(0.0, Duration::from_millis(160));
                self.toast = Some(Toast {
                    text: "正在处理文档…".into(),
                    tone: Tone::Info,
                    expires: None,
                    progress: Some(0.0),
                });
                match operation {
                    dialog::Operation::Markdown => self.engine.export_markdown(path),
                    dialog::Operation::Compact => self.engine.compact(path),
                    dialog::Operation::Raster(settings) => {
                        self.engine.export_raster(path, settings)
                    }
                }
            }
            Message::ToggleMore => self.more_open = !self.more_open,
            Message::ShowSettings => self.show_dialog(dialog::Kind::Settings),
            Message::ShowAbout => self.show_dialog(dialog::Kind::About),
            Message::ToggleRemember => {
                let enabled = !self.remember_reading;
                match crate::preferences::save_remember(enabled) {
                    Ok(()) => {
                        self.remember_reading = enabled;
                        self.reading_suppressed = false;
                    }
                    Err(error) => self.notify(format!("无法保存阅读设置：{error}"), Tone::Error),
                }
            }
            Message::ClearReading => {
                let empty = crate::reading::Store::default();
                if let Some(path) = &self.reading_path {
                    match empty.save(path) {
                        Ok(()) => {
                            self.reading_store = empty;
                            self.reading_suppressed = true;
                            self.notify("已清除阅读记录", Tone::Info);
                        }
                        Err(error) => {
                            self.notify(format!("无法清除阅读记录：{error}"), Tone::Error)
                        }
                    }
                }
            }
            Message::OpenProject(releases) => {
                if let Some(address) = web::Address::parse(if releases {
                    "https://github.com/kelsoz/Folio-Flow/releases"
                } else {
                    "https://github.com/kelsoz/Folio-Flow"
                }) {
                    address.open();
                }
            }
            Message::DefaultPdf => {
                return Task::perform(
                    async { crate::web::default_pdf().map_err(|e| e.to_string()) },
                    Message::DefaultPdfResult,
                );
            }
            Message::DefaultPdfResult(result) => match result {
                Ok(()) => self.notify(
                    "请在 Windows 设置中选择 Folio Flow 作为 PDF 默认应用",
                    Tone::Info,
                ),
                Err(error) => self.notify(format!("无法打开默认应用设置：{error}"), Tone::Error),
            },
            Message::Export => self.show_dialog(dialog::Kind::Export),
            Message::Print => {
                if self.document().is_some() {
                    return window::oldest()
                        .and_then(|id| window::run(id, |window| window_handle(window)))
                        .map(Message::PrintOwner);
                }
            }
            Message::PrintOwner(owner) => {
                let pages = self.viewer.page_count();
                let current = self.viewer.current_page();

                // The system dialog is modal and blocks its thread.
                let (sender, receiver) = oneshot::channel();

                std::thread::spawn(move || {
                    let _ = sender.send(print::prompt(owner, pages, current));
                });

                return Task::perform(receiver, |target| {
                    Message::PrintTarget(Once::new(target.unwrap_or(Ok(None))))
                });
            }
            Message::PrintTarget(target) => match target.take() {
                Some(Ok(Some(target))) => {
                    self.toast = Some(Toast {
                        text: "正在打印…".into(),
                        tone: Tone::Info,
                        expires: None,
                        progress: Some(0.0),
                    });
                    self.engine.print(target);
                }
                Some(Err(error)) => self.notify(error, Tone::Error),
                _ => {}
            },
            Message::ToggleTheme => {
                self.palette = if self.palette.is_dark {
                    Palette::LIGHT
                } else {
                    Palette::DARK
                };
                if let Err(error) = crate::preferences::save_light(!self.palette.is_dark) {
                    self.notify(format!("无法保存主题设置：{error}"), Tone::Error);
                }
                return shape_window_task(window_border_color(self.palette));
            }
            Message::Find => {
                self.set_view(ViewMode::Reader);
                return self.open_find();
            }
            Message::FindInput(query) => {
                self.find.query = query;
                self.search();
            }
            Message::FindSubmit => {
                self.viewer
                    .step_match(if self.modifiers.shift() { -1 } else { 1 });
            }
            Message::FindStep(direction) => self.viewer.step_match(direction),
            Message::Escape => {
                if self.dialog.is_some() {
                    return self.handle(Message::DialogClose);
                }
                if self.more_open {
                    self.more_open = false;
                    return Task::none();
                } else if self.find.open {
                    return self.close_find();
                }

                match self.view_mode {
                    ViewMode::Reader => self.viewer.clear_selection(),
                    ViewMode::Organizer => self.organizer.clear_selection(),
                }
            }
            Message::Key(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                return self.key_pressed(key, modifiers);
            }
            Message::Key(keyboard::Event::ModifiersChanged(modifiers)) => {
                self.modifiers = modifiers;
            }
            Message::Key(_) => {}
            Message::FileHovered(path) => {
                if self.document_busy || self.opening.is_some() || self.dialog.is_some() {
                    return Task::none();
                }
                self.more_open = false;
                self.external_drop.path = Some(path);
                self.external_drop.hovering = true;
                return external_position_task(None);
            }
            Message::FileHoverLeft => {
                self.organizer.clamp(self.viewer.page_count());
                self.sidebar.clamp_pages(&self.order, &self.viewer_order);
                if !self.external_drop.importing {
                    self.external_drop = file_drop::State::default();
                }
            }
            Message::ExternalPoll => {
                if self.external_drop.hovering {
                    return external_position_task(None);
                }
            }
            Message::ExternalPosition(point) => {
                if self.external_drop.hovering {
                    self.update_drop_target(point, true);
                }
            }
            Message::FileDropped(path) => return external_position_task(Some(path)),
            Message::DropAt { path, point } => {
                if self.document_busy || self.opening.is_some() || self.dialog.is_some() {
                    self.notify("请等待当前操作完成后再拖入文件", Tone::Info);
                    return Task::none();
                }
                if point.is_none() && self.document().is_some() {
                    self.external_drop = file_drop::State::default();
                    self.notify(
                        "无法确定放置位置，请重新拖入页面导航栏或页面编排视图",
                        Tone::Error,
                    );
                    return Task::none();
                }
                self.external_drop.path = Some(path.clone());
                self.update_drop_target(point, false);
                self.external_drop.hovering = false;
                if let Some(target) = self.external_drop.target {
                    if !file_drop::supported(&path) {
                        self.external_drop = file_drop::State::default();
                        self.notify(
                            "支持拖入 PDF、PNG、JPG、WebP、BMP、GIF 和 TIFF 文件",
                            Tone::Error,
                        );
                        return Task::none();
                    }
                    let doc = self.document().unwrap().clone();
                    self.pending_edit_previous = Some(self.edits.clone());
                    self.pending_edit_animation = None;
                    self.document_busy = true;
                    self.external_drop.importing = true;
                    self.engine.import_pages(engine::ImportRequest {
                        edit: engine::EditRequest {
                            doc: doc.id,
                            sources: self.edits.current().order.as_sources().to_vec(),
                            rotations: self.edits.current().rotations().collect(),
                        },
                        path,
                        slot: target.slot,
                    });
                    self.toast = Some(Toast {
                        text: "正在插入页面…".into(),
                        tone: Tone::Info,
                        expires: None,
                        progress: None,
                    });
                } else {
                    self.external_drop = file_drop::State::default();
                    if file_drop::is_pdf(&path) {
                        return self.request_leave(LeaveAction::Open(path));
                    } else {
                        self.notify("请将图片拖入页面导航栏或页面编排视图", Tone::Info);
                    }
                }
            }
            Message::ScaleFactor(scale_factor) => {
                self.set_scale_factor(scale_factor);
                return shape_window_task(window_border_color(self.palette));
            }
            Message::Tick(now) => {
                if self
                    .toast
                    .as_ref()
                    .and_then(|toast| toast.expires)
                    .is_some_and(|expires| now >= expires)
                {
                    self.toast = None;
                }

                if self
                    .resting_since
                    .is_some_and(|since| now.duration_since(since) >= SCROLL_TEST_REST)
                {
                    trace::memory("at rest");
                    return iced::exit();
                }

                match self.screenshot_countdown {
                    Some(0) => {
                        self.screenshot_countdown = Some(u8::MAX);
                        trace::memory("settled");
                        trace::note(|| self.viewer.cache_report());

                        return window::oldest()
                            .and_then(window::screenshot)
                            .map(|shot| Message::Screenshot(Once::new(shot)));
                    }
                    Some(u8::MAX) => {}
                    Some(frames) => self.screenshot_countdown = Some(frames - 1),
                    None => {}
                }
            }
            Message::Frame(now) => return self.scroll_test_frame(now),
            Message::DismissToast => self.toast = None,
            Message::Screenshot(shot) => {
                if let (Some(shot), Some(path)) = (shot.take(), self.options.screenshot.clone()) {
                    let saved = image::save_buffer(
                        &path,
                        &shot.rgba,
                        shot.size.width,
                        shot.size.height,
                        image::ColorType::Rgba8,
                    );

                    if let Err(error) = saved {
                        eprintln!("screenshot failed: {error}");
                    }
                }

                return iced::exit();
            }
        }

        Task::none()
    }

    fn engine_event(&mut self, event: engine::Event) -> Task<Message> {
        let current = self.document().map(|doc| doc.id);

        match event {
            engine::Event::Opened(doc) => {
                trace::mark("opened received");
                self.opening = None;

                if self.remember_reading
                    && let Some(path) = &self.reading_path
                {
                    self.reading_store = crate::reading::Store::load(path);
                }
                let restore = if self.remember_reading
                    && self.options.page.is_none()
                    && self.options.zoom.is_none()
                    && self.options.screenshot.is_none()
                {
                    self.reading_store.get(&doc.path)
                } else {
                    None
                };
                present(&mut self.viewer, &mut self.sidebar, &mut self.options, doc);
                if let Some(position) = restore {
                    self.viewer.restore_reading_position(&position);
                    trace::note(|| {
                        format!(
                            "reading restored: page={}, top={}",
                            position.page + 1,
                            position.top
                        )
                    });
                }
                self.order.reset(self.viewer.page_count());
                self.edits.reset(self.viewer.page_count());
                self.viewer_order.reset(self.viewer.page_count());
                self.document_busy = false;
                self.pending_edit_previous = None;
                self.pending_edit_animation = None;
                self.pending_edit_focus = None;
                self.pending_edit_selection.clear();
                self.edit_animation = None;
                self.more_open = false;
                self.set_view(if self.options.organizer {
                    ViewMode::Organizer
                } else {
                    ViewMode::Reader
                });
                self.organizer.set_document(self.viewer.page_count());

                self.refresh_page_input();

                if let Some(query) = self.options.find.take() {
                    self.find.open = true;
                    self.find.query = query;
                }

                // The find bar stays open across documents, on its query.
                if self.find.open {
                    self.search();
                }

                let print = if std::mem::take(&mut self.options.print) {
                    Task::done(Message::Print)
                } else {
                    Task::none()
                };

                return Task::batch([sidebar::reset_scroll(), print]);
            }
            engine::Event::Imported {
                info,
                first_source,
                count,
                slot,
            } if self.pending_edit_previous.is_some() => {
                let mut next = self.edits.current().clone();
                next.order.insert_sources(first_source, count, slot);
                self.edits.commit(next);
                self.pending_edit_focus = Some(first_source);
                self.pending_edit_selection = (first_source..first_source + count).collect();
                self.external_drop = file_drop::State {
                    settling_doc: Some(info.id),
                    ..file_drop::State::default()
                };
                let task = self.engine_event(engine::Event::Edited(info));
                self.notify(
                    format!("已插入 {count} 页，可撤销；按 Ctrl+S 保存"),
                    Tone::Info,
                );
                return task;
            }
            engine::Event::Imported { .. } => {}
            engine::Event::Edited(doc) if self.pending_edit_previous.is_some() => {
                let focus = self.pending_edit_focus.take();
                let selection = std::mem::take(&mut self.pending_edit_selection);
                let previous = self.pending_edit_previous.take().unwrap();
                let offset = self.organizer.offset;
                let invalidated: Vec<_> = previous
                    .current()
                    .order
                    .as_sources()
                    .iter()
                    .copied()
                    .filter(|source| {
                        rotation(previous.current(), *source)
                            != rotation(self.edits.current(), *source)
                    })
                    .collect();

                self.viewer.set_document_after_edit(
                    doc.clone(),
                    &previous.current().order,
                    &self.edits.current().order,
                    &invalidated,
                );
                self.sidebar.update_document(&doc);
                self.order = self.edits.current().order.clone();
                self.viewer_order = self.edits.current().order.clone();
                self.sidebar.clamp_pages(&self.order, &self.viewer_order);
                self.organizer.set_document(self.viewer.page_count());
                self.organizer.offset = offset;
                self.organizer.clamp(self.viewer.page_count());
                self.document_busy = false;

                let selected: Vec<_> = selection
                    .into_iter()
                    .filter(|source| self.edit_slot(*source).is_some())
                    .collect();
                let focus_slot = focus.and_then(|source| self.edit_slot(source));

                match self.view_mode {
                    ViewMode::Reader => {
                        if let Some(slot) = focus_slot {
                            self.viewer.go_to_page(slot);
                        }
                    }
                    ViewMode::Organizer => {
                        self.organizer.selected.clear();
                        self.organizer.selected.extend(selected.iter().copied());
                        self.organizer.anchor = focus
                            .filter(|source| self.edit_slot(*source).is_some())
                            .or_else(|| selected.first().copied());
                        if let Some(anchor) = self.organizer.anchor {
                            self.organizer.reveal_source(
                                anchor,
                                self.viewer.page_count(),
                                &self.order,
                            );
                        }
                    }
                }

                if let Some(mut animation) = self.pending_edit_animation.take() {
                    animation.restart();
                    self.edit_animation = Some(animation);
                }

                self.refresh_page_input();
                if self.find.open {
                    self.search();
                }
            }
            engine::Event::EditFailed { doc, error } if Some(doc) == current => {
                self.external_drop = file_drop::State::default();
                self.document_busy = false;
                if let Some(previous) = self.pending_edit_previous.take() {
                    self.edits.restore(previous);
                    self.order = self.edits.current().order.clone();
                }
                self.pending_edit_animation = None;
                self.pending_edit_focus = None;
                self.pending_edit_selection.clear();
                self.notify(format!("无法编辑 PDF。{error}"), Tone::Error);
            }
            engine::Event::Edited(_) | engine::Event::EditFailed { .. } => {}
            engine::Event::OpenFailed { path, error } => {
                self.opening = None;

                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();

                self.notify(format!("无法打开 {name}。{error}"), Tone::Error);
            }
            engine::Event::Tile { doc, key, bitmap } if Some(doc) == current => {
                trace::mark("first tile received");
                self.viewer.insert_tile(key, bitmap);
            }
            engine::Event::Thumbnail { doc, page, bitmap } if Some(doc) == current => {
                self.viewer.insert_thumbnail(page, bitmap);
            }
            engine::Event::Text { doc, page, text } if Some(doc) == current => {
                self.viewer.insert_text(page, text);
            }
            engine::Event::Matches {
                doc,
                search,
                matches,
            } if Some(doc) == current && search == self.find.id => {
                self.viewer.add_matches(matches);
            }
            engine::Event::SearchDone {
                doc,
                search,
                capped,
            } if Some(doc) == current && search == self.find.id => {
                trace::note(|| format!("search done: {} matches", self.viewer.match_count()));
                self.find.searching = false;
                self.find.capped = capped;
                self.viewer.finish_search();
            }
            engine::Event::Tile { .. }
            | engine::Event::Thumbnail { .. }
            | engine::Event::Text { .. }
            | engine::Event::Matches { .. }
            | engine::Event::SearchDone { .. } => {}
            engine::Event::Progress { kind, done, total } => {
                let label = match kind {
                    JobKind::Export => "正在导出 Markdown…",
                    JobKind::Print => "正在打印…",
                    JobKind::Save => "正在保存 PDF…",
                    JobKind::Extract => "正在提取页面…",
                    JobKind::Compact => "正在压缩 PDF…",
                    JobKind::Raster => "正在导出页面…",
                };

                self.toast = Some(Toast {
                    text: label.into(),
                    tone: Tone::Info,
                    expires: None,
                    progress: Some(done as f32 / total.max(1) as f32),
                });
            }
            engine::Event::Copied(text) => {
                if !text.is_empty() {
                    self.notify("已复制到剪贴板", Tone::Info);
                    return iced::clipboard::write(text);
                }
            }
            engine::Event::RasterEstimated {
                doc,
                request,
                bytes,
            } => {
                if Some(doc) == current
                    && request == self.estimate_revision
                    && let Some(dialog) = &mut self.dialog
                {
                    dialog.estimate = Some(bytes);
                }
            }
            engine::Event::Exported(path) => {
                self.document_busy = false;
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();

                self.notify(format!("已导出 {name}"), Tone::Info);
            }
            engine::Event::Saved { info, path } if Some(info.id) == current => {
                let selected: Vec<_> = self
                    .organizer
                    .selected
                    .iter()
                    .filter_map(|source| self.edit_slot(*source))
                    .collect();
                let anchor = self
                    .organizer
                    .focused()
                    .and_then(|source| self.edit_slot(source));
                self.viewer.update_document_info(info.clone());
                self.sidebar.update_document(&info);
                self.order.reset(self.viewer.page_count());
                self.edits.reset(self.viewer.page_count());
                self.viewer_order.reset(self.viewer.page_count());
                self.sidebar.clamp_pages(&self.order, &self.viewer_order);
                if self.view_mode == ViewMode::Organizer {
                    self.organizer.selected.clear();
                    self.organizer.selected.extend(selected.iter().copied());
                    self.organizer.anchor = anchor.or_else(|| selected.first().copied());
                }
                self.document_busy = false;
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.notify(format!("已保存 {name}"), Tone::Info);
                if self.leave_after_save
                    && let Some(action) = self.leave_action.take()
                {
                    return self.finish_leave(action);
                }
            }
            engine::Event::Saved { .. } => {}
            engine::Event::Extracted { path, pages } => {
                self.document_busy = false;
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.notify(format!("已提取 {pages} 页至 {name}"), Tone::Info);
            }
            engine::Event::Compacted {
                path,
                before,
                after,
                replaced,
            } => {
                self.document_busy = false;
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let message = if !replaced {
                    format!(
                        "原文件已保留：整理后为 {}，未小于当前文件（{}）",
                        format_bytes(after),
                        format_bytes(before)
                    )
                } else if after < before {
                    let saved = (before - after) as f64 / before as f64 * 100.0;
                    format!(
                        "已压缩 {name}：{} → {}（减小 {saved:.0}%）",
                        format_bytes(before),
                        format_bytes(after)
                    )
                } else {
                    format!(
                        "已保存压缩副本 {name}，大小 {}；未小于当前文件（{}）",
                        format_bytes(after),
                        format_bytes(before)
                    )
                };
                self.notify(message, Tone::Info);
            }
            engine::Event::Printed => self.notify("已发送至打印机", Tone::Info),
            engine::Event::JobFailed { kind, error } => {
                if kind == JobKind::Save {
                    self.leave_action = None;
                    self.leave_after_save = false;
                }
                if matches!(
                    kind,
                    JobKind::Save
                        | JobKind::Extract
                        | JobKind::Compact
                        | JobKind::Raster
                        | JobKind::Export
                ) {
                    self.document_busy = false;
                }
                let what = match kind {
                    JobKind::Export => "导出失败",
                    JobKind::Print => "打印失败",
                    JobKind::Save => "保存失败",
                    JobKind::Extract => "提取失败",
                    JobKind::Compact => "压缩失败",
                    JobKind::Raster => "图片导出失败",
                };

                self.notify(format!("{what}. {error}"), Tone::Error);
            }
        }

        Task::none()
    }

    /// Advances the scroll test by a frame; at its end, reports and quits.
    fn scroll_test_frame(&mut self, now: Instant) -> Task<Message> {
        let frames = self.options.scroll_test.unwrap_or(0) as usize;

        let Some(test) = self.scroll_test.as_mut() else {
            return Task::none();
        };

        test.intervals
            .push(now.duration_since(test.last).as_secs_f32() * 1000.0);
        test.last = now;
        test.incomplete += test.missing as u32;

        if test.intervals.len() <= frames {
            self.viewer.scroll_by(0.0, SCROLL_TEST_STEP);
            return Task::none();
        }

        // The first interval runs from the end of loading, not from a frame.
        let mut intervals = test.intervals.split_off(1);
        let incomplete = test.incomplete;
        let slowest = intervals
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map_or(0, |(frame, _)| frame + 1);

        intervals.sort_by(f32::total_cmp);

        let at =
            |quantile: f32| intervals[((intervals.len() - 1) as f32 * quantile).round() as usize];

        trace::note(|| {
            format!(
                "scroll test: {} frames, interval p50 {:.2} ms, p95 {:.2} ms, p99 {:.2} ms, \
                 max {:.2} ms at frame {slowest}; {incomplete} frames with tiles in view missing",
                intervals.len(),
                at(0.5),
                at(0.95),
                at(0.99),
                at(1.0),
            )
        });
        trace::memory("after scrolling");
        trace::note(|| self.viewer.cache_report());

        // Stay a while, to see what is still held once things have settled.
        self.scroll_test = None;
        self.options.scroll_test = None;
        self.resting_since = Some(Instant::now());

        Task::none()
    }

    fn document_shortcut(&self, key: &Key, modifiers: keyboard::Modifiers) -> Option<Message> {
        if !modifiers.command() {
            return None;
        }

        match key.as_ref() {
            Key::Character("s") if modifiers.shift() => Some(Message::SaveAs),
            Key::Character("s") => Some(Message::Save),
            Key::Character("z") if modifiers.shift() => Some(Message::Redo),
            Key::Character("z") => Some(Message::Undo),
            Key::Character("y") => Some(Message::Redo),
            Key::Character("r") if modifiers.shift() => Some(Message::Rotate(-1)),
            Key::Character("r") => Some(Message::Rotate(1)),
            _ => None,
        }
    }

    fn key_pressed(&mut self, key: Key, modifiers: keyboard::Modifiers) -> Task<Message> {
        if self.dialog.is_some() {
            return if key == Key::Named(Named::Escape) {
                self.handle(Message::DialogClose)
            } else {
                Task::none()
            };
        }
        if let Some(message) = self.document_shortcut(&key, modifiers) {
            return self.handle(message);
        }

        if self.view_mode == ViewMode::Organizer {
            return self.organizer_key_pressed(key, modifiers);
        }

        let command = modifiers.command();
        let step = if modifiers.shift() { -1 } else { 1 };

        match key.as_ref() {
            Key::Character("f") if command => return self.open_find(),
            Key::Character("g") if command && self.find.open => self.viewer.step_match(step),
            Key::Named(Named::F3) if self.find.open => self.viewer.step_match(step),
            Key::Named(Named::F3) => return self.open_find(),
            Key::Named(Named::Enter) if self.find.open => self.viewer.step_match(step),
            Key::Character("o") if command => return self.handle(Message::OpenDialog),
            Key::Character("p") if command => return self.handle(Message::Print),
            Key::Character("e") if command => return self.handle(Message::Export),
            Key::Character("b") if command => {
                self.sidebar.open = !self.sidebar.open;
                self.motion.set_sidebar(self.sidebar.open);
            }
            Key::Character("c") if command => {
                if let Some((from, to, granularity)) = self.viewer.selection() {
                    self.engine.copy(from, to, granularity);
                }
            }
            Key::Character("a") if command => self.viewer.select_all(),
            Key::Character("=" | "+") if command => self.viewer.zoom_step(1),
            Key::Character("-") if command => self.viewer.zoom_step(-1),
            Key::Character("0") if command => self.viewer.zoom_to(1.0, None),
            Key::Character("1") if command => self.viewer.set_fit(Fit::Width),
            Key::Character("2") if command => self.viewer.set_fit(Fit::Page),
            Key::Named(Named::ArrowDown) => self.viewer.scroll_by(0.0, 56.0),
            Key::Named(Named::ArrowUp) => self.viewer.scroll_by(0.0, -56.0),
            Key::Named(Named::ArrowRight) => self.viewer.scroll_by(56.0, 0.0),
            Key::Named(Named::ArrowLeft) => self.viewer.scroll_by(-56.0, 0.0),
            Key::Named(Named::PageDown) => self.viewer.scroll_by_viewport(1.0),
            Key::Named(Named::PageUp) => self.viewer.scroll_by_viewport(-1.0),
            Key::Named(Named::Space) if modifiers.shift() => self.viewer.scroll_by_viewport(-1.0),
            Key::Named(Named::Space) => self.viewer.scroll_by_viewport(1.0),
            Key::Named(Named::Home) => self.viewer.go_to_start(),
            Key::Named(Named::End) => self.viewer.go_to_end(),
            _ => {}
        }

        Task::none()
    }

    fn organizer_key_pressed(&mut self, key: Key, modifiers: keyboard::Modifiers) -> Task<Message> {
        let command = modifiers.command();
        let count = self.viewer.page_count();
        let columns = self.organizer.columns().max(1) as i32;

        match key.as_ref() {
            Key::Character("f") if command => {
                self.set_view(ViewMode::Reader);
                return self.open_find();
            }
            Key::Character("g") if command && self.find.open => {
                self.set_view(ViewMode::Reader);
                self.viewer.step_match(step_match(&modifiers))
            }
            Key::Named(Named::F3) if self.find.open => {
                self.set_view(ViewMode::Reader);
                self.viewer.step_match(step_match(&modifiers))
            }
            Key::Named(Named::F3) => {
                self.set_view(ViewMode::Reader);
                return self.open_find();
            }
            Key::Named(Named::Enter) if self.find.open => {
                self.set_view(ViewMode::Reader);
                self.viewer.step_match(step_match(&modifiers))
            }
            Key::Character("o") if command => return self.handle(Message::OpenDialog),
            Key::Character("p") if command => return self.handle(Message::Print),
            Key::Character("e") if command => return self.handle(Message::Export),
            Key::Character("b") if command => {
                self.sidebar.open = !self.sidebar.open;
                self.motion.set_sidebar(self.sidebar.open);
            }
            Key::Character("a") if command => self.organizer.select_all(count, &self.order),
            Key::Character("1") if command => self.set_view(ViewMode::Reader),
            Key::Character("2") if command => self.set_view(ViewMode::Organizer),
            Key::Character("=" | "+") if command => {
                self.organizer.track =
                    (self.organizer.track + 20.0).clamp(organizer::MIN_TRACK, organizer::MAX_TRACK);
                self.organizer.clamp(count);
            }
            Key::Character("-") if command => {
                self.organizer.track =
                    (self.organizer.track - 20.0).clamp(organizer::MIN_TRACK, organizer::MAX_TRACK);
                self.organizer.clamp(count);
            }
            Key::Character("0") if command => {
                self.organizer.track = organizer::DEFAULT_TRACK;
                self.organizer.clamp(count);
            }
            Key::Named(Named::Delete) => return self.handle(Message::DeletePages),
            Key::Named(Named::ArrowDown) => self.organizer.move_by(columns, count, &self.order),
            Key::Named(Named::ArrowUp) => self.organizer.move_by(-columns, count, &self.order),
            Key::Named(Named::ArrowRight) => self.organizer.move_by(1, count, &self.order),
            Key::Named(Named::ArrowLeft) => self.organizer.move_by(-1, count, &self.order),
            Key::Named(Named::PageDown) => {
                self.organizer
                    .scroll_by(self.organizer.size.height * 0.9, count);
            }
            Key::Named(Named::PageUp) => {
                self.organizer
                    .scroll_by(-self.organizer.size.height * 0.9, count);
            }
            Key::Named(Named::Space) if modifiers.shift() => {
                self.organizer
                    .scroll_by(-self.organizer.size.height * 0.9, count);
            }
            Key::Named(Named::Space) => {
                self.organizer
                    .scroll_by(self.organizer.size.height * 0.9, count);
            }
            Key::Named(Named::Home) => self.organizer.move_by(-(count as i32), count, &self.order),
            Key::Named(Named::End) => self.organizer.move_by(count as i32, count, &self.order),
            Key::Named(Named::Enter) => {
                if let Some(source) = self.organizer.focused()
                    && let Some(page) = self.viewer_slot(source)
                {
                    self.set_view(ViewMode::Reader);
                    self.viewer.go_to_page(page);
                }
            }
            _ => {}
        }

        Task::none()
    }
}

fn step_match(modifiers: &keyboard::Modifiers) -> i8 {
    if modifiers.shift() { -1 } else { 1 }
}

pub(crate) fn format_bytes(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024 * 1024) as f64)
    } else if bytes >= 1024 {
        format!("{:.0} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn rotation(state: &EditState, source: u32) -> u8 {
    state
        .rotations()
        .find_map(|(page, turns)| (page == source).then_some(turns))
        .unwrap_or(0)
}

fn edit_changes(previous: &EditState, next: &EditState) -> (EditKind, Vec<(u32, Option<u32>, i8)>) {
    let deleted: Vec<_> = previous
        .order
        .as_sources()
        .iter()
        .copied()
        .filter(|source| next.order.slot_of(*source).is_none())
        .map(|source| (previous.order.slot_of(source).unwrap_or(0), None, 0))
        .collect();
    if !deleted.is_empty() {
        return (EditKind::Delete, deleted);
    }

    let rotated: Vec<_> = previous
        .order
        .as_sources()
        .iter()
        .copied()
        .filter_map(|source| {
            let old = rotation(previous, source);
            let new = rotation(next, source);
            let delta = (new + 4 - old) % 4;
            let turns = match delta {
                1 => 1,
                2 => 2,
                3 => -1,
                _ => return None,
            };
            Some((
                previous.order.slot_of(source)?,
                next.order.slot_of(source),
                turns,
            ))
        })
        .collect();
    (EditKind::Rotate, rotated)
}

/// Shows a newly loaded document: from the top, or the first time, the way
/// the command line asked.
fn present(
    viewer: &mut Viewer,
    sidebar: &mut sidebar::State,
    options: &mut Options,
    doc: Arc<DocInfo>,
) {
    sidebar.set_document(&doc);
    sidebar.show(sidebar::Tab::Pages);
    viewer.set_document(doc);

    match options.sidebar.take().as_deref() {
        Some("pages") => sidebar.show(sidebar::Tab::Pages),
        Some("outline") => sidebar.show(sidebar::Tab::Outline),
        _ => {}
    }

    if options.select_all {
        viewer.select_all();
    }

    if let Some(zoom) = options.zoom.take() {
        viewer.zoom_to(zoom, None);
    }

    if let Some(page) = options.page.take() {
        viewer.go_to_page(page.saturating_sub(1));
    }
}

/// Pages whose thumbnails the sidebar is showing.
fn sidebar_thumbnails(
    sidebar: &sidebar::State,
    order: &PageOrder,
    document_order: &PageOrder,
) -> Vec<u32> {
    if sidebar.open && sidebar.tab == sidebar::Tab::Pages {
        sidebar
            .visible_page_slots(order, document_order)
            .filter_map(|slot| document_order.slot_of(order.source_at(slot as u32)))
            .collect()
    } else {
        Vec::new()
    }
}

fn reader_wanted(viewer: &mut Viewer, sidebar_pages: &[u32]) -> engine::Wanted {
    let mut wanted = viewer.wanted(0..0);
    let thumbnails = viewer.wanted_thumbnails(sidebar_pages);

    for page in thumbnails.thumbnails {
        if !wanted.thumbnails.contains(&page) {
            wanted.thumbnails.push(page);
        }
    }

    wanted
}

/// Escape, whether or not a widget took it: a focused text field does, to
/// drop its focus, and the key should still close the find bar.
fn escape(
    event: iced::Event,
    _status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(Named::Escape),
            ..
        }) => Some(Message::Escape),
        iced::Event::Keyboard(keyboard::Event::KeyPressed {
            key: Key::Named(Named::Tab),
            modifiers,
            repeat: false,
            ..
        }) if modifiers.is_empty() => Some(Message::ToggleImmersive),
        _ => None,
    }
}

/// The window events the app acts on. The rest are left out on purpose:
/// every message rebuilds the view and redraws, and Windows reports a move
/// for each step of dragging the window, which made the drag stutter.
fn window_event(
    event: iced::Event,
    _status: iced::event::Status,
    _window: window::Id,
) -> Option<Message> {
    match event {
        iced::Event::Window(window::Event::CloseRequested) => {
            Some(Message::WindowAction(WindowAction::Close))
        }
        iced::Event::Window(window::Event::FileDropped(path)) => Some(Message::FileDropped(path)),
        iced::Event::Window(window::Event::FileHovered(path)) => Some(Message::FileHovered(path)),
        iced::Event::Window(window::Event::FilesHoveredLeft) => Some(Message::FileHoverLeft),
        iced::Event::Window(window::Event::Opened { size, .. } | window::Event::Resized(size)) => {
            Some(Message::WindowSize(size))
        }
        iced::Event::Window(window::Event::Rescaled(scale_factor)) => {
            Some(Message::ScaleFactor(scale_factor))
        }
        _ => None,
    }
}

fn external_position_task(drop: Option<PathBuf>) -> Task<Message> {
    window::oldest().and_then(move |id| {
        let drop = drop.clone();
        window::run(id, native_drop_position).map(move |point| {
            if let Some(path) = &drop {
                Message::DropAt {
                    path: path.clone(),
                    point,
                }
            } else {
                Message::ExternalPosition(point)
            }
        })
    })
}

fn native_drop_position(window: &dyn window::Window) -> Option<Point> {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{HWND, POINT};
        use windows::Win32::Graphics::Gdi::ScreenToClient;
        use windows::Win32::UI::HiDpi::GetDpiForWindow;
        use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;
        let handle = window_handle(window);
        if handle == 0 {
            return None;
        }
        let hwnd = HWND(handle as *mut _);
        let mut point = POINT::default();
        // The window handle is borrowed from Iced; convert physical screen pixels to logical client coordinates.
        unsafe {
            GetCursorPos(&mut point).ok()?;
            if !ScreenToClient(hwnd, &mut point).as_bool() {
                return None;
            }
            let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
            Some(Point::new(point.x as f32 / scale, point.y as f32 / scale))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        None
    }
}

fn window_border_color(palette: Palette) -> u32 {
    let red = (palette.border.r * 255.0).round() as u32;
    let green = (palette.border.g * 255.0).round() as u32;
    let blue = (palette.border.b * 255.0).round() as u32;

    red | (green << 8) | (blue << 16)
}

fn shape_window_task(border_color: u32) -> Task<Message> {
    window::oldest()
        .and_then(move |id| window::run(id, move |window| shape_window(window, border_color)))
        .discard()
}

fn shape_window(window: &dyn window::Window, border_color: u32) {
    #[cfg(windows)]
    {
        use std::ffi::c_void;

        use windows::Win32::Foundation::HWND;
        use windows::Win32::Graphics::Dwm::{
            DWMWA_BORDER_COLOR, DWMWA_VISIBLE_FRAME_BORDER_THICKNESS, DwmSetWindowAttribute,
        };

        let hwnd = HWND(window_handle(window) as *mut c_void);
        let border_thickness = 1_u32;

        install_frame_subclass(hwnd);

        // SAFETY: the handle belongs to the live window passed by Iced.
        unsafe {
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_BORDER_COLOR,
                (&border_color as *const u32).cast(),
                std::mem::size_of::<u32>() as u32,
            );
            let _ = DwmSetWindowAttribute(
                hwnd,
                DWMWA_VISIBLE_FRAME_BORDER_THICKNESS,
                (&border_thickness as *const u32).cast(),
                std::mem::size_of::<u32>() as u32,
            );
        }
    }

    #[cfg(target_os = "macos")]
    {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        use window::raw_window_handle::{AppKitWindowHandle, RawWindowHandle};

        let _ = border_color;
        let RawWindowHandle::AppKit(AppKitWindowHandle { ns_view, .. }) = window
            .window_handle()
            .map(|handle| handle.as_raw())
            .unwrap()
        else {
            return;
        };

        // SAFETY: Iced passes the live main-thread NSView.
        unsafe {
            let view = ns_view.as_ptr().cast::<AnyObject>();
            let _: () = msg_send![view, setWantsLayer: true];
            let layer: *mut AnyObject = msg_send![view, layer];
            if !layer.is_null() {
                let _: () = msg_send![layer, setCornerRadius: 10.0_f64];
                let _: () = msg_send![layer, setMasksToBounds: true];
            }
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    let _ = (window, border_color);
}

#[cfg(windows)]
const FRAME_SUBCLASS_ID: usize = 1;
#[cfg(windows)]
const RESIZE_EDGE: f32 = 6.0;
#[cfg(windows)]
const RESIZE_CORNER: f32 = 18.0;
#[cfg(windows)]
static FRAME_SUBCLASS_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);

#[cfg(windows)]
fn install_frame_subclass(hwnd: windows::Win32::Foundation::HWND) {
    use std::sync::atomic::Ordering;

    use windows::Win32::UI::Shell::SetWindowSubclass;

    let handle = hwnd.0 as isize;
    if FRAME_SUBCLASS_HWND.load(Ordering::Acquire) == handle {
        return;
    }

    // SAFETY: the callback and subclass id are static, and the handle is live.
    if unsafe { SetWindowSubclass(hwnd, Some(frame_subclass), FRAME_SUBCLASS_ID, 0) }.as_bool() {
        FRAME_SUBCLASS_HWND.store(handle, Ordering::Release);
    }
}

#[cfg(windows)]
unsafe extern "system" fn frame_subclass(
    hwnd: windows::Win32::Foundation::HWND,
    message: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    _subclass_id: usize,
    _reference_data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::{LRESULT, POINT, RECT};
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Shell::DefSubclassProc;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, HTCLIENT, IsZoomed, WM_NCDESTROY, WM_NCHITTEST,
    };

    if message == WM_NCHITTEST {
        let mut rect = RECT::default();

        // SAFETY: the handle belongs to the subclassed window.
        if unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok() && !unsafe { IsZoomed(hwnd) }.as_bool()
        {
            let packed = lparam.0 as u32;
            let point = POINT {
                x: (packed & 0xffff) as i16 as i32,
                y: ((packed >> 16) & 0xffff) as i16 as i32,
            };
            let dpi = unsafe { GetDpiForWindow(hwnd) };
            let scale = if dpi == 0 { 1.0 } else { dpi as f32 / 96.0 };
            let width = (rect.right - rect.left) as f32 / scale;
            let hit = frame_hit_test(rect, point, scale, theme::edge(width));

            if hit != HTCLIENT {
                return LRESULT(hit as isize);
            }
        }
    }

    if message == WM_NCDESTROY {
        use std::sync::atomic::Ordering;

        let _ = FRAME_SUBCLASS_HWND.compare_exchange(
            hwnd.0 as isize,
            0,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    // SAFETY: this is the required fallback for a window subclass.
    unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
}

#[cfg(windows)]
fn frame_hit_test(
    rect: windows::Win32::Foundation::RECT,
    point: windows::Win32::Foundation::POINT,
    scale: f32,
    drag_height: f32,
) -> u32 {
    use windows::Win32::UI::WindowsAndMessaging::{
        HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP,
        HTTOPLEFT, HTTOPRIGHT,
    };

    let edge = (RESIZE_EDGE * scale).round().max(1.0) as i32;
    let corner = (RESIZE_CORNER * scale).round().max(edge as f32) as i32;
    let x = point.x - rect.left;
    let y = point.y - rect.top;
    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    let left = x < edge;
    let right = x >= width - edge;
    let top = y < edge;
    let bottom = y >= height - edge;
    let top_corner = y < corner;
    let bottom_corner = y >= height - corner;
    let left_corner = x < corner;
    let right_corner = x >= width - corner;

    if left_corner && top_corner {
        HTTOPLEFT
    } else if right_corner && top_corner {
        HTTOPRIGHT
    } else if left_corner && bottom_corner {
        HTBOTTOMLEFT
    } else if right_corner && bottom_corner {
        HTBOTTOMRIGHT
    } else if left {
        HTLEFT
    } else if right {
        HTRIGHT
    } else if top {
        HTTOP
    } else if bottom {
        HTBOTTOM
    } else if y >= 0 && y < (drag_height * scale).round() as i32 {
        HTCAPTION
    } else {
        HTCLIENT
    }
}

/// The native handle the print dialog should be owned by.
fn window_handle(window: &dyn window::Window) -> isize {
    use window::raw_window_handle::RawWindowHandle;

    match window.window_handle().map(|handle| handle.as_raw()) {
        Ok(RawWindowHandle::Win32(handle)) => handle.hwnd.get(),
        _ => 0,
    }
}

/// The default executor starts a thread per core. All that runs on it here
/// is a timer and the wait for a dialog to close, so one thread will do.
struct Executor(tokio::runtime::Runtime);

impl iced::Executor for Executor {
    fn new() -> Result<Self, std::io::Error> {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .map(Self)
    }

    fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
        drop(self.0.spawn(future));
    }

    fn block_on<T>(&self, future: impl Future<Output = T>) -> T {
        self.0.block_on(future)
    }

    fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.enter();
        f()
    }
}

fn window_icon() -> Option<window::Icon> {
    let icon = image::load_from_memory_with_format(
        include_bytes!("../assets/icon/folio.png"),
        image::ImageFormat::Png,
    )
    .ok()?
    .into_rgba8();

    let (width, height) = icon.dimensions();

    window::icon::from_rgba(icon.into_raw(), width, height).ok()
}

pub fn run(options: Options) -> iced::Result {
    let size = options.window_size.unwrap_or(WINDOW);

    iced::application(move || App::boot(options.clone()), App::update, App::view)
        .executor::<Executor>()
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .settings(iced::Settings {
            default_text_size: ui::theme::TEXT.into(),
            default_font: ui::font(),
            ..iced::Settings::default()
        })
        .window(window::Settings {
            size,
            min_size: Some(Size::new(560.0, 380.0)),
            decorations: false,
            transparent: cfg!(target_os = "macos"),
            exit_on_close_request: false,
            icon: window_icon(),
            #[cfg(windows)]
            platform_specific: window::settings::PlatformSpecific {
                undecorated_shadow: true,
                corner_preference: window::settings::platform::CornerPreference::Round,
                ..window::settings::PlatformSpecific::default()
            },
            ..window::Settings::default()
        })
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::tests::fixture_path;
    use crate::ui::viewer::tests::on_screen;

    /// Runs updates until `done`, as the engine's wake-ups would.
    fn settle(app: &mut App, done: impl Fn(&App) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(20);

        while !done(app) {
            assert!(Instant::now() < deadline, "the view never settled");

            std::thread::sleep(Duration::from_millis(2));
            let _ = app.update(Message::Engine);
        }
    }

    fn settle_thumbnails(app: &mut App) {
        settle(app, |app| {
            (0..app.viewer.page_count()).all(|page| app.viewer.thumbnail(page).is_some())
        });
    }

    fn seed_thumbnails(app: &mut App) {
        for page in 0..app.viewer.page_count() {
            app.viewer.insert_thumbnail(
                page,
                engine::Bitmap {
                    width: 1,
                    height: 1,
                    rgba: vec![page as u8 * 40, 99, 77, 255],
                },
            );
        }
    }

    fn assert_thumbnail_source(app: &App, slot: u32, source: u32) {
        let iced_image::Handle::Rgba { pixels, .. } = app.viewer.thumbnail(slot).unwrap() else {
            panic!("expected rgba pixels");
        };
        assert_eq!(pixels[0], source as u8 * 40);
    }

    #[test]
    fn reading_record_restores_through_the_app_open_lifecycle() {
        let record_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../TEMP/reading-app-roundtrip-{}.json",
            std::process::id()
        ));
        let (mut first, _) = App::boot(Options::default());
        first.remember_reading = true;
        first.reading_path = Some(record_path.clone());
        first.open(fixture_path());
        settle(&mut first, |app| app.viewer.document().is_some());
        let _ = first.update(Message::GoTo(1));
        let _ = first.update(Message::Viewer(Interaction::Panned { dx: 0.0, dy: 150.0 }));
        let _ = first.update(Message::Tick(Instant::now() + Duration::from_secs(5)));
        let expected = first.viewer.reading_position();
        assert!(
            !record_path.exists(),
            "Reading operations must not write a record"
        );
        let _ = first.update(Message::WindowAction(WindowAction::Close));
        assert_eq!(
            crate::reading::Store::load(&record_path).get(&fixture_path()),
            Some(expected.clone())
        );
        let (mut reopened, _) = App::boot(Options::default());
        reopened.remember_reading = true;
        reopened.reading_path = Some(record_path);
        reopened.open(fixture_path());
        settle(&mut reopened, |app| app.viewer.document().is_some());
        assert_eq!(reopened.viewer.reading_position().page, expected.page);
        assert!((reopened.viewer.reading_position().top - expected.top).abs() < 0.1);
        for size in [Size::new(900.0, 700.0), Size::new(1100.0, 850.0)] {
            let _ = reopened.update(Message::Viewer(Interaction::Resized(size)));
            assert_eq!(reopened.viewer.reading_position().page, expected.page);
        }
    }

    #[test]
    fn user_reading_record_probe_when_requested() {
        if std::env::var_os("FOLIO_READING_PROBE").is_none() {
            return;
        }
        let dir = crate::preferences::directory().unwrap();
        let path = dir.join("reading.json");
        let raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        for entry in raw["entries"].as_array().unwrap() {
            let file = PathBuf::from(entry["path"].as_str().unwrap());
            let saved = crate::reading::Store::load(&path)
                .get(&file)
                .expect("Existing user record must match its PDF");
            let (mut app, _) = App::boot(Options::default());
            app.remember_reading = true;
            app.reading_path = Some(path.clone());
            app.reading_suppressed = true;
            app.open(file);
            app.reading_suppressed = true;
            settle(&mut app, |app| app.viewer.document().is_some());
            assert_eq!(app.viewer.reading_position().page, saved.page);
            assert!((app.viewer.reading_position().top - saved.top).abs() < 1.0);
            for size in [Size::new(1100.0, 850.0), Size::new(800.0, 600.0)] {
                let _ = app.update(Message::Viewer(Interaction::Resized(size)));
                assert_eq!(app.viewer.reading_position().page, saved.page);
            }
            println!(
                "User record restored at page {}, offset {}",
                saved.page + 1,
                saved.top
            );
        }
    }

    #[test]
    fn settings_and_file_menu_work_without_a_document() {
        let (mut app, _) = App::boot(Options::default());
        let _ = app.update(Message::ToggleMore);
        assert!(app.more_open);
        let _ = app.update(Message::ShowSettings);
        assert!(!app.more_open);
        assert_eq!(app.dialog.as_ref().unwrap().kind, dialog::Kind::Settings);
        let _ = app.update(Message::DialogConfirm);
        assert!(!app.document_busy());
        let _ = app.update(Message::ShowAbout);
        assert_eq!(app.dialog.as_ref().unwrap().kind, dialog::Kind::About);
    }

    #[test]
    fn middle_pan_takes_over_animations_tracks_the_mouse_and_clamps_each_axis() {
        for double in [false, true] {
            let (mut app, _) = App::boot(Options {
                sidebar: Some("hidden".into()),
                ..Options::default()
            });
            app.viewer.set_document(Arc::new(engine::DocInfo {
                id: 1,
                path: PathBuf::new(),
                title: String::new(),
                pages: vec![
                    engine::PageSize {
                        width: 612.0,
                        height: 792.0
                    };
                    30
                ],
                outline: Vec::new(),
            }));
            app.order.reset(30);
            app.viewer_order.reset(30);
            let _ = app.update(Message::SetReadingLayout(double));
            app.viewer.zoom_to(3.0, None);
            app.viewer.scroll_by(500.0, 500.0);
            app.viewer.animate_to_page(20);
            let _ = app.update(Message::Viewer(Interaction::Panned { dx: 0.0, dy: 0.0 }));
            assert!(!app.viewer.scrolling());
            assert!(app.view_transition.is_none());
            let before = app.viewer.page_rect(0);
            let _ = app.update(Message::Viewer(Interaction::Panned {
                dx: -100.0,
                dy: -80.0,
            }));
            let after = app.viewer.page_rect(0);
            assert!((after.x - before.x - 100.0).abs() < 1.0);
            assert!((after.y - before.y - 80.0).abs() < 1.0);
            app.viewer.set_fit(Fit::Width);
            let before = app.viewer.page_rect(0);
            let _ = app.update(Message::Viewer(Interaction::Panned {
                dx: 100000.0,
                dy: -100000.0,
            }));
            let at_top = app.viewer.page_rect(0);
            assert_eq!(at_top.x, before.x, "a fitting horizontal axis stays still");
            assert!(at_top.y >= 0.0, "panning stops at the top boundary");
        }
    }

    #[test]
    fn thumbnail_click_scrolls_in_reading_but_keeps_magic_move_from_organizer() {
        for double in [false, true] {
            let (mut app, _) = App::boot(Options {
                sidebar: Some("hidden".into()),
                ..Options::default()
            });
            app.viewer.set_document(Arc::new(engine::DocInfo {
                id: 1,
                path: PathBuf::new(),
                title: String::new(),
                pages: vec![
                    engine::PageSize {
                        width: 612.0,
                        height: 792.0
                    };
                    20
                ],
                outline: Vec::new(),
            }));
            app.order.reset(20);
            app.viewer_order.reset(20);
            app.organizer.set_document(20);
            let _ = app.update(Message::SetReadingLayout(double));
            app.viewer.go_to_page(0);
            let _ = app.update(Message::SidebarPageClicked { source: 12 });
            assert_eq!(app.view_mode, ViewMode::Reader);
            assert_eq!(app.viewer.is_double_page(), double);
            assert_eq!(
                app.viewer.current_page(),
                0,
                "the initial frame stays in place"
            );
            assert!(app.viewer.scrolling());
            assert!(app.view_transition.is_none());
            let _ = app.update(Message::Animate(
                Instant::now() + Duration::from_millis(500),
            ));
            assert_eq!(app.viewer.current_page(), 12);
            assert!(!app.viewer.scrolling());
            let _ = app.update(Message::SetView(ViewMode::Organizer));
            let _ = app.update(Message::ToggleSidebar);
            let _ = app.update(Message::SidebarPageClicked { source: 3 });
            assert_eq!(app.view_mode, ViewMode::Reader);
            assert_eq!(app.viewer.is_double_page(), double);
            assert_eq!(app.viewer.current_page(), 3);
            assert!(!app.viewer.scrolling());
            assert!(app.view_transition.is_some());
        }
    }

    #[test]
    fn immersive_toggle_restores_sidebar_and_toasts_in_both_views() {
        let (mut app, _) = App::boot(Options::default());
        for mode in [ViewMode::Reader, ViewMode::Organizer] {
            app.set_view(mode);
            let sidebar = app.sidebar.open;
            let _ = app.update(Message::ToggleImmersive);
            assert!(app.immersive);
            assert!(app.toast.as_ref().unwrap().text.contains("已进入沉浸模式"));
            assert_eq!(app.sidebar.open, sidebar);
            let _ = app.update(Message::ToggleImmersive);
            assert!(!app.immersive);
            assert!(app.toast.as_ref().unwrap().text.contains("已退出沉浸模式"));
            assert_eq!(app.sidebar.open, sidebar);
            assert_eq!(app.view_mode, mode);
        }
        let _ = app.update(Message::SetReadingLayout(true));
        assert_eq!(app.view_mode, ViewMode::Reader);
        assert!(app.viewer.is_double_page());
        let _ = app.update(Message::SetReadingLayout(false));
        assert!(!app.viewer.is_double_page());
    }

    #[test]
    fn v038_transition_snapshots_when_requested() {
        if std::env::var_os("FOLIO_V038_SNAPSHOTS").is_none() {
            return;
        }
        let (mut app, _) = App::boot(Options {
            file: Some(fixture_path()),
            sidebar: Some("hidden".into()),
            window_size: Some(Size::new(1360.0, 870.0)),
            ..Options::default()
        });
        settle(&mut app, |app| app.document().is_some());
        app.change_view(ViewMode::Organizer, None);
        app.view_transition = None;
        app.sync();
        settle_thumbnails(&mut app);
        app.palette = Palette::DARK;
        let directory = format!("target/v038-reviewed/{}", std::process::id());
        std::fs::create_dir_all(&directory).unwrap();
        for (from, to) in [(0, 1), (1, 0), (0, 2), (2, 0), (1, 2), (2, 1)] {
            let mode = |value| {
                if value == 2 {
                    ViewMode::Organizer
                } else {
                    ViewMode::Reader
                }
            };
            app.change_view(mode(from), (from != 2).then_some(from == 1));
            app.view_transition = None;
            app.motion.view = Tween::new(1.0);
            app.viewer.go_to_start();
            app.sync();
            settle(&mut app, |app| {
                app.view_mode == ViewMode::Organizer || app.viewer.is_complete()
            });
            app.change_view(mode(to), (to != 2).then_some(to == 1));
            app.sync();
            settle(&mut app, |app| {
                app.view_mode == ViewMode::Organizer || app.viewer.is_complete()
            });
            let start = app.view_transition.as_ref().unwrap().progress.started;
            for elapsed in [0, 115, 230, 345, 500] {
                if elapsed != 0 {
                    let _ = app.handle(Message::Animate(start + Duration::from_millis(elapsed)));
                }
                let mut simulator = iced_test::Simulator::with_size(
                    iced::Settings {
                        default_font: ui::font(),
                        ..iced::Settings::default()
                    },
                    app.window_size,
                    ui::view(&app),
                );
                simulator
                    .snapshot(&app.theme())
                    .unwrap()
                    .matches_image(format!("{directory}/{from}-{to}-{elapsed}.png"))
                    .unwrap();
            }
        }
    }

    #[test]
    fn v037_visual_snapshots_when_requested() {
        if std::env::var_os("FOLIO_V037_SNAPSHOTS").is_none() {
            return;
        }
        let (mut app, _) = App::boot(Options {
            file: Some(fixture_path()),
            sidebar: Some("hidden".into()),
            ..Options::default()
        });
        settle(&mut app, |app| app.document().is_some());
        app.palette = Palette::DARK;
        let directory = format!("target/v037-reviewed/{}", std::process::id());
        std::fs::create_dir_all(&directory).unwrap();
        for (name, mode, double, immersive) in [
            ("single", ViewMode::Reader, false, false),
            ("double", ViewMode::Reader, true, false),
            ("immersive", ViewMode::Reader, true, true),
            ("organizer", ViewMode::Organizer, true, false),
        ] {
            app.set_view(mode);
            app.viewer.set_double_page(double);
            app.immersive = false;
            app.toast = None;
            if immersive {
                let _ = app.update(Message::ToggleImmersive);
            }
            app.motion.view = Tween::new(1.0);
            app.view_transition = None;
            app.viewer.set_viewport(if immersive {
                Size::new(1360.0, 870.0)
            } else {
                ui::page_view_size(Size::new(1360.0, 870.0), 0.0)
            });
            app.organizer.set_size(
                Size::new(1360.0, 768.0),
                app.viewer.page_count(),
                &app.order,
            );
            app.sync();
            settle(&mut app, |app| {
                if mode == ViewMode::Reader {
                    app.viewer.is_complete()
                } else {
                    (0..app.viewer.page_count()).all(|page| app.viewer.thumbnail(page).is_some())
                }
            });
            let mut simulator = iced_test::Simulator::with_size(
                iced::Settings {
                    default_font: ui::font(),
                    ..iced::Settings::default()
                },
                Size::new(1360.0, 870.0),
                ui::view(&app),
            );
            simulator
                .snapshot(&app.theme())
                .unwrap()
                .matches_image(format!("{directory}/{name}.png"))
                .unwrap();
        }
    }

    #[test]
    fn external_drop_preview_snapshots_when_requested() {
        if std::env::var_os("FOLIO_IMPORT_SNAPSHOTS").is_none() {
            return;
        }
        let (mut app, _) = App::boot(Options {
            file: Some(fixture_path()),
            ..Options::default()
        });
        settle(&mut app, |app| app.document().is_some());
        settle_thumbnails(&mut app);
        for (zone, mode) in [
            (file_drop::Zone::Sidebar, ViewMode::Reader),
            (file_drop::Zone::Organizer, ViewMode::Organizer),
        ] {
            let _ = app.update(Message::SetView(mode));
            app.motion.view = Tween::new(1.0);
            app.motion.sidebar.value = if zone == file_drop::Zone::Sidebar {
                1.0
            } else {
                0.0
            };
            app.external_drop = file_drop::State {
                path: Some(PathBuf::from("example.png")),
                hovering: true,
                target: Some(file_drop::Target { zone, slot: 1 }),
                ..file_drop::State::default()
            };
            let mut ui = iced_test::Simulator::with_size(
                iced::Settings {
                    default_font: ui::font(),
                    ..iced::Settings::default()
                },
                Size::new(1360.0, 870.0),
                ui::view(&app),
            );
            ui.snapshot(&app.theme())
                .unwrap()
                .matches_image(format!("target/import-preview-reviewed/{zone:?}.png"))
                .unwrap();
        }
    }

    #[test]
    fn switching_documents_offers_cancel_or_save_before_continuing() {
        let path = std::env::temp_dir().join(format!("folio-leave-{}.pdf", std::process::id()));
        std::fs::copy(fixture_path(), &path).unwrap();
        let (mut app, _) = App::boot(Options {
            file: Some(path.clone()),
            ..Options::default()
        });
        settle(&mut app, |app| app.document().is_some());
        let _ = app.update(Message::Rotate(1));
        settle(&mut app, |app| !app.document_busy);
        assert!(app.edits.dirty());
        let _ = app.update(Message::Open(Some(fixture_path())));
        assert_eq!(app.dialog.as_ref().unwrap().kind, dialog::Kind::Leave);
        assert_eq!(app.document().unwrap().path, path);
        let _ = app.update(Message::DialogClose);
        assert!(app.leave_action.is_none());
        assert!(app.edits.dirty());
        let _ = app.update(Message::Open(Some(fixture_path())));
        let _ = app.update(Message::LeaveChoice(LeaveChoice::Save));
        settle(&mut app, |app| {
            app.opening.is_none() && app.document().is_some_and(|doc| doc.path == fixture_path())
        });
        assert!(!app.edits.dirty());
        let saved = crate::engine::tests::open_path(path.clone());
        assert_eq!(
            saved.doc.pages[0],
            engine::PageSize {
                width: 792.0,
                height: 612.0
            }
        );
        drop(saved);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn external_image_drop_previews_then_inserts_and_undoes_as_one_edit() {
        let (mut app, _) = App::boot(Options {
            file: Some(fixture_path()),
            ..Options::default()
        });
        settle(&mut app, |app| app.document().is_some());
        seed_thumbnails(&mut app);
        let bounds = Rectangle::new(Point::new(20.0, 158.0), Size::new(222.0, 500.0));
        let _ = app.update(Message::SidebarPagesFrame {
            size: bounds.size(),
            bounds,
            animating: false,
        });
        let image_path =
            std::env::temp_dir().join(format!("folio-app-drop-{}.png", std::process::id()));
        image::RgbImage::from_pixel(100, 60, image::Rgb([20, 100, 210]))
            .save(&image_path)
            .unwrap();
        let id = app.document().unwrap().id;
        let point = Point::new(100.0, 160.0);
        let _ = app.handle(Message::FileHovered(image_path.clone()));
        let _ = app.handle(Message::ExternalPosition(Some(point)));
        assert_eq!(
            app.external_drop.target,
            Some(file_drop::Target {
                zone: file_drop::Zone::Sidebar,
                slot: 0
            })
        );
        assert_eq!(app.document().unwrap().id, id);
        assert!(!app.edits.can_undo());
        assert_thumbnail_source(&app, 0, 0);
        let _ = app.update(Message::DropAt {
            path: image_path.clone(),
            point: Some(point),
        });
        settle(&mut app, |app| !app.document_busy);
        assert_eq!(app.viewer.page_count(), 4);
        assert_eq!(app.order.as_sources(), &[3, 0, 1, 2]);
        assert!(app.edits.dirty());
        assert_thumbnail_source(&app, 1, 0);
        let _ = app.update(Message::Undo);
        settle(&mut app, |app| !app.document_busy);
        assert_eq!(app.order.as_sources(), &[0, 1, 2]);
        assert!(!app.edits.dirty());
        assert!(!app.edits.can_undo());
        let _ = app.update(Message::Redo);
        settle(&mut app, |app| !app.document_busy);
        assert_eq!(app.viewer.page_count(), 4);
        assert!(app.edits.dirty());
        std::fs::remove_file(image_path).unwrap();
    }

    #[test]
    fn failed_external_drop_preserves_document_and_history() {
        let (mut app, _) = App::boot(Options {
            file: Some(fixture_path()),
            organizer: true,
            ..Options::default()
        });
        settle(&mut app, |app| app.document().is_some());
        let bounds = Rectangle::new(Point::new(0.0, 102.0), Size::new(1360.0, 700.0));
        let _ = app.update(Message::OrganizerFrame {
            size: bounds.size(),
            bounds,
            animating: false,
        });
        let original = app.document().unwrap().id;
        let path =
            std::env::temp_dir().join(format!("folio-broken-drop-{}.pdf", std::process::id()));
        std::fs::write(&path, b"not a pdf").unwrap();
        let _ = app.update(Message::DropAt {
            path: path.clone(),
            point: Some(Point::new(200.0, 200.0)),
        });
        settle(&mut app, |app| !app.document_busy);
        assert_eq!(app.document().unwrap().id, original);
        assert_eq!(app.viewer.page_count(), 3);
        assert!(!app.edits.can_undo());
        assert!(!app.edits.dirty());
        assert!(app.external_drop.target.is_none());
        assert!(matches!(app.toast.as_ref().unwrap().tone, Tone::Error));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn dialogs_block_edit_shortcuts_and_ignore_stale_results() {
        let (mut app, _) = App::boot(Options {
            file: Some(fixture_path()),
            ..Options::default()
        });
        settle(&mut app, |app| app.document().is_some());
        app.show_dialog(dialog::Kind::Compress);
        let serial = app.dialog_serial;
        let doc = app.document().unwrap().id;
        let _ = app.key_pressed(Key::Character("r".into()), keyboard::Modifiers::COMMAND);
        assert!(!app.edits.dirty());
        assert!(!app.document_busy);
        app.estimate_revision = 20;
        let _ = app.engine_event(engine::Event::RasterEstimated {
            doc,
            request: 19,
            bytes: Ok(123),
        });
        assert!(app.dialog.as_ref().unwrap().estimate.is_none());
        let _ = app.engine_event(engine::Event::RasterEstimated {
            doc,
            request: 20,
            bytes: Ok(456),
        });
        assert!(matches!(
            app.dialog.as_ref().unwrap().estimate,
            Some(Ok(456))
        ));
        let _ = app.handle(Message::DialogClose);
        let _ = app.handle(Message::DialogDestination {
            doc,
            serial,
            operation: dialog::Operation::Markdown,
            path: Some(PathBuf::from("must-not-be-written.md")),
        });
        assert!(!app.document_busy);
        let end = app.dialog_motion.started + Duration::from_millis(250);
        let _ = app.handle(Message::Animate(end));
        assert!(app.dialog.is_none());
        assert!(!app.dialog_motion.is_active(end));
    }

    #[test]
    fn document_shortcuts_cover_save_and_history() {
        let (app, _) = App::boot(Options::default());
        let command = keyboard::Modifiers::COMMAND;
        let command_shift = command | keyboard::Modifiers::SHIFT;

        assert!(matches!(
            app.document_shortcut(&Key::Character("s".into()), command),
            Some(Message::Save)
        ));
        assert!(matches!(
            app.document_shortcut(&Key::Character("s".into()), command_shift),
            Some(Message::SaveAs)
        ));
        assert!(matches!(
            app.document_shortcut(&Key::Character("z".into()), command),
            Some(Message::Undo)
        ));
        assert!(matches!(
            app.document_shortcut(&Key::Character("z".into()), command_shift),
            Some(Message::Redo)
        ));
        assert!(matches!(
            app.document_shortcut(&Key::Character("r".into()), command),
            Some(Message::Rotate(1))
        ));
        assert!(matches!(
            app.document_shortcut(&Key::Character("r".into()), command_shift),
            Some(Message::Rotate(-1))
        ));
    }

    #[test]
    fn edits_undo_redo_and_save_through_the_app() {
        let (mut app, _) = App::boot(Options {
            file: Some(fixture_path()),
            ..Options::default()
        });
        settle(&mut app, |app| {
            app.document().is_some() && app.viewer.is_complete()
        });
        let opened = app.document().unwrap().id;
        app.sidebar.tab = sidebar::Tab::Pages;
        settle_thumbnails(&mut app);
        seed_thumbnails(&mut app);

        let _ = app.handle(Message::Rotate(1));
        settle(&mut app, |app| {
            !app.document_busy() && app.document().is_some_and(|doc| doc.id != opened)
        });
        assert!(app.edits.dirty());
        assert!(app.edit_animation.is_some());
        assert_eq!(app.sidebar.tab, sidebar::Tab::Pages);
        assert!(app.viewer.thumbnail(0).is_none());
        assert_thumbnail_source(&app, 1, 1);
        assert_thumbnail_source(&app, 2, 2);
        let rotated = app.document().unwrap().id;

        let _ = app.handle(Message::Undo);
        settle(&mut app, |app| {
            !app.document_busy() && app.document().is_some_and(|doc| doc.id != rotated)
        });
        assert!(!app.edits.dirty());

        let restored = app.document().unwrap().id;
        let _ = app.handle(Message::Redo);
        settle(&mut app, |app| {
            !app.document_busy() && app.document().is_some_and(|doc| doc.id != restored)
        });
        assert!(app.edits.dirty());
        app.sidebar.show(sidebar::Tab::Outline);

        let saved = std::env::temp_dir().join(format!("folio-app-save-{}.pdf", std::process::id()));
        let _ = app.handle(Message::SaveTo(Some(saved)));
        settle(&mut app, |app| {
            !app.document_busy()
                && app
                    .toast
                    .as_ref()
                    .is_some_and(|toast| toast.text.starts_with("已保存"))
        });
        assert!(!app.edits.dirty());
        assert_eq!(app.viewer.page_count(), 3);
        assert_eq!(app.sidebar.tab, sidebar::Tab::Outline);

        let before_delete = app.document().unwrap().id;
        let _ = app.handle(Message::DeletePages);
        settle(&mut app, |app| {
            !app.document_busy() && app.document().is_some_and(|doc| doc.id != before_delete)
        });
        assert_eq!(app.viewer.page_count(), 2);
        assert!(app.edit_animation.is_some());
        assert_thumbnail_source(&app, 0, 1);
        assert_thumbnail_source(&app, 1, 2);

        let deleted = app.document().unwrap().id;
        let _ = app.handle(Message::Undo);
        settle(&mut app, |app| {
            !app.document_busy() && app.document().is_some_and(|doc| doc.id != deleted)
        });
        assert_eq!(app.viewer.page_count(), 3);
    }

    #[test]
    fn edit_animation_maps_rotation_and_deleted_slots() {
        let previous = EditState::identity(5);
        let mut rotated = previous.clone();
        rotated.rotate_slots(&[1], 3);
        let (kind, changes) = edit_changes(&previous, &rotated);
        assert_eq!(kind, EditKind::Rotate);
        assert_eq!(changes, [(1, Some(1), -1)]);

        let mut deleted = previous.clone();
        deleted.delete_slots(&[1, 3]);
        let (kind, changes) = edit_changes(&previous, &deleted);
        assert_eq!(kind, EditKind::Delete);
        assert_eq!(changes, [(1, None, 0), (3, None, 0)]);

        let pages = changes
            .into_iter()
            .map(|(from_slot, to_slot, turns)| PageEditAnimation {
                from_slot,
                to_slot,
                turns,
                snapshot: PageSnapshot {
                    rect: Rectangle::new(Point::ORIGIN, Size::new(100.0, 140.0)),
                    size: engine::PageSize {
                        width: 612.0,
                        height: 792.0,
                    },
                    thumbnail: None,
                    tiles: Vec::new(),
                },
            })
            .collect();
        let reader_rects: Vec<_> = (0..5)
            .map(|slot| {
                Rectangle::new(
                    Point::new(0.0, slot as f32 * 200.0),
                    Size::new(100.0, 140.0),
                )
            })
            .collect();
        let mut animation =
            EditAnimation::new(kind, pages, 5, 0.0, Vec::new(), reader_rects.clone());
        assert_eq!(animation.previous_slot(0), Some(0));
        assert_eq!(animation.previous_slot(1), Some(2));
        assert_eq!(animation.previous_slot(2), Some(4));
        assert_eq!(animation.previous_slot(3), None);

        let target = Rectangle::new(Point::new(0.0, 400.0), Size::new(100.0, 140.0));
        assert_eq!(animation.reader_rect(2, target), reader_rects[4]);
        animation.restart();
        animation.advance(animation.progress.started + Duration::from_millis(120));
        assert_eq!(animation.reader_rect(2, target).y, 600.0);
        animation.advance(animation.progress.started + Duration::from_millis(240));
        assert_eq!(animation.reader_rect(2, target), target);

        let mut rotation = EditAnimation::new(
            EditKind::Rotate,
            Vec::new(),
            5,
            0.0,
            Vec::new(),
            reader_rects,
        );
        rotation.restart();
        rotation.advance(rotation.progress.started + Duration::from_millis(180));
        assert_eq!(rotation.reader_rect(1, target).y, 300.0);
    }

    #[test]
    fn window_border_uses_colorref_channel_order() {
        assert_eq!(window_border_color(Palette::DARK), 0x0045_4342);
        assert_eq!(window_border_color(Palette::LIGHT), 0x00cd_c9c9);
    }

    #[test]
    fn toolbar_hover_animates_only_the_active_control() {
        let (mut app, _) = App::boot(Options::default());
        let _ = app.handle(Message::ToolbarHover {
            control: ToolbarControl::Open,
            hovered: true,
        });

        let now = Instant::now();
        let _ = app.handle(Message::Animate(now + Duration::from_millis(60)));

        assert!(app.toolbar_hover(ToolbarControl::Open) > 0.0);
        assert!(app.toolbar_hover(ToolbarControl::Open) < 1.0);
        assert_eq!(app.toolbar_hover(ToolbarControl::Search), 0.0);

        let _ = app.handle(Message::ToolbarHover {
            control: ToolbarControl::Open,
            hovered: false,
        });
        let _ = app.handle(Message::Animate(now + Duration::from_millis(180)));

        assert_eq!(app.toolbar_hover(ToolbarControl::Open), 0.0);
    }

    #[cfg(windows)]
    #[test]
    fn native_frame_hit_testing_covers_drag_and_resize_edges() {
        use windows::Win32::Foundation::{POINT, RECT};
        use windows::Win32::UI::WindowsAndMessaging::{
            HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP,
            HTTOPLEFT, HTTOPRIGHT,
        };

        let rect = RECT {
            left: 100,
            top: 100,
            right: 900,
            bottom: 700,
        };
        let hit = |x, y| frame_hit_test(rect, POINT { x, y }, 1.0, 20.0);

        assert_eq!(hit(500, 110), HTCAPTION);
        assert_eq!(hit(500, 100), HTTOP);
        assert_eq!(hit(101, 400), HTLEFT);
        assert_eq!(hit(899, 400), HTRIGHT);
        assert_eq!(hit(500, 699), HTBOTTOM);
        assert_eq!(hit(101, 101), HTTOPLEFT);
        assert_eq!(hit(110, 105), HTTOPLEFT);
        assert_eq!(hit(899, 101), HTTOPRIGHT);
        assert_eq!(hit(101, 699), HTBOTTOMLEFT);
        assert_eq!(hit(899, 699), HTBOTTOMRIGHT);
        assert_eq!(hit(500, 400), HTCLIENT);
    }

    #[test]
    fn shows_a_document_given_at_start_up() {
        let (mut app, _) = App::boot(Options {
            file: Some(fixture_path()),
            page: Some(2),
            ..Options::default()
        });

        // There is no window, so no layout: the view is the size assumed.
        assert_eq!(
            app.viewer.viewport(),
            ui::page_view_size(WINDOW, app.motion.sidebar.value)
        );
        assert!(app.opening.is_some());

        settle(&mut app, |app| {
            app.document().is_some() && app.viewer.is_complete()
        });

        assert!(app.opening.is_none());
        assert_eq!(app.title(), "Sample Document — Folio Flow");
        assert_eq!(app.viewer.current_page(), 1);
        assert_eq!(app.page_input, "2");
        assert_eq!(app.sidebar.tab, sidebar::Tab::Pages);
    }

    #[test]
    fn leaving_pages_clears_its_animation_demand() {
        let (mut app, _) = App::boot(Options::default());
        app.sidebar_animating = true;

        let _ = app.update(Message::ShowTab(sidebar::Tab::Outline));

        assert!(!app.sidebar_animating);
    }

    #[test]
    fn opens_another_document_over_the_first() {
        let (mut app, _) = App::boot(Options {
            file: Some(fixture_path()),
            ..Options::default()
        });

        settle(&mut app, |app| app.document().is_some());

        let first = app.document().map(|doc| doc.id);

        app.viewer.go_to_page(2);
        app.sidebar.show(sidebar::Tab::Outline);

        let _ = app.update(Message::Open(Some(fixture_path())));

        settle(&mut app, |app| {
            app.document().map(|doc| doc.id) != first && app.viewer.is_complete()
        });

        // From the top again, and whole.
        assert_eq!(app.viewer.current_page(), 0);
        assert!(app.opening.is_none());
        assert_eq!(app.sidebar.tab, sidebar::Tab::Pages);
    }

    #[test]
    fn organizer_drag_previews_order_and_commits_once_on_release() {
        let (mut app, _) = App::boot(Options {
            file: Some(fixture_path()),
            ..Options::default()
        });

        settle(&mut app, |app| {
            app.document().is_some() && app.viewer.is_complete()
        });
        let opened = app.document().unwrap().id;
        settle_thumbnails(&mut app);
        let _ = app.update(Message::SetView(ViewMode::Organizer));
        seed_thumbnails(&mut app);

        for (source, slot) in [(0, 2), (0, 1)] {
            let _ = app.update(Message::OrganizerDrag {
                source,
                slot,
                scroll: 0.0,
                commit: false,
            });
            assert_eq!(app.document().unwrap().id, opened);
            assert_eq!(app.edits.current().order.as_sources(), &[0, 1, 2]);
            assert!(!app.document_busy());
            for page in 0..3 {
                assert!(app.viewer.thumbnail(page).is_some());
            }
        }
        assert_eq!(app.order.as_sources(), &[1, 0, 2]);

        let _ = app.update(Message::OrganizerDrag {
            source: 0,
            slot: 1,
            scroll: 0.0,
            commit: true,
        });
        assert!(app.document_busy());
        assert_eq!(app.edits.current().order.as_sources(), &[1, 0, 2]);

        settle(&mut app, |app| {
            !app.document_busy() && app.document().is_some_and(|doc| doc.id != opened)
        });
        assert_eq!(app.viewer.page_count(), 3);
        for (slot, source) in [(0, 1), (1, 0), (2, 2)] {
            assert_thumbnail_source(&app, slot, source);
        }
        assert!(
            app.viewer
                .wanted_thumbnails(&[0, 1, 2])
                .thumbnails
                .is_empty()
        );
    }

    #[test]
    fn sidebar_drag_previews_order_and_commits_once_on_release() {
        let (mut app, _) = App::boot(Options {
            file: Some(fixture_path()),
            ..Options::default()
        });

        settle(&mut app, |app| {
            app.document().is_some() && app.viewer.is_complete()
        });
        let opened = app.document().unwrap().id;
        settle_thumbnails(&mut app);
        seed_thumbnails(&mut app);

        for (source, slot) in [(0, 2), (0, 1)] {
            let _ = app.update(Message::SidebarPageDrag {
                source,
                slot,
                scroll: 0.0,
                commit: false,
            });
            assert_eq!(app.document().unwrap().id, opened);
            assert_eq!(app.edits.current().order.as_sources(), &[0, 1, 2]);
            assert!(!app.document_busy());
            for page in 0..3 {
                assert!(app.viewer.thumbnail(page).is_some());
            }
        }
        assert_eq!(app.order.as_sources(), &[1, 0, 2]);

        let _ = app.update(Message::SidebarPageDrag {
            source: 0,
            slot: 1,
            scroll: 0.0,
            commit: true,
        });
        assert!(app.document_busy());
        assert_eq!(app.edits.current().order.as_sources(), &[1, 0, 2]);

        settle(&mut app, |app| {
            !app.document_busy() && app.document().is_some_and(|doc| doc.id != opened)
        });
        assert_eq!(app.viewer.page_count(), 3);
        for (slot, source) in [(0, 1), (1, 0), (2, 2)] {
            assert_thumbnail_source(&app, slot, source);
        }
        assert!(
            app.viewer
                .wanted_thumbnails(&[0, 1, 2])
                .thumbnails
                .is_empty()
        );
    }

    #[test]
    fn chained_edits_keep_source_identity_for_selection_and_navigation() {
        let (mut app, _) = App::boot(Options {
            file: Some(fixture_path()),
            ..Options::default()
        });

        settle(&mut app, |app| {
            app.document().is_some() && app.viewer.is_complete()
        });
        settle_thumbnails(&mut app);
        seed_thumbnails(&mut app);
        let _ = app.update(Message::SetView(ViewMode::Organizer));

        let before_reorder = app.document().unwrap().id;
        let _ = app.update(Message::OrganizerDrag {
            source: 0,
            slot: 2,
            scroll: 0.0,
            commit: true,
        });
        settle(&mut app, |app| {
            !app.document_busy() && app.document().is_some_and(|doc| doc.id != before_reorder)
        });
        assert_eq!(app.order.as_sources(), &[1, 2, 0]);

        let before_delete = app.document().unwrap().id;
        let _ = app.update(Message::DeletePages);
        settle(&mut app, |app| {
            !app.document_busy() && app.document().is_some_and(|doc| doc.id != before_delete)
        });
        assert_eq!(app.order.as_sources(), &[1, 2]);
        assert_eq!(app.viewer.page_count(), 2);

        let _ = app.update(Message::OrganizerClicked {
            slot: 0,
            modifiers: keyboard::Modifiers::default(),
            double_click: true,
        });
        assert_eq!(app.viewer.current_page(), 0);
        let _ = app.update(Message::SetView(ViewMode::Organizer));

        let before_second_reorder = app.document().unwrap().id;
        let _ = app.update(Message::OrganizerDrag {
            source: 2,
            slot: 0,
            scroll: 0.0,
            commit: true,
        });
        settle(&mut app, |app| {
            !app.document_busy()
                && app
                    .document()
                    .is_some_and(|doc| doc.id != before_second_reorder)
        });
        assert_eq!(app.order.as_sources(), &[2, 1]);
        let _ = app.update(Message::OrganizerClicked {
            slot: 0,
            modifiers: keyboard::Modifiers::default(),
            double_click: false,
        });
        assert_eq!(app.target_slots(), [0]);
        assert_thumbnail_source(&app, 0, 2);
        assert_thumbnail_source(&app, 1, 1);

        let _ = app.update(Message::SidebarPageClicked { source: 2 });
        assert_eq!(app.viewer.current_page(), 0);

        let before_undo = app.document().unwrap().id;
        let _ = app.update(Message::Undo);
        assert!(app.document_busy());
        assert_eq!(app.viewer_order.as_sources(), &[2, 1]);
        assert_eq!(app.edits.current().order.as_sources(), &[1, 2]);
        assert_eq!(app.viewer_source(0), 2);
        assert_eq!(app.viewer_slot(1), Some(1));
        assert_eq!(app.edit_slot(1), Some(0));
        settle(&mut app, |app| {
            !app.document_busy() && app.document().is_some_and(|doc| doc.id != before_undo)
        });
        assert_eq!(app.order.as_sources(), &[1, 2]);
        assert_eq!(app.viewer.current_page(), 1);

        let before_redo = app.document().unwrap().id;
        let _ = app.update(Message::Redo);
        settle(&mut app, |app| {
            !app.document_busy() && app.document().is_some_and(|doc| doc.id != before_redo)
        });
        assert_eq!(app.order.as_sources(), &[2, 1]);
        assert_eq!(app.viewer.current_page(), 0);
    }

    #[test]
    fn reports_a_file_it_cannot_open() {
        let (mut app, _) = App::boot(Options {
            file: Some(PathBuf::from("no-such-file.pdf")),
            ..Options::default()
        });

        settle(&mut app, |app| app.toast.is_some());

        let toast = app.toast.as_ref().unwrap();

        assert_eq!(toast.tone, Tone::Error);
        assert!(toast.text.starts_with("无法打开 no-such-file.pdf。"));
        assert!(app.document().is_none() && app.opening.is_none());
    }

    #[test]
    fn follows_a_link_to_its_place() {
        let (mut app, _) = App::boot(Options {
            file: Some(fixture_path()),
            ..Options::default()
        });

        settle(&mut app, |app| app.document().is_some());

        // The second paragraph of the first page is a link to a place 52pt
        // down the second.
        let link = |app: &App| on_screen(&app.viewer, 0, 100.0, 188.0);

        settle(&mut app, |app| app.viewer.link_at(link(app)).is_some());

        let at = link(&app);
        let _ = app.update(Message::Viewer(Interaction::Clicked { at }));

        assert_eq!(app.viewer.current_page(), 1);
        assert_eq!(app.page_input, "2");

        let place = on_screen(&app.viewer, 1, 0.0, 52.0);

        assert!(place.y > 0.0 && place.y < 30.0, "{place:?}");
    }

    #[test]
    fn opens_no_link_but_to_the_web() {
        let (mut app, _) = App::boot(Options::default());

        app.follow(Target::Uri("file:///C:/Windows/System32/calc.exe".into()));

        let toast = app.toast.as_ref().unwrap();

        assert_eq!(toast.tone, Tone::Error);
        assert!(toast.text.contains("网页"), "{}", toast.text);
    }

    #[test]
    fn finds_as_the_query_is_typed() {
        let (mut app, _) = App::boot(Options {
            file: Some(fixture_path()),
            ..Options::default()
        });

        settle(&mut app, |app| app.document().is_some());

        let _ = app.update(Message::Find);

        for query in ["p", "pa", "paragraph"] {
            let _ = app.update(Message::FindInput(query.into()));
        }

        settle(&mut app, |app| !app.find.searching);

        assert_eq!(app.viewer.match_count(), 2);
        assert_eq!(app.viewer.current_match(), Some(1));
    }
}
