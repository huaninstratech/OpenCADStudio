//! How much of the desktop interface the web build shows.
//!
//! A screen the width of a phone cannot hold the ribbon, the docks and the
//! status bar, and a page that embeds the app to show one drawing wants
//! nothing around the drawing. The web build therefore has three interfaces,
//! and which one is in use is decided here — from the page's address and the
//! window's width — so that the view, the input handling and the tests agree.
//!
//! The decision and the address parsing are pure, and are compiled and tested
//! on every target; only reading the address and the window is wasm-only.

/// From this window width in logical pixels down, the web build uses the
/// phone interface. A phone held sideways is still under it; a tablet is not.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub const PHONE_MAX_WIDTH: f32 = 720.0;

/// The interface the web build shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chrome {
    /// Ribbon, docks, command line and status bar.
    Full,
    /// The drawing alone, for a page that embeds the app to show it.
    Viewer,
    /// The drawing over a row of large buttons, on a screen the width of a
    /// phone.
    Phone,
}

impl Chrome {
    /// Whether the interface is one of the two that drop the ribbon.
    pub fn is_compact(self) -> bool {
        self != Self::Full
    }
}

/// The interface for a window `width` logical pixels wide, on a page that
/// asked for the viewer when `viewer` is set.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn chrome(viewer: bool, width: f32) -> Chrome {
    if viewer {
        Chrome::Viewer
    } else if width < PHONE_MAX_WIDTH {
        Chrome::Phone
    } else {
        Chrome::Full
    }
}

/// Whether the page asks for the viewer interface. `ui=viewer` asks for it
/// and `ui=full` declines it; without either, an embedded page (`embed` set)
/// gets it, since a page that embeds the app shows a drawing, and `ui=full`
/// is for a host that wants the whole editor inside it.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
pub fn viewer_from_search(search: &str, embed: bool) -> bool {
    let asked = url::form_urlencoded::parse(search.trim_start_matches('?').as_bytes())
        .find(|(key, _)| key == "ui")
        .map(|(_, value)| value.into_owned());
    match asked.as_deref() {
        Some("viewer") => true,
        Some("full") => false,
        _ => embed,
    }
}

#[cfg(target_arch = "wasm32")]
mod web {
    use super::{chrome, viewer_from_search, Chrome};
    use std::sync::OnceLock;

    /// Read once: the address does not change while the page is open.
    static VIEWER: OnceLock<bool> = OnceLock::new();

    fn viewer() -> bool {
        *VIEWER.get_or_init(|| {
            let search = web_sys::window()
                .and_then(|window| window.location().search().ok())
                .unwrap_or_default();
            viewer_from_search(&search, crate::io::host_bridge::is_embed_mode())
        })
    }

    /// The interface for a window `width` logical pixels wide.
    pub fn current(width: f32) -> Chrome {
        chrome(viewer(), width)
    }

    /// The window's size in logical pixels, for the interface of the first
    /// frame: the window reports its size only once it has been laid out.
    pub fn window_size() -> Option<(f32, f32)> {
        let window = web_sys::window()?;
        let width = window.inner_width().ok()?.as_f64()? as f32;
        let height = window.inner_height().ok()?.as_f64()? as f32;
        Some((width, height))
    }
}

#[cfg(target_arch = "wasm32")]
pub use web::*;

/// The native build always shows the whole interface.
#[cfg(not(target_arch = "wasm32"))]
pub fn current(_width: f32) -> Chrome {
    #[cfg(test)]
    if let Some(chrome) = set_by_a_test(None) {
        return chrome;
    }
    Chrome::Full
}

// What a test on this thread put in place of the decision, if anything.
// Thread-local, so tests that show different interfaces run in parallel.
#[cfg(test)]
thread_local! {
    static SET_BY_A_TEST: std::cell::RefCell<Option<Chrome>> =
        const { std::cell::RefCell::new(None) };
}

/// Makes `current` return `chrome` on this thread for as long as the caller
/// wants: pass it again with `None` to give the decision back.
#[cfg(test)]
pub(crate) fn set_for_this_thread(chrome: Option<Chrome>) {
    SET_BY_A_TEST.with(|slot| *slot.borrow_mut() = chrome);
}

/// The interface a test set, after storing `chrome` when one is given.
#[cfg(test)]
fn set_by_a_test(chrome: Option<Chrome>) -> Option<Chrome> {
    let stored = SET_BY_A_TEST.with(|slot| *slot.borrow());
    if chrome.is_some() {
        set_for_this_thread(chrome);
    }
    stored
}

#[cfg(test)]
mod tests {
    use super::{chrome, set_for_this_thread, viewer_from_search, Chrome, PHONE_MAX_WIDTH};

    #[test]
    fn a_narrow_window_is_a_phone_and_a_wide_one_the_full_interface() {
        assert_eq!(chrome(false, PHONE_MAX_WIDTH), Chrome::Full);
        assert_eq!(chrome(false, PHONE_MAX_WIDTH - 1.0), Chrome::Phone);
        assert_eq!(chrome(false, 390.0), Chrome::Phone);
        assert_eq!(chrome(false, 1280.0), Chrome::Full);
    }

    #[test]
    fn the_viewer_comes_whatever_the_width() {
        assert_eq!(chrome(true, 390.0), Chrome::Viewer);
        assert_eq!(chrome(true, 1280.0), Chrome::Viewer);
    }

    #[test]
    fn only_the_two_compact_interfaces_drop_the_ribbon() {
        assert!(!Chrome::Full.is_compact());
        assert!(Chrome::Viewer.is_compact());
        assert!(Chrome::Phone.is_compact());
    }

    #[test]
    fn the_address_asks_for_the_viewer_and_an_embedded_page_gets_it_by_default() {
        assert!(viewer_from_search("?ui=viewer", false));
        assert!(viewer_from_search("?embed=1&ui=viewer", true));
        assert!(viewer_from_search("", true), "an embedded page is a viewer");
        assert!(
            !viewer_from_search("", false),
            "a page of its own is the editor"
        );
        assert!(
            !viewer_from_search("?embed=1&ui=full", true),
            "a host that wants the editor inside its page says so"
        );
        // Anything else named is not understood, and changes nothing.
        assert!(viewer_from_search("?embed=1&ui=compact", true));
        assert!(!viewer_from_search("?viewer=1", false));
    }

    #[test]
    fn a_test_can_show_an_interface_whatever_the_window() {
        set_for_this_thread(Some(Chrome::Phone));
        assert_eq!(super::current(1280.0), Chrome::Phone);
        set_for_this_thread(None);
        assert_eq!(super::current(390.0), super::Chrome::Full);
    }
}
