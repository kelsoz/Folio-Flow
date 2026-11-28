//! Shared export/compression panel and its options.
use iced::widget::{
    button, column, container, mouse_area, opaque, row, scrollable, space, stack, text,
};
use iced::{Center, Color, Element, Fill, Length, Padding};

use super::theme;
use crate::app::{App, Message};
use crate::engine::raster::{Format, Settings};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Export,
    Compress,
    Leave,
    Settings,
    About,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Markdown,
    Jpg,
    Png,
}
#[derive(Debug, Clone, Copy)]
pub enum Operation {
    Markdown,
    Compact,
    Raster(Settings),
}

pub struct State {
    pub kind: Kind,
    pub format: ExportFormat,
    pub scale: f32,
    pub image_pdf: bool,
    pub dpi: f32,
    pub quality: u8,
    pub estimate: Option<Result<u64, String>>,
    pub choosing: bool,
    pub original_bytes: Option<u64>,
}

impl State {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            format: ExportFormat::Markdown,
            scale: 1.0,
            image_pdf: false,
            dpi: 144.0,
            quality: 75,
            estimate: None,
            choosing: false,
            original_bytes: None,
        }
    }
    pub fn operation(&self) -> Operation {
        match self.kind {
            Kind::Leave | Kind::Settings | Kind::About => Operation::Compact,
            Kind::Compress if !self.image_pdf => Operation::Compact,
            Kind::Compress => Operation::Raster(Settings {
                format: Format::Pdf,
                dpi: self.dpi,
                quality: self.quality,
            }),
            Kind::Export if self.format == ExportFormat::Markdown => Operation::Markdown,
            Kind::Export => Operation::Raster(Settings {
                format: if self.format == ExportFormat::Png {
                    Format::Png
                } else {
                    Format::Jpg
                },
                dpi: 72.0 * self.scale,
                quality: self.quality,
            }),
        }
    }
}

fn rule(palette: theme::Palette) -> Element<'static, Message> {
    container(space().height(1))
        .width(Fill)
        .style(move |_| theme::rule(palette))
        .into()
}

fn note(value: impl Into<String>, palette: theme::Palette) -> Element<'static, Message> {
    text(value.into())
        .size(12)
        .line_height(1.6)
        .color(palette.muted)
        .width(Fill)
        .into()
}

// Keep labels close to their controls; the parent column separates groups.
fn field<'a>(
    label: Element<'a, Message>,
    control: impl Into<Element<'a, Message>>,
) -> Element<'a, Message> {
    column![label, control.into()].spacing(9).width(Fill).into()
}

fn choice<'a>(
    name: &'a str,
    active: bool,
    enabled: bool,
    message: Message,
    palette: theme::Palette,
) -> Element<'a, Message> {
    button(
        text(name)
            .size(12)
            .line_height(1.6)
            .width(Fill)
            .align_x(Center),
    )
    .padding([6, 12])
    .width(Fill)
    .on_press_maybe(enabled.then_some(message))
    .style(move |_, status| {
        let mut style = theme::ghost_button(palette, status, false);
        if active {
            style.background = Some(palette.surface.into());
            style.shadow = iced::Shadow {
                color: Color::from_rgba(0.0, 0.0, 0.0, 0.12),
                offset: iced::Vector::new(0.0, 1.0),
                blur_radius: 4.0,
            };
        }
        style
    })
    .into()
}

fn segment<'a>(items: Vec<Element<'a, Message>>, palette: theme::Palette) -> Element<'a, Message> {
    container(iced::widget::Row::with_children(items).spacing(3))
        .padding(3)
        .width(Fill)
        .style(move |_| container::Style {
            background: Some(palette.raised.into()),
            border: iced::Border {
                color: palette.border,
                width: 1.0,
                radius: 9.0.into(),
            },
            ..Default::default()
        })
        .into()
}

fn title<'a>(value: &'a str, palette: theme::Palette) -> Element<'a, Message> {
    row![
        text(value)
            .font(super::strong())
            .size(19)
            .color(palette.text),
        space().width(Fill),
        button(super::icon::Icon::Close.view(16.0, palette.muted))
            .padding(6)
            .on_press(Message::DialogClose)
            .style(move |_, status| theme::ghost_button(palette, status, false))
    ]
    .align_y(Center)
    .into()
}

fn dimensions(app: &App, settings: Settings) -> String {
    app.viewer
        .document()
        .and_then(|doc| doc.pages.get(app.viewer.current_page() as usize))
        .map(|page| match settings.dimensions(*page) {
            Ok((w, h)) => format!("{w} × {h} 像素"),
            Err(_) => "尺寸过大".into(),
        })
        .unwrap_or_default()
}

fn dimension_label<'a>(
    label: &'a str,
    value: String,
    palette: theme::Palette,
) -> Element<'a, Message> {
    row![
        text(label).size(12).line_height(1.6).color(palette.muted),
        space().width(Fill),
        text(value).size(12).line_height(1.6).color(palette.muted)
    ]
    .align_y(Center)
    .into()
}

pub fn view(app: &App, size: iced::Size) -> Element<'_, Message> {
    let state = app.dialog.as_ref().expect("visible dialog");
    let p = app.palette;
    let enabled = !state.choosing;
    let secondary = |label: &'static str, message| {
        button(text(label).size(12))
            .padding([8, 16])
            .on_press(message)
            .style(move |_, status| theme::ghost_button(p, status, false))
    };
    let primary = |label: &'static str, message| {
        button(text(label).size(12).font(super::strong()))
            .padding([8, 16])
            .on_press(message)
            .style(move |_, status| theme::primary_button(p, status))
    };
    let header = match state.kind {
        Kind::Export => "导出",
        Kind::Compress => "压缩 PDF",
        Kind::Leave => "保存更改？",
        Kind::Settings => "设置",
        Kind::About => "关于",
    };
    let mut content = column![title(header, p)].spacing(20).width(Fill);
    match state.kind {
        Kind::Settings => {
            let theme_options = segment(
                vec![
                    choice(
                        "浅色",
                        !p.is_dark,
                        true,
                        if p.is_dark {
                            Message::ToggleTheme
                        } else {
                            Message::DialogIgnore
                        },
                        p,
                    ),
                    choice(
                        "深色",
                        p.is_dark,
                        true,
                        if p.is_dark {
                            Message::DialogIgnore
                        } else {
                            Message::ToggleTheme
                        },
                        p,
                    ),
                ],
                p,
            );
            content = content
                .push(note("偏好", p))
                .push(
                    row![
                        text("外观").size(13),
                        space().width(Fill),
                        container(theme_options).width(148)
                    ]
                    .align_y(Center),
                )
                .push(rule(p))
                .push(
                    row![
                        column![
                            text("记住阅读位置").size(13),
                            note("再次打开 PDF 时继续阅读", p)
                        ]
                        .spacing(4)
                        .width(Fill),
                        iced::widget::toggler(app.remember_reading)
                            .on_toggle(|_| Message::ToggleRemember)
                            .size(21)
                            .style(move |_, status| theme::toggle(p, status))
                    ]
                    .align_y(Center),
                )
                .push(rule(p))
                .push(
                    row![
                        column![
                            text("默认 PDF 阅读器").size(13),
                            note("在 Windows 设置中确认", p)
                        ]
                        .spacing(4)
                        .width(Fill),
                        secondary("设为默认…", Message::DefaultPdf)
                    ]
                    .align_y(Center),
                )
                .push(rule(p))
                .push(
                    button(
                        row![
                            text("关于 Folio Flow").size(13),
                            space().width(Fill),
                            super::icon::Icon::ChevronRight.view(16.0, p.muted)
                        ]
                        .align_y(Center),
                    )
                    .width(Fill)
                    .padding([8, 0])
                    .on_press(Message::ShowAbout)
                    .style(move |_, status| theme::ghost_button(p, status, false)),
                )
                .push(rule(p))
                .push(
                    row![
                        button(text("清除阅读记录").size(12).color(p.danger))
                            .padding(0)
                            .on_press(Message::ClearReading)
                            .style(move |_, status| {
                                let mut style = theme::ghost_button(p, status, false);
                                style.text_color = p.danger;
                                style
                            }),
                        space().width(Fill),
                        primary("完成", Message::DialogClose)
                    ]
                    .align_y(Center),
                );
        }
        Kind::About => {
            content = content
                .push(secondary("‹ 返回设置", Message::ShowSettings).padding(0))
                .push(
                    container(
                        column![
                            super::icon::Icon::DoublePage.view(44.0, p.text),
                            text("Folio Flow").size(23),
                            text(format!("版本 {}", env!("CARGO_PKG_VERSION")))
                                .size(12)
                                .color(p.faint)
                                .width(Fill)
                                .align_x(Center),
                            text("轻盈的 PDF 阅读与页面编辑工具。\n让阅读、整理与导出更顺手。")
                                .size(13)
                                .color(p.muted)
                                .align_x(Center),
                            row![
                                secondary("项目主页 ↗", Message::OpenProject(false)),
                                secondary("更新说明 ↗", Message::OpenProject(true))
                            ]
                            .spacing(8)
                        ]
                        .spacing(16)
                        .align_x(Center),
                    )
                    .width(Fill)
                    .center_x(Fill)
                    .padding([12, 0]),
                )
                .push(rule(p))
                .push(
                    row![
                        note("基于开源项目 Folio", p),
                        secondary("检查更新 ↗", Message::OpenProject(true))
                    ]
                    .align_y(Center),
                );
        }
        Kind::Leave => {
            content = content
                .push(note("文档有未保存的页面编辑。", p))
                .push(rule(p))
                .push(
                    row![
                        secondary("取消", Message::DialogClose),
                        space().width(Fill),
                        secondary(
                            "不保存",
                            Message::LeaveChoice(crate::app::LeaveChoice::Discard)
                        ),
                        primary(
                            "保存并继续",
                            Message::LeaveChoice(crate::app::LeaveChoice::Save)
                        )
                    ]
                    .spacing(8),
                );
        }
        Kind::Export | Kind::Compress => {
            if state.kind == Kind::Export {
                content = content.push(field(
                    note("文件格式", p),
                    row![
                        format_card(
                            "Markdown",
                            "提取文字",
                            state.format == ExportFormat::Markdown,
                            enabled,
                            Message::DialogFormat(ExportFormat::Markdown),
                            p
                        ),
                        format_card(
                            "JPG",
                            "体积更小",
                            state.format == ExportFormat::Jpg,
                            enabled,
                            Message::DialogFormat(ExportFormat::Jpg),
                            p
                        ),
                        format_card(
                            "PNG",
                            "清晰无损",
                            state.format == ExportFormat::Png,
                            enabled,
                            Message::DialogFormat(ExportFormat::Png),
                            p
                        ),
                    ]
                    .spacing(8),
                ));
                if state.format == ExportFormat::Markdown {
                    content = content.push(note("保留文字与基本排版，保存为 .md 文件。", p));
                } else if let Operation::Raster(settings) = state.operation() {
                    content = content.push(field(
                        dimension_label("图片尺寸", dimensions(app, settings), p),
                        segment(
                            vec![
                                choice(
                                    "1×",
                                    state.scale == 1.0,
                                    enabled,
                                    Message::DialogScale(1.0),
                                    p,
                                ),
                                choice(
                                    "1.5×",
                                    state.scale == 1.5,
                                    enabled,
                                    Message::DialogScale(1.5),
                                    p,
                                ),
                                choice(
                                    "2×",
                                    state.scale == 2.0,
                                    enabled,
                                    Message::DialogScale(2.0),
                                    p,
                                ),
                                choice(
                                    "3×",
                                    state.scale == 3.0,
                                    enabled,
                                    Message::DialogScale(3.0),
                                    p,
                                ),
                            ],
                            p,
                        ),
                    ));
                }
            } else {
                content = content.push(field(
                    note("压缩方式", p),
                    segment(
                        vec![
                            choice(
                                "无损整理",
                                !state.image_pdf,
                                enabled,
                                Message::DialogImagePdf(false),
                                p,
                            ),
                            choice(
                                "纯图压缩",
                                state.image_pdf,
                                enabled,
                                Message::DialogImagePdf(true),
                                p,
                            ),
                        ],
                        p,
                    ),
                ));
                if state.image_pdf {
                    if let Operation::Raster(settings) = state.operation() {
                        content = content.push(field(
                            dimension_label("清晰度", dimensions(app, settings), p),
                            segment(
                                vec![
                                    choice(
                                        "轻巧",
                                        state.dpi == 72.0,
                                        enabled,
                                        Message::DialogDpi(72.0),
                                        p,
                                    ),
                                    choice(
                                        "标准",
                                        state.dpi == 144.0,
                                        enabled,
                                        Message::DialogDpi(144.0),
                                        p,
                                    ),
                                    choice(
                                        "高清",
                                        state.dpi == 216.0,
                                        enabled,
                                        Message::DialogDpi(216.0),
                                        p,
                                    ),
                                ],
                                p,
                            ),
                        ));
                    }
                } else {
                    content = content.push(note(
                        "保留内容与画质，优化文件结构。体积可能不变或增大。",
                        p,
                    ));
                }
            }
            if let Operation::Raster(settings) = state.operation() {
                if settings.format != Format::Png {
                    content = content.push(field(
                        note("图片质量", p),
                        segment(
                            vec![
                                choice(
                                    "体积优先",
                                    state.quality == 60,
                                    enabled,
                                    Message::DialogQuality(60),
                                    p,
                                ),
                                choice(
                                    "均衡",
                                    state.quality == 75,
                                    enabled,
                                    Message::DialogQuality(75),
                                    p,
                                ),
                                choice(
                                    "画质优先",
                                    state.quality == 90,
                                    enabled,
                                    Message::DialogQuality(90),
                                    p,
                                ),
                            ],
                            p,
                        ),
                    ));
                }
                if settings.format == Format::Pdf {
                    content = content.push(
                        container(note(
                            "仅保留页面外观，文字将无法搜索或复制，链接、书签与表单数据也会移除。",
                            p,
                        ))
                        .padding(12)
                        .style(move |_| container::Style {
                            background: Some(p.raised.into()),
                            border: iced::Border {
                                radius: 8.0.into(),
                                ..Default::default()
                            },
                            ..Default::default()
                        }),
                    );
                } else {
                    content = content.push(note("每页一张图片，保存到新文件夹。", p));
                }
                if app
                    .viewer
                    .document()
                    .is_some_and(|doc| doc.pages.windows(2).any(|pages| pages[0] != pages[1]))
                {
                    content = content.push(note("尺寸以当前页为参考，各页按自身尺寸输出。", p));
                }
            }
            if state.kind == Kind::Compress {
                let estimate = if state.image_pdf {
                    match &state.estimate {
                        None => "估算中…".into(),
                        Some(Ok(bytes)) => format!("≈ {}", crate::app::format_bytes(*bytes)),
                        Some(Err(_)) => "暂无法估算".into(),
                    }
                } else {
                    "待估算".into()
                };
                let original = state
                    .original_bytes
                    .map(crate::app::format_bytes)
                    .unwrap_or_else(|| "—".into());
                content = content.push(
                    container(
                        row![
                            column![note("原文件", p), text(original).size(14)].spacing(4),
                            space().width(Fill),
                            column![note("预计大小", p), text(estimate).size(18)].spacing(4)
                        ]
                        .align_y(Center),
                    )
                    .padding(16)
                    .style(move |_| container::Style {
                        background: Some(p.raised.into()),
                        border: iced::Border {
                            radius: 9.0.into(),
                            ..Default::default()
                        },
                        ..Default::default()
                    }),
                );
                if state.image_pdf {
                    content = content.push(note("抽样估算，实际大小可能有差异。", p));
                }
            }
            let can_export = enabled
                && app.viewer.document().is_some_and(|doc| {
                    !doc.pages.is_empty()
                        && match state.operation() {
                            Operation::Raster(settings) => {
                                doc.pages.iter().all(|p| settings.dimensions(*p).is_ok())
                            }
                            _ => true,
                        }
                });
            let meta = if state.kind == Kind::Compress {
                "原文件不变".into()
            } else {
                format!("全部 {} 页", app.viewer.page_count())
            };
            content = content.push(rule(p)).push(
                row![
                    note(meta, p),
                    secondary("取消", Message::DialogClose),
                    button(
                        text(if state.choosing {
                            "选择保存位置…"
                        } else if state.kind == Kind::Compress {
                            "另存为副本…"
                        } else {
                            "选择保存位置…"
                        })
                        .size(12)
                        .font(super::strong())
                    )
                    .padding([8, 16])
                    .on_press_maybe(can_export.then_some(Message::DialogConfirm))
                    .style(move |_, status| theme::primary_button(p, status))
                ]
                .spacing(8)
                .align_y(Center),
            );
        }
    }
    shell(app, size, content.into())
}

fn format_card<'a>(
    name: &'a str,
    hint: &'a str,
    active: bool,
    enabled: bool,
    message: Message,
    p: theme::Palette,
) -> Element<'a, Message> {
    button(
        column![
            text(if active {
                "✓"
            } else if name == "Markdown" {
                "≡"
            } else {
                "▧"
            })
            .size(14)
            .height(18)
            .color(if active { p.text } else { p.faint }),
            space().height(12),
            column![
                text(name).size(13).line_height(1.6).font(super::strong()),
                text(hint).size(11).line_height(1.6).color(p.muted)
            ]
            .spacing(3),
        ]
        .spacing(0),
    )
    .padding([14, 10])
    .width(Fill)
    .on_press_maybe(enabled.then_some(message))
    .style(move |_, status| {
        let mut style = theme::ghost_button(p, status, false);
        if active {
            style.background = Some(p.raised.into());
        }
        style.border = iced::Border {
            color: if active { p.accent } else { p.border },
            width: 1.0,
            radius: 9.0.into(),
        };
        style
    })
    .into()
}

fn shell<'a>(
    app: &'a App,
    size: iced::Size,
    content: Element<'a, Message>,
) -> Element<'a, Message> {
    let palette = app.palette;
    let progress = app.dialog_motion.value.clamp(0.0, 1.0);
    let panel = container(
        scrollable(
            container(content)
                .width(Fill)
                .padding(Padding::ZERO.right(16)),
        )
        .height(Length::Shrink)
        .style(move |_, status| theme::scroller(palette, status)),
    )
    .padding(26)
    .width(480)
    .max_height((size.height - 64.0).max(1.0))
    .style(move |_| theme::dialog(palette, progress));
    let backdrop = mouse_area(container(space()).width(Fill).height(Fill).style(move |_| {
        container::Style {
            background: Some(
                Color::from_rgba(
                    0.0,
                    0.0,
                    0.0,
                    progress * if palette.is_dark { 0.42 } else { 0.2 },
                )
                .into(),
            ),
            ..container::Style::default()
        }
    }))
    .on_press(Message::DialogClose)
    .on_scroll(|_| Message::DialogIgnore);
    stack![
        opaque(backdrop),
        container(mouse_area(opaque(panel)).on_scroll(|_| Message::DialogIgnore))
            .center(Fill)
            .padding(Padding {
                top: (1.0 - progress) * 24.0,
                ..Padding::new(24.0)
            })
    ]
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Options;
    use crate::motion::Tween;
    use std::time::{Duration, Instant};

    #[test]
    fn shared_dialog_layouts_keep_cancel_reachable() {
        let (mut app, _) = App::boot(Options {
            file: Some(crate::engine::tests::fixture_path()),
            ..Options::default()
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        while app.viewer.document().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(2));
            let _ = app.update(Message::Engine);
        }
        let snapshots = std::env::var_os("FOLIO_DIALOG_SNAPSHOTS").is_some();
        for palette in [theme::Palette::DARK, theme::Palette::LIGHT] {
            app.palette = palette;
            for (width, height) in [(560.0, 380.0), (1360.0, 870.0)] {
                for variant in 0..8 {
                    let mut state = State::new(match variant {
                        0..=2 => Kind::Export,
                        5 => Kind::Leave,
                        6 => Kind::Settings,
                        7 => Kind::About,
                        _ => Kind::Compress,
                    });
                    state.format = match variant {
                        1 => ExportFormat::Jpg,
                        2 => ExportFormat::Png,
                        _ => ExportFormat::Markdown,
                    };
                    state.image_pdf = variant == 4;
                    app.dialog = Some(state);
                    app.dialog_motion = Tween::new(1.0);
                    let mut ui = iced_test::Simulator::with_size(
                        iced::Settings {
                            default_font: super::super::font(),
                            ..iced::Settings::default()
                        },
                        iced::Size::new(width, height),
                        super::super::view_at(&app, iced::Size::new(width, height)),
                    );
                    if snapshots {
                        ui.snapshot(&app.theme())
                            .unwrap()
                            .matches_image(format!(
                                "../TEMP/v041-dialog-corrected/{width}-{}-{variant}.png",
                                palette.is_dark
                            ))
                            .unwrap();
                    }
                    // Short windows scroll the dialog itself; they must not spill into the PDF view.
                    if height < 400.0 {
                        ui.point_at(iced::Point::new(width / 2.0, height / 2.0));
                        ui.simulate([iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                            delta: iced::mouse::ScrollDelta::Lines { x: 0.0, y: -100.0 },
                        })]);
                    }
                    ui.click(match variant {
                        6 => "完成",
                        7 => "检查更新 ↗",
                        _ => "取消",
                    })
                    .unwrap();
                    let messages: Vec<_> = ui.into_messages().collect();
                    assert!(messages.iter().any(|message| if variant == 7 {
                        matches!(message, Message::OpenProject(true))
                    } else {
                        matches!(message, Message::DialogClose)
                    }));
                    assert!(!messages.iter().any(|message| matches!(
                        message,
                        Message::Viewer(crate::ui::viewer::Interaction::Scrolled { .. })
                    )));
                }
            }
        }
    }
}
