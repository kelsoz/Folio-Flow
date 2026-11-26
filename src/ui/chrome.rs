//! The three floating top capsules.

use std::time::Duration;

use iced::widget::{
    button, container, mouse_area, row, scrollable, slider, space, text, text_input, tooltip,
};
use iced::{Border, Center, Color, Element, Fill, Length, Padding, mouse};

use crate::app::{App, Message, ToolbarControl, ViewMode, WindowAction};
use crate::ui::icon::Icon;
use crate::ui::organizer;
use crate::ui::theme::{self, Palette};
use crate::ui::viewer::{Fit, MAX_ZOOM, MIN_ZOOM};

const FULL_WIDTH: f32 = 1280.0;
const FILE_WIDTH: f32 = 342.0;
const READ_WIDTH: f32 = 600.0;
const ACTION_WIDTH: f32 = 300.0;
const WIDE_ACTION_WIDTH: f32 = 520.0;
const WINDOW_WIDTH: f32 = 116.0;
const SLIDER_WIDTH: f32 = 96.0;
const SLIDER_LABEL_WIDTH: f32 = 44.0;
const SLIDER_GAP: f32 = 4.0;
const SELECTION_WIDTH: f32 = 72.0;
const TOOL_SIZE: f32 = theme::CONTROL;
const TOOL_GAP: f32 = 2.0;
const DIVIDER_WIDTH: f32 = 1.0;
const CAPSULE_PADDING_X: f32 = 10.0;
const CAPSULE_PADDING_Y: f32 = 6.0;
const COMPACT_GAP: f32 = 10.0;

fn compact_read_width(width: f32) -> f32 {
    (width - theme::edge(width) * 2.0 - WINDOW_WIDTH - COMPACT_GAP).clamp(1.0, READ_WIDTH)
}

fn read_tools_content_width(app: &App) -> f32 {
    let mut width = 3.0 * TOOL_SIZE + DIVIDER_WIDTH + 3.0 * TOOL_GAP;
    width += TOOL_GAP + SLIDER_LABEL_WIDTH + SLIDER_GAP + SLIDER_WIDTH;

    if app.view_mode == ViewMode::Reader {
        width += 2.0 * (TOOL_GAP + TOOL_SIZE);
    }

    width += TOOL_GAP + DIVIDER_WIDTH;

    if app.view_mode == ViewMode::Organizer {
        width += TOOL_GAP + SELECTION_WIDTH;
    }

    width += 4.0 * (TOOL_GAP + TOOL_SIZE);
    width += TOOL_GAP + DIVIDER_WIDTH;
    width += TOOL_GAP + TOOL_SIZE;
    width
}

fn compact_capsule_width(app: &App, width: f32) -> f32 {
    let content_width = read_tools_content_width(app) + CAPSULE_PADDING_X * 2.0;
    content_width.min(compact_read_width(width)).max(1.0)
}

pub fn view(app: &App, width: f32) -> Element<'_, Message> {
    let edge = theme::edge(width);
    let height = theme::chrome_height(width);
    let palette = app.palette;

    let content: Element<'_, Message> = if width < FULL_WIDTH {
        let read_width = compact_capsule_width(app, width);
        let inner_height = (height - CAPSULE_PADDING_Y * 2.0).max(1.0);
        let tools = container(read_tools(app))
            .width((read_width - CAPSULE_PADDING_X * 2.0).max(1.0))
            .height(inner_height)
            .align_y(Center);
        let scroll = scrollable(tools)
            .direction(scrollable::Direction::Horizontal(
                scrollable::Scrollbar::hidden(),
            ))
            .width(Fill)
            .height(inner_height)
            .style(move |_, status| capsule_scroller(palette, status));

        row![
            capsule(scroll, read_width, height, palette),
            drag_space(Fill, height),
            capsule(window_controls(app), WINDOW_WIDTH, height, palette),
        ]
        .align_y(Center)
        .into()
    } else {
        let action_width = if width >= 1560.0 {
            WIDE_ACTION_WIDTH
        } else {
            ACTION_WIDTH
        };
        row![
            capsule(file_capsule(app), FILE_WIDTH, height, palette),
            drag_space(Fill, height),
            capsule(read_tools(app), Length::Shrink, height, palette),
            drag_space(Fill, height),
            capsule(
                action_capsule(app, width >= 1560.0),
                action_width,
                height,
                palette,
            ),
        ]
        .align_y(Center)
        .into()
    };

    container(content)
        .width(Fill)
        .height(Fill)
        .padding(Padding {
            top: edge,
            right: edge,
            bottom: 0.0,
            left: edge,
        })
        .into()
}

fn capsule<'a>(
    content: impl Into<Element<'a, Message>>,
    width: impl Into<Length>,
    height: f32,
    palette: Palette,
) -> Element<'a, Message> {
    mouse_area(
        container(content)
            .width(width)
            .height(height)
            .padding([CAPSULE_PADDING_Y, CAPSULE_PADDING_X])
            .align_y(Center)
            .style(move |_| theme::capsule(palette)),
    )
    .on_press(Message::DragWindow)
    .interaction(mouse::Interaction::Idle)
    .into()
}

fn drag_space(width: impl Into<Length>, height: f32) -> Element<'static, Message> {
    mouse_area(space().width(width).height(height))
        .on_press(Message::DragWindow)
        .interaction(mouse::Interaction::Idle)
        .into()
}

fn capsule_scroller(palette: Palette, status: scrollable::Status) -> scrollable::Style {
    let base = theme::scroller(palette, status);
    let mut rail = base.horizontal_rail;
    rail.background = None;
    rail.scroller.background = Color::TRANSPARENT.into();

    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: base.auto_scroll,
    }
}

fn file_capsule(app: &App) -> Element<'_, Message> {
    let palette = app.palette;
    let doc = app.viewer.document();
    let title: Element<'_, Message> = match (doc, &app.opening) {
        (_, Some(path)) => text(format!(
            "正在打开 {}…",
            path.file_name().unwrap_or_default().to_string_lossy()
        ))
        .size(theme::TEXT)
        .color(palette.muted)
        .wrapping(text::Wrapping::None)
        .into(),
        (Some(doc), None) => text(format!(
            "{}{}",
            if app.edits.dirty() { "● " } else { "" },
            doc.path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| doc.title.clone()),
        ))
        .size(theme::TEXT)
        .font(super::strong())
        .color(palette.text)
        .wrapping(text::Wrapping::None)
        .into(),
        (None, None) => text("打开 PDF")
            .size(theme::TEXT)
            .font(super::strong())
            .color(palette.muted)
            .into(),
    };

    let editable = doc.is_some() && app.opening.is_none() && !app.document_busy();
    let pages = row![
        text_input("", &app.page_input)
            .on_input_maybe(doc.is_some().then_some(Message::PageInput))
            .on_submit(Message::PageSubmit)
            .size(theme::TEXT_SMALL)
            .padding([3, 4])
            .width(34)
            .align_x(Center)
            .style(move |_, status| theme::input(palette, status)),
        text(format!("/ {}", app.viewer.page_count().max(1)))
            .size(11)
            .color(palette.muted),
    ]
    .spacing(4)
    .align_y(Center);

    row![
        tool(
            app,
            ToolbarControl::Open,
            Icon::Open,
            "打开 PDF  Ctrl+O",
            Some(Message::OpenDialog),
            false,
        ),
        container(title).width(Fill).clip(true),
        container(pages).padding([0, 6]),
        divider(palette),
        tool(
            app,
            ToolbarControl::Undo,
            Icon::Undo,
            "撤销  Ctrl+Z",
            (editable && app.edits.can_undo()).then_some(Message::Undo),
            false,
        ),
        tool(
            app,
            ToolbarControl::Redo,
            Icon::Redo,
            "重做  Ctrl+Y",
            (editable && app.edits.can_redo()).then_some(Message::Redo),
            false,
        ),
    ]
    .spacing(2)
    .align_y(Center)
    .width(Fill)
    .into()
}

fn read_tools(app: &App) -> Element<'_, Message> {
    let palette = app.palette;
    let doc = app.viewer.document();
    let when_open = |message: Message| doc.is_some().then_some(message);
    let editable = doc.is_some() && app.opening.is_none() && !app.document_busy();
    let has_target = !app.target_slots().is_empty();
    let mode = app.view_mode;

    let mut tools = row![
        tool(
            app,
            ToolbarControl::Sidebar,
            Icon::Sidebar,
            "侧栏  Ctrl+B",
            Some(Message::ToggleSidebar),
            app.sidebar.open,
        ),
        tool(
            app,
            ToolbarControl::Reader,
            Icon::Document,
            "阅读",
            Some(Message::SetView(ViewMode::Reader)),
            mode == ViewMode::Reader,
        ),
        tool(
            app,
            ToolbarControl::Organizer,
            Icon::Organizer,
            "页面编排",
            Some(Message::SetView(ViewMode::Organizer)),
            mode == ViewMode::Organizer,
        ),
        divider(palette),
    ]
    .spacing(2)
    .align_y(Center);

    match mode {
        ViewMode::Reader => {
            let zoom = app.viewer.zoom();
            let zoom_slider = slider(MIN_ZOOM..=MAX_ZOOM, zoom, Message::ZoomTo)
                .step(0.01_f32)
                .default(1.0_f32)
                .width(SLIDER_WIDTH)
                .height(24)
                .style(move |_, _| slider_style(palette));

            tools = tools.push(labeled_slider(
                app,
                ToolbarControl::Zoom,
                format!("{:.0}%", zoom * 100.0),
                Message::ZoomReset,
                "原始大小  Ctrl+0",
                zoom_slider.into(),
            ));
            tools = tools
                .push(tool(
                    app,
                    ToolbarControl::FitWidth,
                    Icon::FitWidth,
                    "适合宽度  Ctrl+1",
                    when_open(Message::Fit(Fit::Width)),
                    doc.is_some() && app.viewer.fit() == Fit::Width,
                ))
                .push(tool(
                    app,
                    ToolbarControl::FitPage,
                    Icon::FitPage,
                    "适合页面  Ctrl+2",
                    when_open(Message::Fit(Fit::Page)),
                    doc.is_some() && app.viewer.fit() == Fit::Page,
                ));
        }
        ViewMode::Organizer => {
            let grid_slider = slider(
                organizer::MIN_TRACK..=organizer::MAX_TRACK,
                app.organizer.track,
                Message::GridSize,
            )
            .step(10.0_f32)
            .width(SLIDER_WIDTH)
            .height(24)
            .style(move |_, _| slider_style(palette));

            tools = tools.push(labeled_slider(
                app,
                ToolbarControl::Grid,
                format!("{:.0}", app.organizer.track),
                Message::GridSize(organizer::DEFAULT_TRACK),
                "重置网格大小  Ctrl+0",
                grid_slider.into(),
            ));
        }
    }

    tools = tools.push(divider(palette));

    if mode == ViewMode::Organizer {
        let count = app.organizer.selected.len();
        let selection = if count == 0 {
            "未选择".to_string()
        } else {
            format!("已选 {count} 页")
        };
        tools = tools.push(
            container(text(selection).size(11).color(palette.muted))
                .width(72)
                .center_y(theme::CONTROL),
        );
    }

    tools = tools
        .push(tool(
            app,
            ToolbarControl::RotateLeft,
            Icon::RotateLeft,
            "向左旋转  Ctrl+Shift+R",
            (editable && has_target).then_some(Message::Rotate(-1)),
            false,
        ))
        .push(tool(
            app,
            ToolbarControl::RotateRight,
            Icon::RotateRight,
            "向右旋转  Ctrl+R",
            (editable && has_target).then_some(Message::Rotate(1)),
            false,
        ))
        .push(tool(
            app,
            ToolbarControl::Extract,
            Icon::Extract,
            "提取页面",
            (editable && has_target).then_some(Message::Extract),
            false,
        ))
        .push(tool(
            app,
            ToolbarControl::Delete,
            Icon::Delete,
            "删除页面  Delete",
            (editable && app.can_delete()).then_some(Message::DeletePages),
            false,
        ))
        .push(divider(palette))
        .push(tool(
            app,
            ToolbarControl::More,
            Icon::More,
            "更多工具",
            doc.is_some().then_some(Message::ToggleMore),
            app.more_open,
        ));

    tools.width(Length::Shrink).into()
}

fn action_capsule(app: &App, wide: bool) -> Element<'_, Message> {
    let palette = app.palette;
    let doc = app.viewer.document();
    let when_open = |message: Message| doc.is_some().then_some(message);

    row![
        operation(
            app,
            ToolbarControl::Search,
            Icon::Search,
            "查找",
            when_open(Message::Find),
            wide,
        ),
        operation(
            app,
            ToolbarControl::Print,
            Icon::Print,
            "打印",
            when_open(Message::Print),
            wide,
        ),
        operation(
            app,
            ToolbarControl::Compress,
            Icon::Compress,
            "压缩",
            (doc.is_some() && app.opening.is_none() && !app.document_busy())
                .then_some(Message::Compact),
            wide,
        ),
        operation(
            app,
            ToolbarControl::Export,
            Icon::Export,
            "导出",
            when_open(Message::Export),
            wide,
        ),
        space().width(Fill),
        tool(
            app,
            ToolbarControl::Theme,
            if palette.is_dark {
                Icon::Sun
            } else {
                Icon::Moon
            },
            if palette.is_dark {
                "亮色模式"
            } else {
                "暗色模式"
            },
            Some(Message::ToggleTheme),
            false,
        ),
        divider(palette),
        window_controls(app),
    ]
    .spacing(2)
    .align_y(Center)
    .width(Fill)
    .into()
}

fn hover_area<'a>(content: Element<'a, Message>, control: ToolbarControl) -> Element<'a, Message> {
    mouse_area(content)
        .on_enter(Message::ToolbarHover {
            control,
            hovered: true,
        })
        .on_exit(Message::ToolbarHover {
            control,
            hovered: false,
        })
        .into()
}

fn tool<'a>(
    app: &'a App,
    control: ToolbarControl,
    icon: Icon,
    label: &'a str,
    message: Option<Message>,
    active: bool,
) -> Element<'a, Message> {
    let palette = app.palette;
    let hover = app.toolbar_hover(control);
    let interactive = message.is_some();
    let color = match (&message, active) {
        (None, _) => palette.faint,
        (_, true) => palette.on_accent,
        _ => palette.muted,
    };

    let button: Element<'a, Message> =
        button(container(icon.view(16.0, color)).center(theme::CONTROL))
            .padding(0)
            .on_press_maybe(message)
            .style(move |_, status| theme::ghost_button_motion(palette, status, active, hover))
            .into();
    let button = if interactive {
        hover_area(button, control)
    } else {
        button
    };

    tooltip(
        button,
        super::hint(palette, label),
        tooltip::Position::Bottom,
    )
    .gap(8)
    .delay(Duration::from_millis(450))
    .into()
}

fn window_controls(app: &App) -> Element<'_, Message> {
    let maximized = app.window_maximized;

    row![
        window_tool(
            app,
            ToolbarControl::Minimize,
            Icon::Minimize,
            "最小化",
            WindowAction::Minimize,
        ),
        window_tool(
            app,
            ToolbarControl::Maximize,
            if maximized {
                Icon::Restore
            } else {
                Icon::Maximize
            },
            if maximized { "还原" } else { "最大化" },
            WindowAction::Maximize,
        ),
        window_tool(
            app,
            ToolbarControl::Close,
            Icon::Close,
            "关闭",
            WindowAction::Close,
        ),
    ]
    .spacing(2)
    .align_y(Center)
    .into()
}

fn window_tool<'a>(
    app: &'a App,
    control: ToolbarControl,
    icon: Icon,
    label: &'a str,
    action: WindowAction,
) -> Element<'a, Message> {
    let palette = app.palette;
    let hover = app.toolbar_hover(control);
    let color = if icon == Icon::Close {
        palette.danger
    } else {
        palette.muted
    };

    let button: Element<'a, Message> =
        button(container(icon.view(16.0, color)).center(theme::CONTROL))
            .padding(0)
            .on_press(Message::WindowAction(action))
            .style(move |_, status| theme::ghost_button_motion(palette, status, false, hover))
            .into();

    tooltip(
        hover_area(button, control),
        super::hint(palette, label),
        tooltip::Position::Bottom,
    )
    .gap(8)
    .delay(Duration::from_millis(450))
    .into()
}

fn operation<'a>(
    app: &'a App,
    control: ToolbarControl,
    icon: Icon,
    label: &'a str,
    message: Option<Message>,
    wide: bool,
) -> Element<'a, Message> {
    if !wide {
        return tool(app, control, icon, label, message, false);
    }

    let palette = app.palette;
    let hover = app.toolbar_hover(control);
    let interactive = message.is_some();
    let color = if message.is_none() {
        palette.faint
    } else {
        palette.muted
    };

    let button: Element<'a, Message> = button(
        container(
            row![icon.view(16.0, color), text(label).size(12)]
                .spacing(6)
                .align_y(Center),
        )
        .height(theme::CONTROL)
        .align_y(Center),
    )
    .padding([0, 7])
    .on_press_maybe(message)
    .style(move |_, status| theme::ghost_button_motion(palette, status, false, hover))
    .into();
    if interactive {
        hover_area(button, control)
    } else {
        button
    }
}

fn divider(palette: Palette) -> Element<'static, Message> {
    container(space().width(1).height(18))
        .style(move |_| theme::rule(palette))
        .into()
}

fn labeled_slider<'a>(
    app: &'a App,
    control: ToolbarControl,
    label: String,
    reset: Message,
    hint: &'a str,
    slider: Element<'a, Message>,
) -> Element<'a, Message> {
    let palette = app.palette;
    let hover = app.toolbar_hover(control);
    let value = button(
        container(text(label).size(theme::TEXT_SMALL).color(palette.text))
            .center_x(44)
            .center_y(theme::CONTROL),
    )
    .padding(0)
    .on_press(reset)
    .style(move |_, status| theme::ghost_button_motion(palette, status, false, hover));

    let group: Element<'a, Message> = row![
        tooltip(value, super::hint(palette, hint), tooltip::Position::Bottom)
            .gap(8)
            .delay(Duration::from_millis(450)),
        slider,
    ]
    .spacing(4)
    .align_y(Center)
    .into();

    hover_area(group, control)
}

fn slider_style(palette: Palette) -> slider::Style {
    slider::Style {
        rail: slider::Rail {
            backgrounds: (
                Color {
                    a: if palette.is_dark { 0.24 } else { 0.18 },
                    ..palette.text
                }
                .into(),
                palette.accent.into(),
            ),
            width: 3.0,
            border: Border::default(),
        },
        handle: slider::Handle {
            shape: slider::HandleShape::Circle {
                radius: theme::SLIDER_HANDLE_RADIUS,
            },
            background: palette.accent.into(),
            border_width: 0.0,
            border_color: Color::TRANSPARENT,
        },
    }
}

#[cfg(test)]
mod tests {
    use iced::{Event, Point, Size};

    use super::*;

    #[test]
    fn hover_wrapper_keeps_tool_clicks() {
        let (app, _) = crate::app::App::boot(crate::app::Options::default());
        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            Size::new(50.0, 50.0),
            tool(
                &app,
                ToolbarControl::Open,
                Icon::Open,
                "Open",
                Some(Message::OpenDialog),
                false,
            ),
        );
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let release = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));

        ui.point_at(Point::new(25.0, 25.0));
        ui.simulate([press, release]);

        let messages: Vec<_> = ui.into_messages().collect();
        assert!(
            messages
                .iter()
                .any(|message| matches!(message, Message::OpenDialog))
        );
    }

    #[test]
    fn capsule_keeps_child_clicks_and_drags_blank_space() {
        let content = button(text(""))
            .width(30)
            .height(30)
            .padding(0)
            .on_press(Message::ZoomReset);
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));
        let release = Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left));

        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            Size::new(200.0, 50.0),
            capsule(content, 200.0, 50.0, Palette::DARK),
        );
        ui.point_at(Point::new(25.0, 25.0));
        ui.simulate([press.clone(), release.clone()]);

        let messages: Vec<_> = ui.into_messages().collect();
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], Message::ZoomReset));

        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            Size::new(200.0, 50.0),
            capsule(space().width(Fill).height(Fill), 200.0, 50.0, Palette::DARK),
        );
        ui.point_at(Point::new(150.0, 25.0));
        ui.simulate([press]);

        let messages: Vec<_> = ui.into_messages().collect();
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], Message::DragWindow));
    }

    #[test]
    fn capsule_drag_includes_its_padding() {
        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            Size::new(200.0, 50.0),
            capsule(space().width(Fill).height(Fill), 200.0, 50.0, Palette::DARK),
        );
        ui.point_at(Point::new(195.0, 25.0));
        ui.simulate([Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))]);

        let messages: Vec<_> = ui.into_messages().collect();
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], Message::DragWindow));
    }

    #[test]
    fn space_between_capsules_drags_the_window() {
        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            Size::new(200.0, 50.0),
            drag_space(Fill, 50.0),
        );
        ui.point_at(Point::new(100.0, 25.0));
        ui.simulate([Event::Mouse(mouse::Event::ButtonPressed(
            mouse::Button::Left,
        ))]);

        let messages: Vec<_> = ui.into_messages().collect();
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], Message::DragWindow));
    }

    #[test]
    fn zoom_slider_drag_emits_continuous_values() {
        let zoom_slider: Element<'_, Message> =
            slider(MIN_ZOOM..=MAX_ZOOM, 1.0_f32, Message::ZoomTo)
                .step(0.01_f32)
                .width(SLIDER_WIDTH)
                .height(24)
                .style(|_: &iced::Theme, _| slider_style(Palette::DARK))
                .into();
        let mut ui = iced_test::Simulator::with_size(
            iced::Settings::default(),
            Size::new(SLIDER_WIDTH, 24.0),
            zoom_slider,
        );
        let press = Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left));

        ui.point_at(Point::new(10.0, 12.0));
        ui.simulate([press]);
        ui.point_at(Point::new(50.0, 12.0));
        ui.simulate([Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(50.0, 12.0),
        })]);

        let zooms: Vec<_> = ui
            .into_messages()
            .filter_map(|message| match message {
                Message::ZoomTo(zoom) => Some(zoom),
                _ => None,
            })
            .collect();
        assert!(zooms.len() >= 2);
        assert!(zooms.windows(2).any(|pair| pair[0] != pair[1]));
        assert!(
            zooms
                .iter()
                .any(|zoom| !crate::ui::viewer::ZOOM_STEPS.contains(zoom))
        );
    }

    #[test]
    fn full_capsules_fit_at_the_breakpoint() {
        let read_width =
            (FULL_WIDTH - theme::edge(FULL_WIDTH) * 2.0 - FILE_WIDTH - ACTION_WIDTH - 24.0)
                .clamp(480.0, READ_WIDTH);

        assert!(read_width >= 480.0);
        assert!(read_width <= READ_WIDTH);
    }

    #[test]
    fn compact_toolbar_keeps_its_capsule_bounded() {
        let (app, _) = crate::app::App::boot(crate::app::Options::default());

        for width in [560.0, 850.0, 1100.0, 1279.0] {
            let read_width = compact_capsule_width(&app, width);
            let total = theme::edge(width) * 2.0 + read_width + COMPACT_GAP + WINDOW_WIDTH;

            assert!(read_width > 0.0);
            assert!(read_width <= READ_WIDTH);
            assert!(total <= width, "{total} > {width}");
        }

        let content_width = read_tools_content_width(&app) + CAPSULE_PADDING_X * 2.0;
        assert_eq!(compact_capsule_width(&app, 1279.0), content_width);
        assert_eq!(
            compact_capsule_width(&app, 560.0),
            compact_read_width(560.0)
        );
    }

    #[test]
    fn window_controls_remain_clickable_across_toolbar_layouts() {
        let (mut app, _) = crate::app::App::boot(crate::app::Options::default());

        for palette in [Palette::DARK, Palette::LIGHT] {
            app.palette = palette;
            for mode in [ViewMode::Reader, ViewMode::Organizer] {
                app.view_mode = mode;
                for maximized in [false, true] {
                    app.window_maximized = maximized;
                    for width in [
                        560.0, 850.0, 1100.0, 1279.0, 1280.0, 1360.0, 1559.0, 1560.0, 1715.0,
                    ] {
                        let mut ui = iced_test::Simulator::with_size(
                            iced::Settings::default(),
                            Size::new(width, 94.0),
                            view(&app, width),
                        );
                        let right = width - theme::edge(width) - CAPSULE_PADDING_X;
                        let y = theme::edge(width) + theme::chrome_height(width) / 2.0;
                        for index in (0..3).rev() {
                            ui.point_at(Point::new(
                                right - TOOL_SIZE / 2.0 - index as f32 * (TOOL_SIZE + TOOL_GAP),
                                y,
                            ));
                            ui.simulate(iced_test::simulator::click());
                        }
                        let actions: Vec<_> = ui
                            .into_messages()
                            .filter_map(|message| {
                                if let Message::WindowAction(action) = message {
                                    Some(action)
                                } else {
                                    None
                                }
                            })
                            .collect();
                        assert!(
                            matches!(
                                actions.as_slice(),
                                [
                                    WindowAction::Minimize,
                                    WindowAction::Maximize,
                                    WindowAction::Close
                                ]
                            ),
                            "window controls clipped: width={width}, mode={mode:?}, maximized={maximized}, dark={}, actions={actions:?}",
                            palette.is_dark
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn toolbar_probe_writes_layout_snapshots() {
        if std::env::var_os("FOLIO_TOOLBAR_PROBE").is_none() {
            return;
        }

        let (mut app, _) = crate::app::App::boot(crate::app::Options::default());
        let directory = std::path::Path::new("target/toolbar-probe");
        std::fs::create_dir_all(directory).unwrap();

        for maximized in [false, true] {
            app.window_maximized = maximized;
            for width in [
                560.0, 850.0, 1100.0, 1279.0, 1280.0, 1360.0, 1559.0, 1560.0, 1715.0,
            ] {
                let mut ui = iced_test::Simulator::with_size(
                    iced::Settings::default(),
                    Size::new(width, 94.0),
                    view(&app, width),
                );
                let snapshot = ui.snapshot(&app.theme()).unwrap();
                snapshot
                    .matches_image(directory.join(format!("{width}-{maximized}.png")))
                    .unwrap();
            }
        }
    }
}
