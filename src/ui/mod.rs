//! View composition: floating chrome, sidebar, page views and overlays.

pub mod chrome;
pub mod dialog;
pub mod file_drop;
pub mod icon;
pub mod order;
pub mod organizer;
pub mod sidebar;
pub mod theme;
pub mod transition;
pub mod viewer;

use std::time::Duration;

use iced::advanced::layout::{self, Layout};
use iced::advanced::renderer;
use iced::advanced::widget::{Tree, Widget};
use iced::widget::{
    button, column, container, mouse_area, progress_bar, responsive, row, space, stack, text,
    text_input, tooltip,
};
use iced::{
    Center, Color, Element, Fill, Font, Length, Padding, Rectangle, Size, border, font, mouse,
};

use crate::app::ViewMode;
use crate::app::{App, Find, Message, Tone};
use crate::graphics;
use icon::Icon;
use theme::Palette;

#[cfg(target_os = "macos")]
const PINGFANG_FONT: &str =
    "/System/Library/PrivateFrameworks/FontServices.framework/Resources/Reserved/PingFangUI.ttc";
#[cfg(target_os = "macos")]
static PINGFANG_AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// The interface font: the platform's own where we know its name.
pub fn font() -> Font {
    #[cfg(target_os = "windows")]
    {
        Font::with_name("Microsoft YaHei UI")
    }

    #[cfg(target_os = "macos")]
    {
        if *PINGFANG_AVAILABLE.get_or_init(|| std::path::Path::new(PINGFANG_FONT).is_file()) {
            Font::with_name("PingFang SC")
        } else {
            Font::with_name("Hiragino Sans")
        }
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        Font::DEFAULT
    }
}

/// A heavier cut of the interface font for titles.
pub(crate) fn strong() -> Font {
    Font {
        weight: if cfg!(target_os = "windows") {
            font::Weight::Semibold
        } else {
            font::Weight::Bold
        },
        ..font()
    }
}

#[cfg(target_os = "macos")]
pub fn load_platform_fonts() {
    use iced::advanced::graphics::text::font_system;

    // Iced does not scan the reserved system font directory by default.
    let mut fonts = font_system().write().unwrap();
    match std::fs::read(PINGFANG_FONT) {
        Ok(bytes) => fonts.load_font(bytes.into()),
        Err(error) => log::warn!("could not load PingFang: {error}"),
    }
}

/// Lays out a line in each cut of the interface font, which leaves the font
/// loaded and matched for the first frame to use.
pub fn warm_fonts() {
    use iced::advanced::graphics::text::Paragraph;
    use iced::advanced::text::{self, Paragraph as _};

    for font in [font(), strong()] {
        let _ = Paragraph::with_text(text::Text {
            content: "Folio Flow 苹方中文字体 0123456789",
            bounds: Size::INFINITE,
            size: theme::TEXT.into(),
            line_height: text::LineHeight::default(),
            font,
            align_x: text::Alignment::Default,
            align_y: iced::alignment::Vertical::Top,
            shaping: text::Shaping::default(),
            wrapping: text::Wrapping::default(),
        });
    }
}

/// The size the Reader gets in a window of the given size.
pub fn page_view_size(window: Size, sidebar_progress: f32) -> Size {
    let insets = theme::reader_insets(window.width, sidebar_progress);

    Size::new(
        (window.width - insets.left - insets.right).max(1.0),
        (window.height - insets.top - insets.bottom).max(1.0),
    )
}

pub fn view(app: &App) -> Element<'_, Message> {
    responsive(move |size| view_at(app, size))
        .width(Fill)
        .height(Fill)
        .into()
}

fn view_at<'a>(app: &'a App, size: Size) -> Element<'a, Message> {
    let palette = app.palette;
    let background = container(space())
        .width(Fill)
        .height(Fill)
        .style(move |_| theme::canvas(palette));

    let mark = container(Element::new(FrameMark))
        .width(Fill)
        .height(1)
        .style(move |_| theme::canvas(palette));

    let mut layers = stack![background, mark];

    if app.viewer.document().is_some() {
        let content: Element<'a, Message> = match app.view_mode {
            ViewMode::Reader => viewer::view(
                &app.viewer,
                palette,
                app.edit_animation.as_ref(),
                Message::Viewer,
            ),
            ViewMode::Organizer => organizer::view(app),
        };
        let padding = if app.immersive {
            Padding::ZERO
        } else {
            match app.view_mode {
                ViewMode::Reader => theme::reader_insets(size.width, app.motion.sidebar.value),
                ViewMode::Organizer => Padding {
                    top: theme::content_top(size.width),
                    ..Padding::ZERO
                },
            }
        };

        layers = layers.push(container(content).width(Fill).height(Fill).padding(padding));
    } else if app.opening.is_some() {
        layers = layers.push(space().width(Fill).height(Fill));
    } else {
        layers = layers.push(welcome(app));
    }

    if let Some(transition) = &app.view_transition {
        layers = layers.push(transition::view(app, transition));
    }

    let veil = app.motion.view_veil();
    if veil > 0.001 {
        layers = layers.push(container(space()).width(Fill).height(Fill).style(move |_| {
            container::Style {
                background: Some(
                    Color {
                        a: veil,
                        ..palette.canvas
                    }
                    .into(),
                ),
                ..container::Style::default()
            }
        }));
    }

    let sidebar_progress = app.motion.sidebar.value.clamp(0.0, 1.0);
    if !app.immersive && (app.sidebar.open || sidebar_progress > 0.001) {
        let width = theme::sidebar_width(size.width);
        let edge = theme::edge(size.width);
        layers = layers.push(
            container(sidebar::view(app, width))
                .width(width + edge)
                .height(Fill)
                .align_x(iced::Right)
                .padding(Padding {
                    top: theme::content_top(size.width),
                    right: (1.0 - sidebar_progress) * (width + edge),
                    bottom: theme::SIDEBAR_BOTTOM,
                    ..Padding::ZERO
                }),
        );
    }

    if !app.immersive {
        layers = layers.push(chrome::view(app, size.width));
    }

    if app.more_open && !app.immersive {
        layers =
            layers.push(mouse_area(space().width(Fill).height(Fill)).on_press(Message::Escape));
        layers = layers.push(
            container(more_menu(app))
                .width(Fill)
                .height(Fill)
                .align_x(iced::Left)
                .padding(Padding {
                    left: theme::EDGE,
                    top: theme::EDGE + theme::CHROME_HEIGHT + 8.0,
                    ..Padding::ZERO
                }),
        );
    }

    let find_progress = app.motion.find.value.clamp(0.0, 1.0);
    if !app.immersive && (app.find.open || find_progress > 0.001) && app.viewer.document().is_some()
    {
        layers = layers.push(
            container(find_bar(app))
                .width(Fill)
                .height(Fill)
                .align_right(Fill)
                .padding(Padding {
                    top: theme::content_top(size.width) + 8.0,
                    right: theme::edge(size.width) + (1.0 - find_progress) * 420.0,
                    ..Padding::ZERO
                }),
        );
    }

    if let Some(toast) = &app.toast {
        layers = layers.push(
            container(toast_view(palette, toast))
                .width(Fill)
                .height(Fill)
                .align_x(Center)
                .align_bottom(Fill)
                .padding(theme::edge(size.width)),
        );
    }

    if app.dialog.is_some() {
        layers = layers.push(dialog::view(app, size));
    }

    layers.into()
}

/// A pixel-high strip that draws nothing, and tells the graphics module that
/// a frame is being drawn.
struct FrameMark;

impl<Message> Widget<Message, iced::Theme, iced::Renderer> for FrameMark {
    fn size(&self) -> Size<Length> {
        Size::new(Fill, Length::Fixed(1.0))
    }

    fn layout(
        &mut self,
        _tree: &mut Tree,
        _renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, Fill, 1)
    }

    fn draw(
        &self,
        _tree: &Tree,
        _renderer: &mut iced::Renderer,
        _theme: &iced::Theme,
        _style: &renderer::Style,
        _layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        graphics::pace();
    }
}

pub(crate) fn hint<'a>(palette: Palette, label: &'a str) -> Element<'a, Message> {
    container(text(label).size(theme::TEXT_SMALL))
        .padding([5, 9])
        .style(move |_| theme::popover(palette))
        .into()
}

/// A square toolbar button. `active` marks a toggled state; a `None`
/// message disables it.
pub(crate) fn tool<'a>(
    palette: Palette,
    icon: Icon,
    label: &'a str,
    message: Option<Message>,
    active: bool,
) -> Element<'a, Message> {
    let color = match (&message, active) {
        (None, _) => palette.faint,
        (_, true) => palette.on_accent,
        _ => palette.muted,
    };

    tooltip(
        button(container(icon.view(16.0, color)).center(theme::CONTROL))
            .padding(0)
            .on_press_maybe(message)
            .style(move |_, status| theme::ghost_button(palette, status, active)),
        hint(palette, label),
        tooltip::Position::Bottom,
    )
    .gap(8)
    .delay(Duration::from_millis(450))
    .into()
}

fn more_menu(app: &App) -> Element<'_, Message> {
    let palette = app.palette;
    let ready = app.viewer.document().is_some() && !app.document_busy() && app.opening.is_none();
    let rule = || {
        container(space().height(1))
            .width(Fill)
            .style(move |_| theme::rule(palette))
    };
    container(
        column![
            menu_item(
                "打开…",
                "Ctrl+O",
                (!app.document_busy()).then_some(Message::OpenDialog),
                palette
            ),
            menu_item(
                "保存",
                "Ctrl+S",
                (ready && app.edits.dirty()).then_some(Message::Save),
                palette
            ),
            menu_item(
                "另存为…",
                "Ctrl+Shift+S",
                ready.then_some(Message::SaveAs),
                palette
            ),
            rule(),
            menu_item("导出…", "Ctrl+E", ready.then_some(Message::Export), palette),
            if app.window_size.width < 1280.0 {
                menu_item("压缩 PDF…", "", ready.then_some(Message::Compact), palette)
            } else {
                space().height(0).into()
            },
            rule(),
            menu_item("设置…", "", Some(Message::ShowSettings), palette),
            menu_item(
                "退出",
                "",
                Some(Message::WindowAction(crate::app::WindowAction::Close)),
                palette
            ),
        ]
        .spacing(3)
        .width(220),
    )
    .padding(6)
    .style(move |_| theme::popover(palette))
    .into()
}

fn menu_item<'a>(
    label: &'a str,
    shortcut: &'a str,
    message: Option<Message>,
    palette: Palette,
) -> Element<'a, Message> {
    let color = if message.is_some() {
        palette.text
    } else {
        palette.faint
    };
    button(
        row![
            text(label).size(theme::TEXT_SMALL).color(color),
            space().width(Fill),
            text(shortcut).size(11).color(palette.muted),
        ]
        .align_y(Center),
    )
    .padding([7, 9])
    .width(Fill)
    .on_press_maybe(message)
    .style(move |_, status| theme::ghost_button(palette, status, false))
    .into()
}

/// The find bar, floating over the top right of the page view.
fn find_bar(app: &App) -> Element<'_, Message> {
    let palette = app.palette;
    let find = &app.find;
    let count = app.viewer.match_count();
    let more = if find.capped { "+" } else { "" };

    let (status, color) = match (count, app.viewer.current_match()) {
        (0, _) if find.query.trim().is_empty() => (String::new(), palette.muted),
        (0, _) if find.searching => ("正在查找…".into(), palette.muted),
        (0, _) => ("未找到结果".into(), palette.danger),
        (_, Some(current)) => (format!("{current} of {count}{more}"), palette.muted),
        (_, None) => (format!("{count}{more}"), palette.muted),
    };

    let field = text_input("在文档中查找", &find.query)
        .id(Find::FIELD)
        .on_input(Message::FindInput)
        .on_submit(Message::FindSubmit)
        .size(theme::TEXT)
        .padding([4, 6])
        .width(210)
        .style(move |_, status| theme::input(palette, status));

    let step = |direction: i8| (count > 0).then_some(Message::FindStep(direction));

    let bar = container(
        row![
            Icon::Search.view(15.0, palette.muted),
            space().width(8),
            field,
            container(
                text(status)
                    .size(theme::TEXT_SMALL)
                    .color(color)
                    .wrapping(text::Wrapping::None),
            )
            .width(96)
            .align_x(iced::Right)
            .padding([0, 8]),
            tool(
                palette,
                Icon::ChevronUp,
                "上一个  Shift+Enter",
                step(-1),
                false,
            ),
            tool(palette, Icon::ChevronDown, "下一个  Enter", step(1), false),
            tool(
                palette,
                Icon::Close,
                "关闭  Esc",
                Some(Message::Escape),
                false,
            ),
        ]
        .spacing(2)
        .align_y(Center),
    )
    .padding(Padding::new(6.0).left(10.0))
    .style(move |_| theme::popover(palette));

    mouse_area(bar).interaction(mouse::Interaction::Idle).into()
}

fn welcome(app: &App) -> Element<'_, Message> {
    let palette = app.palette;

    let open = button(text("打开 PDF").size(theme::TEXT).font(strong()))
        .padding([8, 16])
        .on_press(Message::OpenDialog)
        .style(move |_, status| theme::primary_button(palette, status));

    container(
        column![
            Icon::Document.view(40.0, palette.faint),
            space().height(6),
            text("尚未打开文档")
                .size(17)
                .font(strong())
                .color(palette.text),
            text("将 PDF 拖到此处，或按 Ctrl+O 打开")
                .size(theme::TEXT)
                .color(palette.muted),
            space().height(10),
            open,
        ]
        .spacing(6)
        .align_x(Center),
    )
    .center(Fill)
    .into()
}

fn toast_view<'a>(palette: Palette, toast: &'a crate::app::Toast) -> Element<'a, Message> {
    let color = match toast.tone {
        Tone::Info => palette.text,
        Tone::Error => palette.danger,
    };

    let mut content = column![text(toast.text.as_str()).size(theme::TEXT).color(color)].spacing(8);

    if let Some(progress) = toast.progress {
        content = content.push(
            container(progress_bar(0.0..=1.0, progress).girth(3).style(move |_| {
                progress_bar::Style {
                    background: palette.hover.into(),
                    bar: palette.accent.into(),
                    border: border::rounded(theme::PROGRESS_RADIUS),
                }
            }))
            .width(220),
        );
    }

    let dismiss = (toast.tone == Tone::Error).then(|| {
        button(container(Icon::Close.view(13.0, palette.muted)).center(20))
            .padding(0)
            .on_press(Message::DismissToast)
            .style(move |_, status| theme::ghost_button(palette, status, false))
    });

    container(row![content].push(dismiss).spacing(12).align_y(Center))
        .padding([10, 14])
        .max_width(520)
        .style(move |_| theme::popover(palette))
        .into()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Instant;

    use iced::{Point, window};

    use super::*;
    use crate::app::Options;
    use crate::engine::{DocInfo, PageSize};
    use crate::ui::viewer::Interaction;

    #[cfg(target_os = "macos")]
    #[test]
    fn pingfang_is_loaded_and_selected() {
        use iced::advanced::graphics::text::font_system;

        assert!(std::path::Path::new(PINGFANG_FONT).is_file());
        load_platform_fonts();

        let mut fonts = font_system().write().unwrap();
        assert!(
            fonts
                .raw()
                .db()
                .faces()
                .any(|face| face.families.iter().any(|(name, _)| name == "PingFang SC"))
        );
        assert_eq!(font().family, font::Family::Name("PingFang SC"));
    }

    #[test]
    fn pages_sidebar_routes_wheel_events_by_pointer_location() {
        let window = Size::new(1360.0, 870.0);
        let (mut app, _) = App::boot(Options::default());
        app.sidebar.open = true;
        app.sidebar.show(sidebar::Tab::Pages);
        app.motion.sidebar.value = 1.0;
        app.motion.sidebar.target = 1.0;
        app.viewer.set_document(Arc::new(DocInfo {
            id: 1,
            path: PathBuf::new(),
            title: String::new(),
            pages: vec![
                PageSize {
                    width: 612.0,
                    height: 792.0,
                };
                20
            ],
            outline: Vec::new(),
        }));

        for (point, in_sidebar) in [
            (Point::new(700.0, 400.0), false),
            (Point::new(100.0, 400.0), true),
        ] {
            let mut ui =
                iced_test::Simulator::with_size(iced::Settings::default(), window, app.view());
            let wheel = iced::Event::Mouse(mouse::Event::WheelScrolled {
                delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
            });

            ui.point_at(point);
            ui.simulate([iced::Event::Window(window::Event::RedrawRequested(
                Instant::now(),
            ))]);
            ui.simulate([wheel]);

            let messages: Vec<_> = ui.into_messages().collect();
            let reader_scrolls = messages
                .iter()
                .filter(|message| matches!(message, Message::Viewer(Interaction::Scrolled { .. })))
                .count();
            let sidebar_scrolls = messages
                .iter()
                .filter(|message| matches!(message, Message::SidebarWheel(_)))
                .count();
            let expected = if in_sidebar { (0, 1) } else { (1, 0) };

            assert_eq!((reader_scrolls, sidebar_scrolls), expected, "{messages:?}");
        }
    }

    #[test]
    fn outline_sidebar_captures_wheel_for_smooth_scrolling() {
        let (mut app, _) = App::boot(Options::default());
        app.sidebar.open = true;
        app.sidebar.show(sidebar::Tab::Outline);
        let doc = Arc::new(DocInfo {
            id: 1,
            path: PathBuf::new(),
            title: String::new(),
            pages: vec![
                PageSize {
                    width: 612.0,
                    height: 792.0
                };
                20
            ],
            outline: (0..100)
                .map(|i| crate::engine::OutlineItem {
                    title: format!("Chapter {i}"),
                    depth: 0,
                    page: Some(i % 20),
                })
                .collect(),
        });
        app.sidebar.set_document(&doc);
        app.viewer.set_document(doc);
        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            Size::new(1360.0, 870.0),
            app.view(),
        );
        ui.point_at(Point::new(100.0, 400.0));
        ui.simulate([iced::Event::Window(window::Event::RedrawRequested(
            Instant::now(),
        ))]);
        ui.simulate([iced::Event::Mouse(mouse::Event::WheelScrolled {
            delta: mouse::ScrollDelta::Lines { x: 0.0, y: -1.0 },
        })]);
        let messages: Vec<_> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|m| matches!(m, Message::SidebarScrolled(_))),
            "{messages:?}"
        );
        assert_eq!(
            messages
                .iter()
                .filter(|m| matches!(m, Message::SidebarWheel(56.0)))
                .count(),
            1
        );
        assert!(
            !messages
                .iter()
                .any(|m| matches!(m, Message::Viewer(Interaction::Scrolled { .. })))
        );
    }

    #[test]
    fn immersive_reader_uses_the_entire_window() {
        let (mut app, _) = App::boot(Options::default());
        app.immersive = true;
        app.sidebar.open = true;
        app.viewer.set_document(Arc::new(DocInfo {
            id: 1,
            path: PathBuf::new(),
            title: String::new(),
            pages: vec![
                PageSize {
                    width: 612.0,
                    height: 792.0
                };
                3
            ],
            outline: Vec::new(),
        }));
        let size = Size::new(1360.0, 870.0);
        let mut simulator =
            iced_test::Simulator::with_size(iced::Settings::default(), size, app.view());
        simulator.simulate([iced::Event::Window(window::Event::RedrawRequested(
            Instant::now(),
        ))]);
        let sizes: Vec<_> = simulator
            .into_messages()
            .filter_map(|message| match message {
                Message::Viewer(Interaction::Resized(size)) => Some(size),
                _ => None,
            })
            .collect();
        assert_eq!(sizes, [size]);
    }

    /// Start-up renders the first screen before there is a window to lay
    /// out, so what it assumes about the layout has to hold.
    #[test]
    fn page_view_size_is_where_the_layout_puts_it() {
        let window = Size::new(1360.0, 870.0);

        for sidebar in [true, false] {
            let (mut app, _) = App::boot(Options::default());

            app.sidebar.open = sidebar;
            app.motion.sidebar.value = if sidebar { 1.0 } else { 0.0 };
            app.motion.sidebar.target = app.motion.sidebar.value;
            app.viewer.set_document(Arc::new(DocInfo {
                id: 1,
                path: PathBuf::new(),
                title: String::new(),
                pages: vec![
                    PageSize {
                        width: 612.0,
                        height: 792.0,
                    };
                    3
                ],
                outline: Vec::new(),
            }));

            let mut ui =
                iced_test::Simulator::with_size(iced::Settings::default(), window, app.view());

            ui.simulate([iced::Event::Window(window::Event::RedrawRequested(
                Instant::now(),
            ))]);

            let sizes: Vec<Size> = ui
                .into_messages()
                .filter_map(|message| match message {
                    Message::Viewer(Interaction::Resized(size)) => Some(size),
                    _ => None,
                })
                .collect();

            assert_eq!(sizes, [page_view_size(window, app.motion.sidebar.value)]);
        }
    }
}

pub mod wheel_gate;
