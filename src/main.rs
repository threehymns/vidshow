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
use rune::{
    App, AppSpec, AnyWindowSpec, Display, DisplayTarget, KeyPolicy, OverlayInput, OverlayOptions,
    OverlayPlacement, OverlaySpec, WindowOptions, WindowSpec,
};
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

    let mut control = App::new(AppSpec {
        min_size: Some((860.0, 620.0)),
        setup: Some(ui::presenter_setup),
        poll: Some(ui::presenter_poll),
        on_key: Some(ui::presenter_key),
        ..AppSpec::new("vidshow", (1120.0, 780.0), no_presenter, ui::presenter_view)
    });
    let control_handle = control.handle();

    let target = cfg.resolve(&displays).map(|index| displays[index].clone());
    // Points, not physical pixels: where `place` works it sets the exact physical rectangle anyway.
    let size = target.as_ref().map(ui::display_points).unwrap_or((1280.0, 720.0));
    let initial = target.as_ref().map(ui::display_target).unwrap_or(DisplayTarget::Primary);

    let state = Projection {
        player: None,
        fit: cfg.fit,
        hide_cursor: cfg.hide_cursor,
        presenter: control_handle.clone(),
        wakers: wakers.clone(),
    };

    // Wayland gives a client no say in where a window opens, so there the picture is a layer-shell
    // overlay pinned to the chosen output. Everywhere else it is an ordinary window, placed.
    let projection = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        ui::Projector::overlay(
            control.overlay(
                OverlaySpec::new("vidshow — presentation", size, ui::projection_view)
                    .options(OverlayOptions {
                        placement: Some(OverlayPlacement::FullScreen),
                        display: initial,
                        // The picture takes no clicks or keys; the control window keeps both.
                        input: OverlayInput::None,
                        keys: KeyPolicy::None,
                        ..Default::default()
                    })
                    .setup(ui::projection_setup),
                state,
            ),
        )
    } else {
        let options = WindowOptions {
            // Created hidden and placed by Present, so nothing flashes on the wrong display.
            visible: false,
            borderless: true,
            always_on_top: cfg.always_on_top,
            resizable: false,
            min_size: Some((0.0, 0.0)),
            position: target.as_ref().map(|display| display.position),
            ..Default::default()
        };
        let app = App::new(AppSpec {
            min_size: Some((0.0, 0.0)),
            setup: Some(ui::projection_setup),
            on_key: Some(ui::projection_key),
            ..AppSpec::new("vidshow — presentation", size, no_projection, ui::projection_view)
        })
        .on_close(ui::projection_closed)
        // Maps at its final size, so a tiling window manager floats it instead of tiling it.
        .float_on_open();
        let (state, handle) = control
            .window(WindowSpec::from_app("projection", app).state(state).options(options));
        ui::Projector::window(handle, state)
    };

    let control = control.init(move || Presenter {
        tools,
        player: None,
        path: None,
        error: None,
        cfg,
        displays,
        presenting: false,
        projection,
        wakers: wakers.clone(),
        pending: file,
        auto_present: present,
        autoplay,
        scrub: None,
    });

    vec![WindowSpec::from_app("control", control).into()]
}
