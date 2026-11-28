//! Colours and widget styles.
//!
//! Every surface draws from the [`Palette`] tokens below, so the two themes
//! differ only in these values.

use iced::widget::{button, container, scrollable, text_input};
use iced::{Background, Border, Color, Padding, Shadow, Vector, border, gradient};

pub const EDGE: f32 = 20.0;
pub const SIDEBAR_BOTTOM: f32 = 22.0;
pub const CHROME_HEIGHT: f32 = 54.0;
pub const SIDEBAR_WIDTH: f32 = 222.0;
pub const CONTROL: f32 = 30.0;
pub const CONTROL_RADIUS: f32 = 6.0;
pub const SCROLLBAR_RADIUS: f32 = 4.0;
pub const ROW_RADIUS: f32 = 5.0;
pub const FIELD_RADIUS: f32 = 5.0;
pub const SIDEBAR_ROW_RADIUS: f32 = 7.0;
pub const CARD_RADIUS: f32 = 8.0;
pub const TAB_RADIUS: f32 = 12.0;
pub const TAB_GROUP_RADIUS: f32 = 16.0;
pub const PANEL_RADIUS: f32 = 16.0;
pub const CAPSULE_RADIUS: f32 = 12.0;
pub const PROGRESS_RADIUS: f32 = 2.0;
pub const SLIDER_HANDLE_RADIUS: f32 = 6.0;
pub const PAPER: Color = Color::WHITE;
pub const TEXT: f32 = 13.0;
pub const TEXT_SMALL: f32 = 12.0;

pub fn edge(_width: f32) -> f32 {
    EDGE
}

pub fn chrome_height(_width: f32) -> f32 {
    CHROME_HEIGHT
}

pub fn content_top(_width: f32) -> f32 {
    EDGE + CHROME_HEIGHT + 28.0
}

fn smooth_range(width: f32, start: f32, end: f32) -> f32 {
    let t = ((width - start) / (end - start)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn sidebar_width(width: f32) -> f32 {
    let compact = 168.0 + 22.0 * smooth_range(width, 720.0, 900.0);
    compact + (SIDEBAR_WIDTH - 190.0) * smooth_range(width, 940.0, 1100.0)
}

/// The viewer is centred inside its bounds. These insets place the paper at
/// the V2 design coordinates and reserve space for the animated sidebar.
pub fn reader_insets(width: f32, sidebar_progress: f32) -> Padding {
    let (open_left, right) = if width < 850.0 {
        (204.0, 20.0)
    } else if width < 1020.0 {
        (238.0, 24.0)
    } else {
        (280.0, 36.0)
    };
    let progress = sidebar_progress.clamp(0.0, 1.0);
    let left = right + (open_left - right) * progress;

    Padding {
        left: (left - 24.0_f32).max(0.0),
        right: (right - 24.0_f32).max(0.0),
        top: (content_top(width) + 6.0 - 20.0).max(0.0),
        bottom: 0.0,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub is_dark: bool,
    /// Behind the pages.
    pub canvas: Color,
    /// Toolbar and sidebar.
    pub surface: Color,
    /// Floating top capsules.
    pub toolbar: Color,
    /// Inputs and popovers sitting on a surface.
    pub raised: Color,
    pub hover: Color,
    pub pressed: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub faint: Color,
    pub accent: Color,
    pub on_accent: Color,
    /// Selected rows and toggled buttons.
    pub accent_soft: Color,
    pub selection: Color,
    /// Search matches, and the one the reader is on. Both sit on the paper,
    /// which is white in either theme.
    pub found: Color,
    pub found_current: Color,
    pub danger: Color,
    pub shadow: Color,
    pub scrollbar: Color,
}

const fn rgb(hex: u32) -> Color {
    rgba(hex, 1.0)
}

const fn rgba(hex: u32, a: f32) -> Color {
    Color {
        r: ((hex >> 16) & 0xff) as f32 / 255.0,
        g: ((hex >> 8) & 0xff) as f32 / 255.0,
        b: (hex & 0xff) as f32 / 255.0,
        a,
    }
}

impl Palette {
    pub const DARK: Self = Self {
        is_dark: true,
        canvas: rgb(0x242526),
        surface: rgb(0x2b2c2d),
        toolbar: rgb(0x303132),
        raised: rgb(0x363738),
        hover: rgb(0x414244),
        pressed: rgb(0x4a4b4d),
        border: rgb(0x424345),
        text: rgb(0xececed),
        muted: rgb(0xc2c3c5),
        faint: rgb(0x9c9da0),
        accent: rgb(0xc4c6cc),
        on_accent: rgb(0x222325),
        accent_soft: rgb(0x3b3c3f),
        selection: rgba(0x4d8ef5, 0.36),
        found: rgba(0xffc400, 0.40),
        found_current: rgba(0xff7a00, 0.58),
        danger: rgb(0xff645b),
        shadow: rgba(0x000000, 0.36),
        scrollbar: rgb(0x505154),
    };

    pub const LIGHT: Self = Self {
        is_dark: false,
        canvas: rgb(0xe5e5e7),
        surface: rgb(0xf4f4f5),
        toolbar: rgb(0xf4f4f5),
        raised: rgb(0xebebed),
        hover: rgb(0xdedee1),
        pressed: rgb(0xd2d2d6),
        border: rgb(0xc9c9cd),
        text: rgb(0x202123),
        muted: rgb(0x626268),
        faint: rgb(0x8a8a90),
        accent: rgb(0x303135),
        on_accent: rgb(0xffffff),
        accent_soft: rgb(0xd8d8dc),
        selection: rgba(0x2f6fe4, 0.30),
        found: rgba(0xffc400, 0.40),
        found_current: rgba(0xff7a00, 0.58),
        danger: rgb(0xc9423b),
        shadow: rgba(0x202123, 0.18),
        scrollbar: rgb(0xa6a6ac),
    };

    pub fn iced(&self) -> iced::Theme {
        iced::Theme::custom(
            if self.is_dark {
                "Folio Flow 暗色"
            } else {
                "Folio Flow 亮色"
            }
            .to_string(),
            iced::theme::Palette {
                background: self.canvas,
                text: self.text,
                primary: self.accent,
                success: self.accent,
                warning: self.danger,
                danger: self.danger,
            },
        )
    }
}

pub fn canvas(palette: Palette) -> container::Style {
    container::Style {
        background: Some(palette.canvas.into()),
        text_color: Some(palette.text),
        ..container::Style::default()
    }
}

fn glass_edge(palette: Palette) -> Color {
    if palette.is_dark {
        rgba(0xffffff, 0.15)
    } else {
        rgba(0xffffff, 0.78)
    }
}

fn floating_shadow(palette: Palette) -> Shadow {
    Shadow {
        color: Color {
            a: if palette.is_dark { 0.22 } else { 0.16 },
            ..palette.shadow
        },
        offset: Vector::new(0.0, if palette.is_dark { 4.0 } else { 7.0 }),
        blur_radius: if palette.is_dark { 28.0 } else { 24.0 },
    }
}

fn glass_background(palette: Palette) -> Background {
    if palette.is_dark {
        return palette.toolbar.into();
    }
    let (top_alpha, bottom_alpha) = (0.82, 0.58);
    let top = Color {
        a: top_alpha,
        ..palette.toolbar
    };
    let bottom = Color {
        a: bottom_alpha,
        ..palette.toolbar
    };
    let glass = gradient::Linear::new(90.0_f32.to_radians())
        .add_stop(0.0, top)
        .add_stop(
            0.48,
            Color {
                a: top_alpha * 0.82,
                ..palette.toolbar
            },
        )
        .add_stop(1.0, bottom);

    Background::Gradient(glass.into())
}

fn floating_surface(palette: Palette, radius: f32) -> container::Style {
    container::Style {
        background: Some(glass_background(palette)),
        text_color: Some(palette.text),
        border: Border {
            color: glass_edge(palette),
            width: 1.0,
            radius: border::radius(radius),
        },
        shadow: floating_shadow(palette),
        snap: false,
    }
}

pub fn capsule(palette: Palette) -> container::Style {
    floating_surface(palette, CAPSULE_RADIUS)
}

pub fn dialog(palette: Palette, progress: f32) -> container::Style {
    let mut style = floating_surface(palette, PANEL_RADIUS);
    style.background = Some(
        Color {
            a: 1.0,
            ..palette.surface
        }
        .into(),
    );
    style.border.color.a *= progress;
    style.shadow.color.a *= progress;
    style
}

pub fn floating_panel(palette: Palette) -> container::Style {
    let mut style = floating_surface(palette, PANEL_RADIUS);
    if palette.is_dark {
        style.background = Some(palette.surface.into());
    }
    style
}

/// A one pixel rule.
pub fn rule(palette: Palette) -> container::Style {
    container::Style {
        background: Some(palette.border.into()),
        ..container::Style::default()
    }
}

/// Tooltips, toasts and other floating surfaces.
pub fn popover(palette: Palette) -> container::Style {
    container::Style {
        background: Some(palette.raised.into()),
        text_color: Some(palette.text),
        border: Border {
            color: palette.border,
            width: 1.0,
            radius: border::radius(CONTROL_RADIUS),
        },
        shadow: Shadow {
            color: palette.shadow,
            offset: Vector::new(0.0, 4.0),
            blur_radius: 16.0,
        },
        ..container::Style::default()
    }
}

/// A flat button that only shows a background on interaction, or when
/// `active` (a toggled state).
pub fn ghost_button(palette: Palette, status: button::Status, active: bool) -> button::Style {
    let background = match status {
        button::Status::Pressed if active => Some(palette.accent),
        button::Status::Pressed => Some(palette.pressed),
        button::Status::Hovered if active => Some(palette.accent),
        button::Status::Hovered => Some(palette.hover),
        _ if active => Some(palette.accent),
        _ => None,
    };

    button::Style {
        background: background.map(Background::Color),
        text_color: match status {
            button::Status::Disabled => palette.faint,
            _ if active => palette.on_accent,
            _ => palette.text,
        },
        border: border::rounded(CONTROL_RADIUS),
        ..button::Style::default()
    }
}

pub fn ghost_button_motion(
    palette: Palette,
    status: button::Status,
    active: bool,
    hover: f32,
) -> button::Style {
    let hover = hover.clamp(0.0, 1.0);
    let mut style = ghost_button(palette, status, active);

    let hover = if matches!(status, button::Status::Hovered | button::Status::Pressed) {
        hover.max(0.35)
    } else {
        hover
    };
    if active {
        let base = palette.accent;
        style.background = Some(
            Color {
                r: base.r + (palette.text.r - base.r) * hover * 0.12,
                g: base.g + (palette.text.g - base.g) * hover * 0.12,
                b: base.b + (palette.text.b - base.b) * hover * 0.12,
                a: base.a,
            }
            .into(),
        );
    } else {
        style.background = Some(
            match status {
                button::Status::Pressed => palette.pressed,
                _ => Color {
                    a: palette.hover.a * hover,
                    ..palette.hover
                },
            }
            .into(),
        );
    }
    style.border.color = Color {
        a: 0.14 * hover,
        ..palette.text
    };
    style.shadow = Shadow {
        color: Color {
            a: palette.shadow.a * 0.24 * hover,
            ..palette.shadow
        },
        offset: Vector::new(0.0, 1.5 * hover),
        blur_radius: 10.0 * hover,
    };

    style
}

pub fn primary_button(palette: Palette, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => iced::theme::palette::mix(palette.accent, palette.text, 0.12),
        button::Status::Pressed => iced::theme::palette::mix(palette.accent, palette.canvas, 0.15),
        _ => palette.accent,
    };

    button::Style {
        background: Some(background.into()),
        text_color: palette.on_accent,
        border: border::rounded(CONTROL_RADIUS),
        ..button::Style::default()
    }
}

/// A full-width row in a list.
pub fn row_button(palette: Palette, status: button::Status, active: bool) -> button::Style {
    let mut style = ghost_button(palette, status, false);
    if active {
        style.background = Some(palette.accent_soft.into());
        style.text_color = palette.text;
    }
    style.border = border::rounded(ROW_RADIUS);
    style
}

pub fn input(palette: Palette, status: text_input::Status) -> text_input::Style {
    let focused = matches!(status, text_input::Status::Focused { .. });

    text_input::Style {
        background: if focused {
            palette.raised.into()
        } else {
            palette.hover.into()
        },
        border: Border {
            color: if focused {
                palette.accent
            } else {
                Color::TRANSPARENT
            },
            width: 1.0,
            radius: border::radius(FIELD_RADIUS),
        },
        icon: palette.muted,
        placeholder: palette.faint,
        value: palette.text,
        selection: palette.selection,
    }
}

pub fn toggle(
    palette: Palette,
    status: iced::widget::toggler::Status,
) -> iced::widget::toggler::Style {
    use iced::widget::toggler::{Status, Style};
    let on = match status {
        Status::Active { is_toggled }
        | Status::Hovered { is_toggled }
        | Status::Disabled { is_toggled } => is_toggled,
    };
    Style {
        background: if on {
            palette.accent
        } else if matches!(status, Status::Hovered { .. }) {
            palette.pressed
        } else {
            palette.hover
        }
        .into(),
        foreground: if on { palette.on_accent } else { palette.muted }.into(),
        background_border_width: 0.0,
        background_border_color: Color::TRANSPARENT,
        foreground_border_width: 0.0,
        foreground_border_color: Color::TRANSPARENT,
        text_color: Some(palette.text),
        border_radius: None,
        padding_ratio: 0.14,
    }
}

pub fn scroller(palette: Palette, status: scrollable::Status) -> scrollable::Style {
    let engaged = matches!(
        status,
        scrollable::Status::Hovered {
            is_vertical_scrollbar_hovered: true,
            ..
        } | scrollable::Status::Dragged { .. }
    );

    let rail = scrollable::Rail {
        background: None,
        border: Border::default(),
        scroller: scrollable::Scroller {
            background: if engaged {
                Color {
                    a: palette.scrollbar.a * 1.6,
                    ..palette.scrollbar
                }
            } else {
                palette.scrollbar
            }
            .into(),
            border: border::rounded(SCROLLBAR_RADIUS),
        },
    };

    scrollable::Style {
        container: container::Style::default(),
        vertical_rail: rail,
        horizontal_rail: rail,
        gap: None,
        auto_scroll: scrollable::AutoScroll {
            background: palette.raised.into(),
            border: border::rounded(CONTROL_RADIUS)
                .color(palette.border)
                .width(1.0),
            shadow: Shadow::default(),
            icon: palette.muted,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_toolbar_button_states_receive_hover_feedback() {
        for palette in [Palette::DARK, Palette::LIGHT] {
            for status in [button::Status::Active, button::Status::Disabled] {
                for active in [false, true] {
                    let idle = ghost_button_motion(palette, status, active, 0.0);
                    let hovered = ghost_button_motion(palette, status, active, 1.0);
                    assert_ne!(idle.background, hovered.background);
                    assert!(hovered.border.color.a > idle.border.color.a);
                    if status == button::Status::Disabled {
                        assert_eq!(hovered.text_color, palette.faint);
                    }
                }
            }
        }
    }

    #[test]
    fn dialog_keeps_its_background_while_its_text_is_visible() {
        for palette in [Palette::DARK, Palette::LIGHT] {
            for progress in [0.0, 0.01, 0.5, 1.0] {
                assert_eq!(
                    dialog(palette, progress).background,
                    Some(
                        Color {
                            a: 1.0,
                            ..palette.surface
                        }
                        .into()
                    )
                );
            }
        }
    }

    #[test]
    fn dark_canvas_uses_the_requested_tone() {
        assert_eq!(Palette::DARK.canvas, rgb(0x242526));
    }

    #[test]
    fn dark_active_buttons_use_a_muted_highlight() {
        assert_eq!(Palette::DARK.accent, rgb(0xc4c6cc));
    }

    #[test]
    fn reader_preview_grows_when_the_sidebar_collapses() {
        let open = reader_insets(1360.0, 1.0);
        let closed = reader_insets(1360.0, 0.0);

        assert!(closed.left < open.left);
        assert_eq!(closed.right, open.right);
    }

    #[test]
    fn dark_floating_surfaces_use_solid_fills() {
        assert_eq!(
            capsule(Palette::DARK).background,
            Some(Palette::DARK.toolbar.into())
        );
        assert_eq!(
            floating_panel(Palette::DARK).background,
            Some(Palette::DARK.surface.into())
        );
    }

    #[test]
    fn light_toolbar_glass_is_translucent() {
        let Some(Background::Gradient(gradient::Gradient::Linear(glass))) =
            capsule(Palette::LIGHT).background
        else {
            panic!("toolbar glass should use a linear gradient");
        };

        assert!(
            glass
                .stops
                .into_iter()
                .flatten()
                .all(|stop| stop.color.a < 1.0)
        );
    }

    #[test]
    fn chrome_metrics_do_not_jump_at_breakpoints() {
        for width in [560.0, 849.9, 850.0, 850.1, 1019.9, 1020.0, 1279.0] {
            assert_eq!(edge(width), EDGE);
            assert_eq!(chrome_height(width), CHROME_HEIGHT);
            assert_eq!(content_top(width), 102.0);
        }
    }

    #[test]
    fn sidebar_width_changes_continuously() {
        for width in [720.0, 850.0, 900.0, 940.0, 1020.0, 1100.0] {
            let before = sidebar_width(width - 0.1);
            let after = sidebar_width(width + 0.1);

            assert!((after - before).abs() < 0.1, "{width}: {before} -> {after}");
        }
    }

    #[test]
    fn floating_panel_shares_the_capsule_edge_and_shadow() {
        let capsule = capsule(Palette::DARK);
        let panel = floating_panel(Palette::DARK);

        assert_eq!(panel.border.color, capsule.border.color);
        assert_eq!(panel.border.width, capsule.border.width);
        assert_eq!(panel.shadow.color, capsule.shadow.color);
        assert_eq!(panel.shadow.offset, capsule.shadow.offset);
        assert_eq!(panel.shadow.blur_radius, capsule.shadow.blur_radius);
    }
}
