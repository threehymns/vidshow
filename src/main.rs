//! vidshow: present a video file on a second display.
//!
//! One window holds the controls; the projector window opens only when you press Present and
//! covers the display you chose, full screen and borderless. A file can also arrive from Explorer
//! through Open With, which passes its path as the first argument.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod audio;
mod media;
mod player;
mod settings;
mod ui;

use media::Tools;
use player::Wakers;
use rune::{App, AppSpec, AnyWindowSpec, Display, WindowOptions, WindowSpec};
use settings::Settings;
use std::path::PathBuf;
use ui::{Presenter, Projection};

/// Stand-in for `AppSpec::init`; `App::init` supplies the real state, which needs handles that
/// only exist once both windows do.
fn no_presenter() -> Presenter {
    unreachable!("the control window's state comes from App::init")
}

fn no_projection() -> Projection {
    unreachable!("the projector's state comes from WindowSpec::state")
}

fn main() {
    rune::set_app_id("vidshow");

    // Rune takes its own flags; what is left is the file Explorer (or a shell) passed us.
    let mut file: Option<PathBuf> = None;
    let mut present = false;
    let mut autoplay = true;
    for arg in rune::extra_args() {
        match arg.as_str() {
            "--present" => present = true,
            "--no-play" => autoplay = false,
            other if !other.starts_with('-') && file.is_none() => {
                let path = PathBuf::from(other);
                // A mistyped path should not look like a failure to open a real file.
                if path.is_file() {
                    file = Some(path);
                }
            }
            _ => {}
        }
    }

    rune::run_windows(move |displays| build(file.clone(), present, autoplay, displays));
}

fn build(file: Option<PathBuf>, present: bool, autoplay: bool, displays: &[Display]) -> Vec<AnyWindowSpec> {
    let cfg = Settings::load();
    let tools = Tools::discover();
    let wakers = Wakers::new();
    let displays: Vec<Display> = displays.to_vec();

    let control = App::new(AppSpec {
        min_size: Some((860.0, 620.0)),
        setup: Some(ui::presenter_setup),
        poll: Some(ui::presenter_poll),
        on_key: Some(ui::presenter_key),
        ..AppSpec::new("vidshow", (1120.0, 780.0), no_presenter, ui::presenter_view)
    });
    let control_handle = control.handle();

    let target = cfg.resolve(&displays).map(|index| displays[index].clone());
    let options = WindowOptions {
        // Created hidden and placed by Present, so nothing flashes on the wrong display.
        visible: false,
        borderless: true,
        always_on_top: cfg.always_on_top,
        resizable: false,
        min_size: Some((0.0, 0.0)),
        position: target.as_ref().map(|display| display.position),
        physical_size: target.as_ref().map(|display| display.size),
        ..Default::default()
    };

    let projection = App::new(AppSpec {
        min_size: Some((0.0, 0.0)),
        setup: Some(ui::projection_setup),
        on_key: Some(ui::projection_key),
        ..AppSpec::new("vidshow — presentation", (1280.0, 720.0), no_projection, ui::projection_view)
    })
    .on_close(ui::projection_closed);

    let mut control = control;
    let (projection_state, projection_window) = control.window(
        WindowSpec::from_app("projection", projection)
            .state(Projection {
                player: None,
                fit: cfg.fit,
                hide_cursor: cfg.hide_cursor,
                presenter: control_handle.clone(),
                wakers: wakers.clone(),
            })
            .options(options),
    );

    let control = control.init(move || Presenter {
        tools,
        player: None,
        path: None,
        error: None,
        cfg,
        displays,
        presenting: false,
        projection: projection_window,
        projection_state,
        wakers: wakers.clone(),
        pending: file,
        auto_present: present,
        autoplay,
        scrub: None,
    });

    vec![WindowSpec::from_app("control", control).into()]
}
