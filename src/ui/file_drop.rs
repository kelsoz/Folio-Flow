//! External file drag preview; the PDF worker owns the actual import.
use super::theme::Palette;
use iced::widget::canvas::{self, Frame};
use iced::{Point, Rectangle, Size};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Sidebar,
    Organizer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Target {
    pub zone: Zone,
    pub slot: u32,
}

#[derive(Default)]
pub struct State {
    pub path: Option<PathBuf>,
    pub hovering: bool,
    pub importing: bool,
    pub target: Option<Target>,
    pub settling_doc: Option<u64>,
}

impl State {
    pub fn slot(&self, zone: Zone) -> Option<u32> {
        if self.path.as_deref().is_none_or(|path| !supported(path)) {
            return None;
        }
        self.target
            .filter(|target| target.zone == zone)
            .map(|target| target.slot)
    }
    pub fn label(&self) -> &'static str {
        if self.path.as_deref().is_some_and(is_pdf) {
            "插入 PDF 全部页面"
        } else {
            "插入图片页"
        }
    }
}

pub fn is_pdf(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

pub fn supported(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        [
            "pdf", "png", "jpg", "jpeg", "webp", "bmp", "gif", "tif", "tiff",
        ]
        .iter()
        .any(|value| extension.eq_ignore_ascii_case(value))
    })
}

pub fn marker(frame: &mut Frame, rect: Rectangle, label: &str, palette: Palette) {
    frame.fill_rectangle(rect.position(), rect.size(), palette.accent_soft);
    frame.fill_rectangle(rect.position(), Size::new(3.0, rect.height), palette.accent);
    frame.fill_text(canvas::Text {
        content: label.into(),
        position: Point::new(rect.center_x(), rect.center_y()),
        color: palette.text,
        size: 12.0.into(),
        font: super::font(),
        align_x: iced::advanced::text::Alignment::Center,
        align_y: iced::alignment::Vertical::Center,
        ..canvas::Text::default()
    });
}
