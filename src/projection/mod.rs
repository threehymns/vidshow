//! The projector: the window or overlay that shows the picture on the display you chose.
//!
//! The control window has every control; this covers its display exactly and shows nothing but the
//! picture. It is a window wherever a window can be placed, and on Wayland, which gives clients no
//! say in where a window goes, a layer-shell overlay pinned to the chosen output. [`Projector`] is
//! the only code that knows which of the two it is, and the only owner of "the picture is up".

mod keys;
mod view;

pub use keys::projection_key;
pub use view::{projection_closed, projection_setup, projection_view};

use crate::keybinds::{Action, Keybinds};
use crate::player::{Player, Wakers};
use crate::settings::FitMode;
use rune::{AppHandle, Display, DisplayTarget, OverlayHandle, OverlayPlacement, WindowHandle};
use std::sync::Arc;

/// What the projector cannot do for itself, because the control window owns the clip, the settings
/// and the saved state.
#[derive(Clone, Copy, Debug)]
pub enum Event {
    /// A transport action was asked for in the projector's window.
    Action(Action),
    /// The projector closed itself.
    Closed,
}

/// The one wire between the projector and the control window, which the two are joined by where
/// they are built.
pub type Notify = Arc<dyn Fn(Event) + Send + Sync>;

/// The projector's window state, pushed by the control window on every change.
pub struct Projection {
    pub player: Option<Arc<Player>>,
    pub fit: FitMode,
    pub hide_cursor: bool,
    /// Reports back rather than reaching into the control window.
    pub notify: Notify,
    pub wakers: Arc<Wakers>,
    pub keybinds: Keybinds,
}

impl Projection {
    pub fn apply(&mut self, player: Option<Arc<Player>>, fit: FitMode, hide_cursor: bool) {
        self.player = player;
        self.fit = fit;
        self.hide_cursor = hide_cursor;
    }
}

/// A display's size in points: what window sizes are asked for in, and the size the projector
/// window follows when you present on a different display.
pub fn display_points(display: &Display) -> (f32, f32) {
    let scale = if display.scale > 0.0 {
        display.scale
    } else {
        1.0
    };
    (
        display.size.0 as f32 / scale as f32,
        display.size.1 as f32 / scale as f32,
    )
}

/// How rune names a display, for placing a surface on it.
pub fn display_target(display: &Display) -> DisplayTarget {
    match &display.id {
        Some(id) => DisplayTarget::Id(id.clone()),
        None => DisplayTarget::Primary,
    }
}

/// Where the picture goes. The two targets are the whole of vidshow's platform difference, and
/// `presenting` is the one fact behind it, so no caller has to keep its own copy.
pub struct Projector {
    target: Target,
    presenting: bool,
}

enum Target {
    Window {
        handle: WindowHandle,
        state: AppHandle<Projection>,
    },
    Overlay(OverlayHandle<Projection>),
}

impl Projector {
    pub fn window(handle: WindowHandle, state: AppHandle<Projection>) -> Self {
        Self {
            target: Target::Window { handle, state },
            presenting: false,
        }
    }

    pub fn overlay(handle: OverlayHandle<Projection>) -> Self {
        Self {
            target: Target::Overlay(handle),
            presenting: false,
        }
    }

    /// Whether the picture is up on the chosen display.
    pub fn is_presenting(&self) -> bool {
        self.presenting
    }

    /// Show the picture on `display`, covering it whole.
    pub(crate) fn show(&mut self, display: &Display) {
        match &self.target {
            Target::Window { handle, .. } => {
                // The size follows the display chosen while presenting; where `place` also sizes
                // windows it asks for the same rectangle. No exclusive full screen, so alt-tab,
                // notifications and screen sharing behave normally.
                handle.resize_instant(display_points(display));
                handle.place(display_target(display), OverlayPlacement::FullScreen);
                handle.open();
            }
            Target::Overlay(handle) => {
                handle.set_display(display_target(display));
                handle.show();
            }
        }
        self.presenting = true;
    }

    pub(crate) fn hide(&mut self) {
        match &self.target {
            Target::Window { handle, .. } => handle.hide(),
            Target::Overlay(handle) => handle.hide(),
        }
        self.presenting = false;
    }

    /// `On top` for the window. A layer-shell overlay already sits above ordinary windows.
    pub(crate) fn set_always_on_top(&self, on: bool) {
        if let Target::Window { handle, .. } = &self.target {
            handle.set_always_on_top(on);
        }
    }

    /// Push the current clip, fit and cursor policy to the projector.
    pub(crate) fn sync(&self, player: Option<Arc<Player>>, fit: FitMode, hide_cursor: bool) {
        match &self.target {
            Target::Window { state, .. } => {
                state.update(move |projection, _| projection.apply(player, fit, hide_cursor))
            }
            Target::Overlay(handle) => {
                handle.update(move |projection, _| projection.apply(player, fit, hide_cursor))
            }
        }
    }
}
