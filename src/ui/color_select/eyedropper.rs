//! Pick a colour from anywhere inside the application window.
//!
//! While active, a transparent layer covers the window with a crosshair. The
//! first mouse move after the panel is hidden asks iced for a screenshot of
//! the window; a magnifier beside the cursor shows the pixels around it from
//! that screenshot, and a click picks the centre pixel.

use super::editor::rgb_color;
use super::{math, ColorPickerMsg};
use crate::app::Message;
use iced::widget::{column, container, mouse_area, pin, row, stack, text, Space};
use iced::window::Screenshot;
use iced::{Border, Color, Element, Length, Point, Theme};

/// Pixels shown across the magnifier (odd, so one sits in the centre).
const GRID: i32 = 11;
const CELL: f32 = 10.0;
/// Gap between the cursor and the magnifier.
const OFFSET: f32 = 20.0;

#[derive(Debug, Clone)]
pub enum EyedropperMsg {
    Start,
    Move(Point),
    Shot(Screenshot),
    Pick,
}

#[derive(Default)]
pub struct Eyedropper {
    shot: Option<Screenshot>,
    requested: bool,
    cursor: Option<Point>,
}

impl Eyedropper {
    /// Track the cursor. Returns true when the window screenshot should be
    /// requested now — the panel is gone from the screen by then.
    pub fn track(&mut self, cursor: Point) -> bool {
        self.cursor = Some(cursor);
        let request = self.shot.is_none() && !self.requested;
        self.requested = true;
        request
    }

    pub fn set_shot(&mut self, shot: Screenshot) {
        self.shot = Some(shot);
    }

    /// The pixel under the cursor, once the screenshot has arrived.
    pub fn picked(&self) -> Option<(u8, u8, u8)> {
        sample(self.shot.as_ref()?, self.cursor?, 0, 0)
    }

    pub fn view(&self) -> Element<'_, Message> {
        let eye = |m| Message::ColorPicker(ColorPickerMsg::Eyedropper(m));
        let catcher = mouse_area(Space::new().width(Length::Fill).height(Length::Fill))
            .on_move(move |p| eye(EyedropperMsg::Move(p)))
            .on_press(eye(EyedropperMsg::Pick))
            .interaction(iced::mouse::Interaction::Crosshair);

        let (Some(shot), Some(cursor)) = (self.shot.as_ref(), self.cursor) else {
            return catcher.into();
        };

        let half = GRID / 2;
        let mut grid = column![];
        for dy in -half..=half {
            let mut line = row![];
            for dx in -half..=half {
                let fill = sample(shot, cursor, dx, dy).map_or(Color::TRANSPARENT, rgb_color);
                let centre = dx == 0 && dy == 0;
                line = line.push(container(Space::new()).width(CELL).height(CELL).style(
                    move |_theme: &Theme| container::Style {
                        background: Some(fill.into()),
                        border: Border {
                            color: if centre {
                                Color::WHITE
                            } else {
                                Color::TRANSPARENT
                            },
                            width: if centre { 1.5 } else { 0.0 },
                            radius: 0.0.into(),
                        },
                        ..Default::default()
                    },
                ));
            }
            grid = grid.push(line);
        }
        let (r, g, b) = self.picked().unwrap_or((0, 0, 0));
        let loupe = container(
            column![
                grid,
                text(math::hex_string(r, g, b))
                    .size(11)
                    .font(iced::Font::MONOSPACE),
            ]
            .spacing(4)
            .align_x(iced::Center),
        )
        .padding(4)
        .style(|theme: &Theme| {
            let palette = theme.palette();
            container::Style {
                background: Some(palette.background.base.color.into()),
                text_color: Some(palette.background.base.text),
                border: Border {
                    color: palette.background.strong.color,
                    width: 1.0,
                    radius: 6.0.into(),
                },
                ..Default::default()
            }
        });

        // Keep the magnifier on screen: flip it to the other side of the
        // cursor near the right and bottom edges.
        let size = GRID as f32 * CELL + 8.0;
        let window_w = shot.size.width as f32 / shot.scale_factor;
        let window_h = shot.size.height as f32 / shot.scale_factor;
        // In a very small window neither side fits; stay inside the top-left.
        let x = if cursor.x + OFFSET + size > window_w {
            cursor.x - OFFSET - size
        } else {
            cursor.x + OFFSET
        }
        .max(0.0);
        let y = if cursor.y + OFFSET + size + 20.0 > window_h {
            cursor.y - OFFSET - size - 20.0
        } else {
            cursor.y + OFFSET
        }
        .max(0.0);
        stack![catcher, pin(loupe).x(x).y(y)].into()
    }
}

/// The screenshot pixel `dx`, `dy` physical pixels away from a logical point.
fn sample(shot: &Screenshot, at: Point, dx: i32, dy: i32) -> Option<(u8, u8, u8)> {
    let x = (at.x * shot.scale_factor) as i64 + i64::from(dx);
    let y = (at.y * shot.scale_factor) as i64 + i64::from(dy);
    let (w, h) = (i64::from(shot.size.width), i64::from(shot.size.height));
    if !(0..w).contains(&x) || !(0..h).contains(&y) {
        return None;
    }
    let i = ((y * w + x) * 4) as usize;
    let px = shot.rgba.get(i..i + 3)?;
    Some((px[0], px[1], px[2]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot() -> Screenshot {
        // 4×2 physical pixels at scale 2: pixel (x, y) has red = x, green = y.
        let mut rgba = Vec::new();
        for y in 0..2u8 {
            for x in 0..4u8 {
                rgba.extend([x, y, 9, 255]);
            }
        }
        Screenshot::new(rgba, iced::Size::new(4, 2), 2.0)
    }

    #[test]
    fn samples_physical_pixels_from_logical_points() {
        let s = shot();
        assert_eq!(sample(&s, Point::new(1.0, 0.5), 0, 0), Some((2, 1, 9)));
        assert_eq!(sample(&s, Point::new(1.0, 0.5), 1, -1), Some((3, 0, 9)));
        assert_eq!(sample(&s, Point::new(1.9, 0.0), 1, 0), None);
    }

    #[test]
    fn asks_for_one_screenshot_and_picks_after_it_arrives() {
        let mut e = Eyedropper::default();
        assert!(e.track(Point::new(0.0, 0.0)));
        assert!(!e.track(Point::new(1.0, 0.0)));
        assert_eq!(e.picked(), None);
        e.set_shot(shot());
        assert_eq!(e.picked(), Some((2, 0, 9)));
    }
}
