//! The projector: the window or overlay that shows the picture on the display you chose.
//!
//! The control window has every control; the projector covers its display exactly and shows
//! nothing but the picture — the same shape of thing as PowerPoint's presenter output or an OBS
//! projector, without exclusive full screen mode. It is a window wherever a window can be placed
//! (Windows, X11, macOS); on Wayland, which gives clients no say in where a window goes, it is a
//! layer-shell overlay pinned to the chosen output instead. [`Projector`] is the only code that
//! knows which of the two it is.

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

/// The projector's state.
pub struct Projection {
    pub player: Option<Arc<Player>>,
    pub fit: FitMode,
    pub hide_cursor: bool,
    /// Reports back rather than reaching into the control window.
    pub notify: Notify,
    pub wakers: Arc<Wakers>,
    /// The shortcut table this window answers to.
    pub keybinds: Keybinds,
}

impl Projection {
    /// Takes on the clip and presentation options the control window currently has.
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

/// Where the picture goes. The two variants are the whole of vidshow's platform difference.
pub enum Projector {
    Window {
        handle: WindowHandle,
        state: AppHandle<Projection>,
    },
    Overlay(OverlayHandle<Projection>),
}

impl Projector {
    pub fn window(handle: WindowHandle, state: AppHandle<Projection>) -> Self {
        Self::Window { handle, state }
    }

    pub fn overlay(handle: OverlayHandle<Projection>) -> Self {
        Self::Overlay(handle)
    }

    /// Show the picture on `display`, covering it whole.
    pub(crate) fn show(&self, display: &Display) {
        match self {
            Self::Window { handle, .. } => {
                // The size follows the display chosen while presenting; where `place` also sizes
                // windows it asks for the same rectangle. No exclusive full screen, so alt-tab,
                // notifications and screen sharing behave normally.
                handle.resize_instant(display_points(display));
                handle.place(display_target(display), OverlayPlacement::FullScreen);
                handle.open();
            }
            Self::Overlay(handle) => {
                handle.set_display(display_target(display));
                handle.show();
            }
        }
    }

    pub(crate) fn hide(&self) {
        match self {
            Self::Window { handle, .. } => handle.hide(),
            Self::Overlay(handle) => handle.hide(),
        }
    }

    /// `On top` for the window. A layer-shell overlay already sits above ordinary windows.
    pub(crate) fn set_always_on_top(&self, on: bool) {
        if let Self::Window { handle, .. } = self {
            handle.set_always_on_top(on);
        }
    }

    /// Push the current clip, fit and cursor policy to the projector.
    pub(crate) fn sync(&self, player: Option<Arc<Player>>, fit: FitMode, hide_cursor: bool) {
        match self {
            Self::Window { state, .. } => {
                state.update(move |projection, _| projection.apply(player, fit, hide_cursor))
            }
            Self::Overlay(handle) => {
                handle.update(move |projection, _| projection.apply(player, fit, hide_cursor))
            }
        }
    }
}
