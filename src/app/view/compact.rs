//! The interfaces that drop the ribbon. Which one is in use is
//! `io::web_chrome`'s decision; this is what each puts where the ribbon,
//! the docks and the status bar are in the whole interface: on a phone, a
//! row of buttons a finger can hit; in a viewer, nothing.

use super::{Message, OpenCADStudio};
use crate::io::web_chrome::Chrome;
use crate::ui::icons;
use crate::ui::kit::flat_button_style;
use crate::ui::style::{chrome, kit_metrics as km};
use iced::widget::{button, column, container, row, Space};
use iced::{Background, Border, Element, Length, Theme};

static OPEN: &[u8] = include_bytes!("../../../assets/icons/ui/folder_open.svg");
static UNDO: &[u8] = include_bytes!("../../../assets/icons/ui/undo.svg");
static REDO: &[u8] = include_bytes!("../../../assets/icons/ui/redo.svg");
static VIEWS: &[u8] = include_bytes!("../../../assets/icons/viewcube.svg");
static ZOOM_OUT: &[u8] = include_bytes!("../../../assets/icons/zoom_out.svg");
static ZOOM_IN: &[u8] = include_bytes!("../../../assets/icons/zoom_in.svg");
static ZOOM_EXTENTS: &[u8] = include_bytes!("../../../assets/icons/zoom_ext.svg");
static VIEW_TOP: &[u8] = include_bytes!("../../../assets/icons/view_top.svg");
static VIEW_FRONT: &[u8] = include_bytes!("../../../assets/icons/view_front.svg");
static VIEW_RIGHT: &[u8] = include_bytes!("../../../assets/icons/view_right.svg");
static VIEW_ISO: &[u8] = include_bytes!("../../../assets/icons/view_iso.svg");

/// The side of one of the phone's buttons: the smallest target a finger
/// hits reliably, and more than twice the toolbar's.
pub(super) const BUTTON: f32 = 44.0;
/// An icon in one of them.
const ICON: f32 = 20.0;
/// Space either side of a row of buttons, and between two of them. Seven
/// buttons and their gaps are 352 px, so the row fits a 360 px screen.
const PAD: f32 = 4.0;
const GAP: f32 = 6.0;

impl OpenCADStudio {
    /// The interface in use (see `io::web_chrome`).
    pub(in crate::app) fn chrome(&self) -> Chrome {
        crate::io::web_chrome::current(self.win_size.0)
    }

    /// What stands where the status bar does under a compact interface: the
    /// phone's buttons, and nothing in the viewer.
    pub(super) fn compact_bar(&self, interface: Chrome) -> Element<'_, Message> {
        if interface != Chrome::Phone {
            return Space::new().into();
        }
        let tab = &self.tabs[self.active_tab];
        // There is nothing to undo, zoom or turn on the Start page.
        let drawing = !tab.is_start;
        let command = |name: &str| drawing.then(|| Message::Command(name.to_string()));
        let mut bar = column![];
        if self.phone_views_open {
            bar = bar.push(
                row![
                    touch_button(VIEW_TOP, command("VIEW TOP")),
                    touch_button(VIEW_FRONT, command("VIEW FRONT")),
                    touch_button(VIEW_RIGHT, command("VIEW RIGHT")),
                    touch_button(VIEW_ISO, command("VIEW ISO")),
                ]
                .spacing(GAP)
                .padding([0.0, PAD]),
            );
        }
        bar = bar.push(
            row![
                touch_button(OPEN, Some(Message::OpenFile)),
                touch_button(
                    UNDO,
                    drawing
                        .then_some(Message::Undo)
                        .filter(|_| !tab.history.undo_stack.is_empty()),
                ),
                touch_button(
                    REDO,
                    drawing
                        .then_some(Message::Redo)
                        .filter(|_| !tab.history.redo_stack.is_empty()),
                ),
                touch_button(ZOOM_OUT, command("ZOOM OUT")),
                touch_button(ZOOM_IN, command("ZOOM IN")),
                touch_button(ZOOM_EXTENTS, command("ZOOM EXTENTS")),
                touch_button(VIEWS, Some(Message::TogglePhoneViews)),
            ]
            .spacing(GAP)
            .padding([0.0, PAD]),
        );
        container(bar).width(Length::Fill).style(bar_style).into()
    }
}

/// A square button a finger can hit, `art` centred in it; without a
/// `message` it is shown disabled.
fn touch_button(art: &'static [u8], message: Option<Message>) -> Element<'static, Message> {
    let icon = if message.is_some() {
        icons::tinted(art, ICON, chrome::text)
    } else {
        icons::tinted_faded(art, ICON, 0.4, chrome::text)
    };
    button(container(icon).center(Length::Fill))
        .on_press_maybe(message)
        .width(Length::Fixed(BUTTON))
        .height(Length::Fixed(BUTTON))
        .padding(0)
        .style(flat_button_style(false))
        .into()
}

/// The bar under the buttons: the colour of the ribbon's, with a line
/// against the drawing.
fn bar_style(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(Background::Color(chrome::bar(theme))),
        border: Border {
            color: chrome::border(theme),
            width: km::line(),
            radius: 0.0.into(),
        },
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::{BUTTON, GAP, PAD};
    use crate::app::{Message, OpenCADStudio};
    use crate::io::web_chrome::{set_for_this_thread, Chrome};
    use iced::mouse::{self, Button};
    use iced::{Event, Point, Settings, Size, Theme};
    use iced_test::Simulator;

    /// A phone held upright.
    const PHONE: Size = Size::new(390.0, 844.0);

    fn with_ui_test(test: impl FnOnce() + Send + 'static) {
        let _guard = crate::headless_ui_lock();
        std::thread::Builder::new()
            .stack_size(32 * 1024 * 1024)
            .spawn(test)
            .expect("spawn UI test")
            .join()
            .unwrap_or_else(|error| std::panic::resume_unwind(error));
    }

    /// The centre of the button at `index` of a row whose buttons end
    /// `rows` rows above the bottom of the screen.
    fn button_centre(index: usize, rows: usize) -> Point {
        Point::new(
            PAD + BUTTON / 2.0 + index as f32 * (BUTTON + GAP),
            PHONE.height - PAD - BUTTON / 2.0 - rows as f32 * (BUTTON + PAD),
        )
    }

    /// Clicks at `at` in a view built at the size of a phone, and gives the
    /// one message it published; `what` names the click in a failure.
    fn tap(app: &mut OpenCADStudio, at: Point, what: &str) -> Message {
        let messages = click(app, at);
        assert_eq!(messages.len(), 1, "{what}: one message, not {messages:?}");
        messages.into_iter().next().unwrap()
    }

    fn click(app: &mut OpenCADStudio, at: Point) -> Vec<Message> {
        let mut ui = Simulator::with_size(Settings::default(), PHONE, app.view_main());
        ui.point_at(at);
        ui.simulate([
            Event::Mouse(mouse::Event::ButtonPressed(Button::Left)),
            Event::Mouse(mouse::Event::ButtonReleased(Button::Left)),
        ]);
        ui.into_messages().collect()
    }

    /// The message a tool button of the ribbon sends for a command.
    fn command(name: &str) -> Message {
        Message::Command(name.to_string())
    }

    /// Draws the interface to `<OCS_COMPACT_CAPTURES>/<name>.png` when that
    /// variable names a folder: evidence for reviewing the layout.
    fn captures(ui: &mut Simulator<'_, Message>, name: &str) {
        let Some(directory) = std::env::var_os("OCS_COMPACT_CAPTURES") else {
            return;
        };
        let directory = std::path::PathBuf::from(directory);
        // The library writes an image it does not find and compares one it
        // does; a review wants this run's, so the last one goes first.
        for previous in captures_of(&directory, name) {
            let _ = std::fs::remove_file(previous);
        }
        assert!(ui
            .snapshot(&Theme::Dark)
            .expect("render the interface")
            .matches_image(directory.join(format!("{name}.png")))
            .expect("write the capture"));
        assert!(
            !captures_of(&directory, name).is_empty(),
            "the capture {name} was written"
        );
    }

    /// The captures of `name` in `directory`. The library ends each with the
    /// name of the renderer that drew it.
    fn captures_of(directory: &std::path::Path, name: &str) -> Vec<std::path::PathBuf> {
        let starts_with = format!("{name}-");
        std::fs::read_dir(directory)
            .map(|entries| {
                entries
                    .flatten()
                    .map(|entry| entry.path())
                    .filter(|path| {
                        path.file_name().is_some_and(|file_name| {
                            let file_name = file_name.to_string_lossy();
                            file_name.starts_with(&starts_with) && file_name.ends_with(".png")
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether the view offers a text starting with `label`.
    fn offers(app: &OpenCADStudio, size: Size, label: &str) -> bool {
        let mut ui = Simulator::with_size(Settings::default(), size, app.view_main());
        ui.find(label).is_ok()
    }

    /// The app on a new drawing: the hook that pushes one leaves the Start
    /// tab showing, so the drawing's tab is made the active one.
    fn open_a_drawing(app: &mut OpenCADStudio) {
        let _ = app.push_test_document();
        app.active_tab = app.tabs.len() - 1;
    }

    fn phone_app() -> OpenCADStudio {
        set_for_this_thread(Some(Chrome::Phone));
        let mut app = OpenCADStudio::new_for_test();
        open_a_drawing(&mut app);
        app
    }

    #[test]
    fn the_phone_buttons_line_the_bottom_edge_and_do_what_they_say() {
        with_ui_test(|| {
            let mut app = phone_app();
            {
                let mut ui = Simulator::with_size(Settings::default(), PHONE, app.view_main());
                captures(&mut ui, "phone-drawing");
            }
            assert!(matches!(
                tap(&mut app, button_centre(0, 0), "Open"),
                Message::OpenFile
            ));
            // Nothing to undo or redo yet, either side of Zoom.
            assert!(click(&mut app, button_centre(1, 0)).is_empty(), "Undo");
            assert!(click(&mut app, button_centre(2, 0)).is_empty(), "Redo");
            assert!(
                matches!(tap(&mut app, button_centre(3, 0), "Zoom out"), Message::Command(name) if name == "ZOOM OUT")
            );
            assert!(
                matches!(tap(&mut app, button_centre(5, 0), "Zoom extents"), Message::Command(name) if name == "ZOOM EXTENTS")
            );

            // The last button shows the views, a row above the first.
            assert!(matches!(
                tap(&mut app, button_centre(6, 0), "Views"),
                Message::TogglePhoneViews
            ));
            let _ = app.update(Message::TogglePhoneViews);
            {
                let mut ui = Simulator::with_size(Settings::default(), PHONE, app.view_main());
                captures(&mut ui, "phone-views");
            }
            assert!(
                matches!(tap(&mut app, button_centre(0, 1), "View top"), Message::Command(name) if name == "VIEW TOP")
            );
            assert!(
                matches!(tap(&mut app, button_centre(3, 1), "View iso"), Message::Command(name) if name == "VIEW ISO")
            );
            // The first row is still where it was.
            assert!(matches!(
                tap(&mut app, button_centre(0, 0), "Open again"),
                Message::OpenFile
            ));
            set_for_this_thread(None);
        });
    }

    #[test]
    fn the_viewer_shows_the_drawing_and_nothing_around_it() {
        with_ui_test(|| {
            set_for_this_thread(Some(Chrome::Viewer));
            let mut app = OpenCADStudio::new_for_test();
            // No Start page in a viewer: it waits for its page's drawing.
            assert!(!offers(&app, Size::new(1280.0, 720.0), "New Drawing"));
            open_a_drawing(&mut app);
            {
                let mut ui = Simulator::with_size(Settings::default(), PHONE, app.view_main());
                captures(&mut ui, "viewer-drawing");
            }
            // No bar under the drawing: where the phone keeps its first
            // button, a click is a click in the drawing.
            let bottom_corner = click(&mut app, button_centre(0, 0));
            assert!(bottom_corner
                .iter()
                .all(|message| !matches!(message, Message::OpenFile | Message::TogglePhoneViews)));

            set_for_this_thread(Some(Chrome::Full));
            assert!(offers(&app, Size::new(1280.0, 720.0), "Model"));
            set_for_this_thread(None);
        });
    }

    #[test]
    fn the_whole_interface_stays_on_a_wide_window_and_off_the_native_build() {
        with_ui_test(|| {
            set_for_this_thread(None);
            let app = OpenCADStudio::new_for_test();
            assert_eq!(app.chrome(), Chrome::Full);
            assert!(offers(&app, Size::new(1280.0, 720.0), "New Drawing"));

            set_for_this_thread(Some(Chrome::Phone));
            let app = OpenCADStudio::new_for_test();
            assert_eq!(app.chrome(), Chrome::Phone);
            // The Start page still shows on the phone, over the buttons.
            assert!(offers(&app, PHONE, "New Drawing"));
            {
                let mut ui = Simulator::with_size(Settings::default(), PHONE, app.view_main());
                captures(&mut ui, "phone-start");
            }
            set_for_this_thread(None);
        });
    }

    #[test]
    fn the_row_of_seven_fits_the_narrowest_phone() {
        let width = 2.0 * PAD + 7.0 * BUTTON + 6.0 * GAP;
        assert!(width <= 360.0, "the row is {width} px wide");
    }
}
