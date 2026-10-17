//! A pointer-enter / leave area that keeps counting the pointer as inside
//! while it rests on an overlay (a dropdown, a combo box list) drawn over
//! its bounds.
//!
//! `mouse_area` reads such a pointer as gone: iced hands the layer below an
//! overlay that wants the pointer an unavailable cursor, with no position. An
//! auto-hiding dock edge then hid as soon as one of its own dropdowns opened
//! under the pointer, taking the dropdown with it. Here an unavailable cursor
//! keeps the last state, so only a pointer seen outside the bounds, or one
//! that leaves the window, exits the area.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{self, tree, Tree, Widget};
use iced::advanced::{mouse, overlay, renderer, Shell};
use iced::{Element, Event, Length, Rectangle, Renderer, Size, Theme, Vector};

use crate::app::Message;

/// Wrap `content`, publishing `on_enter` / `on_exit` as the pointer enters
/// and leaves its bounds, overlays drawn over them included.
pub fn hover_zone<'a>(
    content: impl Into<Element<'a, Message>>,
    on_enter: Message,
    on_exit: Message,
) -> Element<'a, Message> {
    Element::new(HoverZone {
        content: content.into(),
        on_enter,
        on_exit,
    })
}

struct HoverZone<'a> {
    content: Element<'a, Message>,
    on_enter: Message,
    on_exit: Message,
}

#[derive(Default)]
struct State {
    hovered: bool,
}

impl Widget<Message, Theme, Renderer> for HoverZone<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State::default())
    }

    fn diff(&mut self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_mut(&mut self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn widget::Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );
        // Unlike `mouse_area`, this runs for captured events too, and a
        // cursor without a position (on an overlay) changes nothing.
        let hovered = match (event, cursor.land()) {
            (Event::Mouse(mouse::Event::CursorLeft), _) => false,
            (_, mouse::Cursor::Available(p)) => layout.bounds().contains(p),
            _ => return,
        };
        let state = tree.state.downcast_mut::<State>();
        if hovered != state.hovered {
            state.hovered = hovered;
            shell.publish(if hovered {
                self.on_enter.clone()
            } else {
                self.on_exit.clone()
            });
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, Theme, Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}
