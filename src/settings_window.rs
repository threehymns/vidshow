//! The settings popup: the presentation options that do not earn a place on the control window.
//!
//! It is a rune-kit dialog mounted over the control window, so it reads that window's state and
//! applies every toggle to the running clip and projector straight away.

use crate::ui::Presenter;
use rune::{El, col};
use rune_kit::{Dialog, Kit, Switch};

/// Mount the settings dialog while it is open. The dialog brings its own scrim, focus and
/// Escape/click-outside dismissal, so the control window only has to open it.
pub fn settings_window(s: &Presenter, k: &Kit, layer: &mut Vec<El>) {
    if !s.settings_open {
        return;
    }
    Dialog::new("settings", "Settings")
        .open(true)
        .fit()
        .width(360.0)
        .on_close(|s: &mut Presenter| s.settings_open = false)
        .child(
            col()
                .key("settings-options")
                .w_full()
                .gap(12.0)
                .padding(16.0)
                .child(
                    Switch::new(s.cfg.always_on_top)
                        .label("On top")
                        .on_toggle(Presenter::toggle_on_top)
                        .build(k),
                )
                .child(
                    Switch::new(s.cfg.preview)
                        .label("Preview")
                        .on_toggle(Presenter::toggle_preview)
                        .build(k),
                )
                .child(
                    Switch::new(s.cfg.hide_cursor)
                        .label("Hide cursor")
                        .on_toggle(Presenter::toggle_cursor)
                        .build(k),
                ),
        )
        .build(k, layer);
}
