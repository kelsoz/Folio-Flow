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
            Kind::Leave => Operation::Compact,
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

pub fn view(app: &App, size: iced::Size) -> Element<'_, Message> {
    let state = app.dialog.as_ref().expect("visible dialog");
    let palette = app.palette;
    if state.kind == Kind::Leave {
        let content = column![
            text("保存文档更改？")
                .font(super::strong())
                .size(22)
                .color(palette.text),
            text("当前文档包含未保存的页面编辑。保存后继续，或放弃更改。")
                .size(13)
                .color(palette.muted)
                .width(Fill),
            row![
                space().width(Fill),
                button(text("取消"))
                    .padding([9, 12])
                    .on_press(Message::DialogClose)
                    .style(move |_, status| theme::ghost_button(palette, status, false)),
                button(text("不保存"))
                    .padding([9, 12])
                    .on_press(Message::LeaveChoice(crate::app::LeaveChoice::Discard))
                    .style(move |_, status| theme::ghost_button(palette, status, false)),
                button(text("保存并继续"))
                    .padding([9, 12])
                    .on_press(Message::LeaveChoice(crate::app::LeaveChoice::Save))
                    .style(move |_, status| theme::primary_button(palette, status))
            ]
            .spacing(8)
        ]
        .spacing(20)
        .width(Fill);
        return shell(app, size, content.into());
    }
    let title = if state.kind == Kind::Export {
        "导出文档"
    } else {
        "压缩 PDF"
    };
    let label = |value: &str| {
        text(value.to_owned())
            .size(13)
            .color(palette.muted)
            .width(Fill)
    };
    let choice = |name: &str, active, message| -> Element<'_, Message> {
        button(text(name.to_owned()).size(13))
            .padding([9, 14])
            .on_press_maybe((!state.choosing).then_some(message))
            .style(move |_, status| theme::ghost_button(palette, status, active))
            .into()
    };
    let mut content = column![
        row![
            text(title)
                .font(super::strong())
                .size(22)
                .color(palette.text),
            space().width(Fill),
            button(text("关闭").size(12))
                .on_press(Message::DialogClose)
                .style(move |_, status| theme::ghost_button(palette, status, false))
        ]
        .align_y(Center),
        label("导出当前文档的全部页面，保留当前页序与旋转。"),
    ]
    .spacing(16);

    if let Some(bytes) = state.original_bytes {
        content = content.push(label(&format!(
            "原文件大小：{}",
            crate::app::format_bytes(bytes)
        )));
    }
    if state.kind == Kind::Export {
        content = content.push(
            row![
                choice(
                    "Markdown",
                    state.format == ExportFormat::Markdown,
                    Message::DialogFormat(ExportFormat::Markdown)
                ),
                choice(
                    "JPG 图片",
                    state.format == ExportFormat::Jpg,
                    Message::DialogFormat(ExportFormat::Jpg)
                ),
                choice(
                    "PNG 图片",
                    state.format == ExportFormat::Png,
                    Message::DialogFormat(ExportFormat::Png)
                )
            ]
            .spacing(8),
        );
        if state.format == ExportFormat::Markdown {
            content = content.push(label("提取文字与基本排版，保存为 Markdown（.md）文件。"));
        } else {
            content = content
                .push(label("图片大小 · 72 DPI = 1×"))
                .push(
                    row![
                        choice("1×", state.scale == 1.0, Message::DialogScale(1.0)),
                        choice("1.5×", state.scale == 1.5, Message::DialogScale(1.5)),
                        choice("2×", state.scale == 2.0, Message::DialogScale(2.0)),
                        choice("3×", state.scale == 3.0, Message::DialogScale(3.0))
                    ]
                    .spacing(8),
                )
                .push(label("每页单独保存，自动放入所选目录下的新文件夹。"));
        }
    } else {
        content = content.push(
            row![
                choice("无损整理", !state.image_pdf, Message::DialogImagePdf(false)),
                choice("纯图 PDF", state.image_pdf, Message::DialogImagePdf(true))
            ]
            .spacing(8),
        );
        if state.image_pdf {
            content = content.push(label("图片清晰度"))
                .push(row![choice("轻巧 · 72 DPI", state.dpi == 72.0, Message::DialogDpi(72.0)),
                    choice("标准 · 144 DPI", state.dpi == 144.0, Message::DialogDpi(144.0)),
                    choice("高清 · 216 DPI", state.dpi == 216.0, Message::DialogDpi(216.0))].spacing(6))
                .push(text("每页转为一张图片。文字、链接、书签、表单等数据将移除，批注外观保留在图片中；原文件不变。")
                    .size(13).color(palette.muted).width(Fill));
        } else {
            content = content
                .push(label(
                    "重新整理 PDF 存储结构，保留当前文档内容，不降低画质。",
                ))
                .push(label("效果取决于原文件，整理后也可能更大。"));
        }
    }

    if let Operation::Raster(settings) = state.operation() {
        if settings.format != Format::Png {
            content = content.push(label("JPEG 质量")).push(
                row![
                    choice(
                        "体积优先 · 60",
                        state.quality == 60,
                        Message::DialogQuality(60)
                    ),
                    choice("均衡 · 75", state.quality == 75, Message::DialogQuality(75)),
                    choice(
                        "画质优先 · 90",
                        state.quality == 90,
                        Message::DialogQuality(90)
                    )
                ]
                .spacing(6),
            );
        }
        if let Some(doc) = app.viewer.document() {
            let first = settings.dimensions(doc.pages[0]);
            let current = doc
                .pages
                .get(app.viewer.current_page() as usize)
                .copied()
                .unwrap_or(doc.pages[0]);
            let current_dimensions = settings.dimensions(current);
            let base = Settings {
                dpi: 72.0,
                ..settings
            }
            .dimensions(current);
            let mixed = doc.pages.iter().any(|p| *p != doc.pages[0]);
            let dimensions = match (base, current_dimensions, first) {
                (Ok((bw, bh)), Ok((w, h)), Ok(_)) => format!(
                    "当前页：原始 1× {bw} × {bh} 像素 → 输出 {w} × {h} 像素\n共 {} 页 · {} DPI{}",
                    doc.pages.len(),
                    settings.dpi,
                    if mixed {
                        " · 各页按自身尺寸输出"
                    } else {
                        ""
                    }
                ),
                _ => "当前尺寸过大，请降低倍率或清晰度。".into(),
            };
            content = content.push(label(&dimensions));
        }
        if settings.format == Format::Pdf {
            let estimate = match &state.estimate {
                None => "正在抽样预估文件大小…".into(),
                Some(Ok(bytes)) => format!(
                    "预计大小：约 {}（抽样估计，实际可能明显偏离）",
                    crate::app::format_bytes(*bytes)
                ),
                Some(Err(error)) => format!("暂时无法预估：{error}"),
            };
            content = content.push(label(&estimate));
        }
    }

    let can_export = !state.choosing
        && app.viewer.document().is_some_and(|doc| {
            !doc.pages.is_empty()
                && match state.operation() {
                    Operation::Raster(settings) => {
                        doc.pages.iter().all(|p| settings.dimensions(*p).is_ok())
                    }
                    _ => true,
                }
        });
    content = content.push(
        row![
            space().width(Fill),
            button(text("取消"))
                .padding([10, 18])
                .on_press(Message::DialogClose)
                .style(move |_, status| theme::ghost_button(palette, status, false)),
            button(text(if state.choosing {
                "选择保存位置…"
            } else {
                "选择位置并保存"
            }))
            .padding([10, 18])
            .on_press_maybe(can_export.then_some(Message::DialogConfirm))
            .style(move |_, status| theme::primary_button(palette, status))
        ]
        .spacing(10),
    );

    shell(app, size, content.width(Fill).into())
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
        .height(Length::Shrink),
    )
    .padding(28)
    .width(560)
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
                for variant in 0..6 {
                    let mut state = State::new(match variant {
                        0..=2 => Kind::Export,
                        5 => Kind::Leave,
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
                                "target/daily-dialog-review/{width}-{}-{variant}.png",
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
                    ui.click("取消").unwrap();
                    let messages: Vec<_> = ui.into_messages().collect();
                    assert!(
                        messages
                            .iter()
                            .any(|message| matches!(message, Message::DialogClose))
                    );
                    assert!(!messages.iter().any(|message| matches!(
                        message,
                        Message::Viewer(crate::ui::viewer::Interaction::Scrolled { .. })
                    )));
                }
            }
        }
    }
}
