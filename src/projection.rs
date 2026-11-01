//! The projector: the window that shows the picture on the display you chose.
//!
//! The control window has every control; the projector covers its display exactly and shows
//! nothing but the picture — the same shape of thing as PowerPoint's presenter output or an OBS
//! projector, without exclusive full screen mode. It is a window wherever a window can be placed
//! (Windows, X11, macOS); on Wayland, which gives clients no say in where a window goes, it is a
//! layer-shell overlay pinned to the chosen output instead. That difference lives here and
//! nowhere else: [`Projector`] is the only code that knows which of the two it is.

use crate::keybinds::Keybinds;
use crate::player::{Player, Wakers};
use crate::settings::FitMode;
use rune::{
    AppHandle, Color, Cursor, Cx, Display, DisplayTarget, El, OverlayHandle, OverlayPlacement, Tx,
    WindowHandle, el,
};
use std::sync::Arc;

/// Called when the projector closes itself, so whoever opened it can stop claiming to present.
/// The projector holds no handle to the control window; the two are wired together only where
/// they are built.
pub type Closed = Arc<dyn Fn() + Send + Sync>;

/// The projector's state.
pub struct Projection {
    pub player: Option<Arc<Player>>,
    pub fit: FitMode,
    pub hide_cursor: bool,
    pub on_closed: Closed,
    pub wakers: Arc<Wakers>,
    /// The shortcut table this window answers to.
    pub keybinds: Keybinds,
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

/// Where the picture goes. A window can be placed on a display by every platform vidshow runs on
/// except Wayland, which leaves the output to the compositor; there the picture is a layer-shell
/// overlay, the one surface a client can pin to a chosen output.
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
                // windows it asks for the same rectangle.
                handle.resize_instant(display_points(display));
                // A borderless window covering the display, the way PowerPoint's presenter output
                // and an OBS projector do it: no exclusive full screen, so other windows can still
                // come up.
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
            Self::Window { state, .. } => state.update(move |projection, _| {
                projection.player = player;
                projection.fit = fit;
                projection.hide_cursor = hide_cursor;
            }),
            Self::Overlay(handle) => handle.update(move |projection, _| {
                projection.player = player;
                projection.fit = fit;
                projection.hide_cursor = hide_cursor;
            }),
        }
    }
}

pub fn projection_view(p: &Projection, _cx: &mut Cx<Projection>) -> El {
    let mut root = el().key("projector").w_full().h_full().fill(Color::BLACK);
    if let Some(player) = &p.player {
        root = root.child(
            el().key("frame")
                .w_full()
                .h_full()
                .native_surface(player.surface(), p.fit.surface())
                .cursor(if p.hide_cursor {
                    Cursor::Hidden
                } else {
                    Cursor::Default
                }),
        );
    }
    root
}

pub fn projection_closed(p: &mut Projection, _tx: &mut Tx) {
    if let Some(player) = &p.player {
        player.set_presenting(false);
    }
    (p.on_closed)();
    p.wakers.wake();
}

pub fn projection_setup(p: &mut Projection, waker: rune::Waker) {
    p.wakers.add(waker);
}
