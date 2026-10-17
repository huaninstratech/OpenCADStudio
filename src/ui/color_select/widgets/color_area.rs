//! Saturation/brightness field for one hue.
//!
//! Drawn as three layered quads instead of pixel by pixel: the solid hue, a
//! white-to-transparent wash from left to right, and a transparent-to-black
//! wash from top to bottom. Saturation runs left to right, brightness bottom
//! to top. Dragging anywhere moves the marker.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, tree, Widget};
use iced::advanced::{mouse, renderer, Shell};
use iced::gradient::Linear;
use iced::{
    Background, Border, Color, Element, Event, Gradient, Length, Point, Radians, Rectangle,
    Renderer, Size, Theme,
};

const RADIUS: f32 = 5.0;
const MARKER: f32 = 16.0;

pub struct ColorArea<'a, Message> {
    /// Fully saturated, full-brightness colour of the current hue.
    hue_color: Color,
    /// The colour under the marker.
    marker_color: Color,
    saturation: f32,
    brightness: f32,
    on_change: Box<dyn Fn(f32, f32) -> Message + 'a>,
    width: Length,
    height: Length,
}

#[derive(Default)]
struct State {
    dragging: bool,
}

impl<'a, Message> ColorArea<'a, Message> {
    /// `saturation` and `brightness` are 0–1; `on_change` receives the same.
    pub fn new(
        hue_color: Color,
        marker_color: Color,
        saturation: f32,
        brightness: f32,
        on_change: impl Fn(f32, f32) -> Message + 'a,
    ) -> Self {
        Self {
            hue_color,
            marker_color,
            saturation: saturation.clamp(0.0, 1.0),
            brightness: brightness.clamp(0.0, 1.0),
            on_change: Box::new(on_change),
            width: Length::Fill,
            height: Length::Fixed(150.0),
        }
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    fn values_at(bounds: Rectangle, cursor: Point) -> (f32, f32) {
        let s = ((cursor.x - bounds.x) / bounds.width.max(1.0)).clamp(0.0, 1.0);
        let b = 1.0 - ((cursor.y - bounds.y) / bounds.height.max(1.0)).clamp(0.0, 1.0);
        (s, b)
    }
}

impl<Message> Widget<Message, Theme, Renderer> for ColorArea<'_, Message> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn size(&self) -> Size<Length> {
        Size::new(self.width, self.height)
    }

    fn layout(
        &mut self,
        _tree: &mut widget::Tree,
        _renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        layout::atomic(limits, self.width, self.height)
    }

    fn update(
        &mut self,
        tree: &mut widget::Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        _viewport: &Rectangle,
    ) {
        let state = tree.state.downcast_mut::<State>();
        let bounds = layout.bounds();
        let publish = |shell: &mut Shell<'_, Message>, position: Point| {
            let (s, b) = Self::values_at(bounds, position);
            shell.publish((self.on_change)(s, b));
            shell.capture_event();
        };
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(position) = cursor.position_over(bounds) {
                    state.dragging = true;
                    publish(shell, position);
                }
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) if state.dragging => {
                if let Some(position) = cursor.position() {
                    publish(shell, position);
                }
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
                state.dragging = false;
                shell.capture_event();
            }
            _ => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &widget::Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        _viewport: &Rectangle,
        _renderer: &Renderer,
    ) -> mouse::Interaction {
        if tree.state.downcast_ref::<State>().dragging || cursor.is_over(layout.bounds()) {
            mouse::Interaction::Crosshair
        } else {
            mouse::Interaction::default()
        }
    }

    fn draw(
        &self,
        _tree: &widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &renderer::Style,
        layout: Layout<'_>,
        _cursor: mouse::Cursor,
        _viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;

        let bounds = layout.bounds();
        let rounded = renderer::Quad {
            bounds,
            border: Border {
                radius: RADIUS.into(),
                ..Border::default()
            },
            ..renderer::Quad::default()
        };
        let wash = |angle: f32, from: Color, to: Color| {
            Background::Gradient(Gradient::Linear(
                Linear::new(Radians(angle))
                    .add_stop(0.0, from)
                    .add_stop(1.0, to),
            ))
        };

        renderer.fill_quad(rounded, self.hue_color);
        renderer.fill_quad(
            rounded,
            wash(
                std::f32::consts::FRAC_PI_2,
                Color::WHITE,
                Color {
                    a: 0.0,
                    ..Color::WHITE
                },
            ),
        );
        renderer.fill_quad(
            renderer::Quad {
                border: Border {
                    color: Color::from_rgba(0.0, 0.0, 0.0, 0.5),
                    width: 1.0,
                    radius: RADIUS.into(),
                },
                ..rounded
            },
            wash(
                std::f32::consts::PI,
                Color {
                    a: 0.0,
                    ..Color::BLACK
                },
                Color::BLACK,
            ),
        );

        let centre = Point::new(
            bounds.x + bounds.width * self.saturation,
            bounds.y + bounds.height * (1.0 - self.brightness),
        );
        let marker = |size: f32| Rectangle {
            x: centre.x - size / 2.0,
            y: centre.y - size / 2.0,
            width: size,
            height: size,
        };
        renderer.fill_quad(
            renderer::Quad {
                bounds: marker(MARKER + 2.0),
                border: Border {
                    radius: ((MARKER + 2.0) / 2.0).into(),
                    ..Border::default()
                },
                ..renderer::Quad::default()
            },
            Color::from_rgba(0.0, 0.0, 0.0, 0.55),
        );
        renderer.fill_quad(
            renderer::Quad {
                bounds: marker(MARKER),
                border: Border {
                    color: Color::WHITE,
                    width: 2.0,
                    radius: (MARKER / 2.0).into(),
                },
                ..renderer::Quad::default()
            },
            self.marker_color,
        );
    }
}

impl<'a, Message: 'a> From<ColorArea<'a, Message>> for Element<'a, Message> {
    fn from(area: ColorArea<'a, Message>) -> Self {
        Element::new(area)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_map_to_saturation_and_brightness() {
        let bounds = Rectangle::new(Point::new(10.0, 20.0), Size::new(200.0, 100.0));
        let at = |x, y| ColorArea::<()>::values_at(bounds, Point::new(x, y));
        assert_eq!(at(10.0, 20.0), (0.0, 1.0));
        assert_eq!(at(210.0, 120.0), (1.0, 0.0));
        assert_eq!(at(110.0, 70.0), (0.5, 0.5));
        assert_eq!(at(-5.0, 500.0), (0.0, 0.0));
    }
}
