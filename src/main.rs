//! vidshow: present a video file on a second display.
//!
//! One window holds the controls; the projector window opens only when you press Present and
//! covers the display you chose, full screen and borderless. A file can also arrive from Explorer
//! through Open With, which passes its path as the first argument.

#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod audio;
mod control;
mod keybinds;
mod media;
mod player;
mod projection;
mod settings;

use control::{Clip, Overlay, Presenter, Startup};
use keybinds::Keybinds;
use media::Tools;
use player::Wakers;
use projection::{Event, Projection, Projector};
use rune::{
    AnyWindowSpec, App, AppSpec, Display, DisplayTarget, KeyPolicy, OverlayInput, OverlayOptions,
    OverlayPlacement, OverlaySpec, WindowOptions, WindowSpec,
};
use settings::Settings;
use std::sync::Arc;

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
    let startup = Startup::parse(rune::extra_args());
    rune::run_windows(move |displays| build(startup.clone(), displays));
}

fn build(startup: Startup, displays: &[Display]) -> Vec<AnyWindowSpec> {
    let (cfg, settings_problem) = Settings::load();
    let tools = Tools::discover();
    let wakers = Wakers::new();
    let displays: Vec<Display> = displays.to_vec();

    let mut control = App::new(AppSpec {
        min_size: Some((860.0, 620.0)),
        fonts: rune_theme_opencode::FONTS,
        families: rune_theme_opencode::FAMILIES,
        setup: Some(control::presenter_setup),
        poll: Some(control::presenter_poll),
        on_key: Some(control::presenter_key),
        ..AppSpec::new(
            "vidshow",
            (1120.0, 780.0),
            no_presenter,
            control::presenter_view,
        )
    });
    let control_handle = control.handle();

    let target = cfg.resolve(&displays).map(|index| displays[index].clone());
    // Points, not physical pixels: where `place` works it sets the exact physical rectangle anyway.
    let size = target
        .as_ref()
        .map(projection::display_points)
        .unwrap_or((1280.0, 720.0));
    let initial = target
        .as_ref()
        .map(projection::display_target)
        .unwrap_or(DisplayTarget::Primary);

    // The projector cannot reach into the control window, so it reports back: one wire for the
    // transport actions it is asked for and for closing itself.
    let notify: projection::Notify = Arc::new(move |event| {
        control_handle.update(move |s, _| match event {
            Event::Action(action) => {
                s.perform_playback(action);
            }
            Event::Closed => s.stop_presenting(),
        });
    });

    let state = Projection {
        player: None,
        fit: cfg.fit,
        hide_cursor: cfg.hide_cursor,
        notify,
        wakers: wakers.clone(),
        keybinds: Keybinds::defaults(),
    };

    // Wayland gives a client no say in where a window opens, so there the picture is a layer-shell
    // overlay pinned to the chosen output. Everywhere else it is an ordinary window, placed.
    let projection = if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        Projector::overlay(
            control.overlay(
                OverlaySpec::new("vidshow — presentation", size, projection::projection_view)
                    .options(OverlayOptions {
                        placement: Some(OverlayPlacement::FullScreen),
                        display: initial,
                        // The picture takes no clicks or keys; the control window keeps both.
                        input: OverlayInput::None,
                        keys: KeyPolicy::None,
                        ..Default::default()
                    })
                    .setup(projection::projection_setup),
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
            setup: Some(projection::projection_setup),
            on_key: Some(projection::projection_key),
            ..AppSpec::new(
                "vidshow — presentation",
                size,
                no_projection,
                projection::projection_view,
            )
        })
        .on_close(projection::projection_closed)
        // Maps at its final size, so a tiling window manager floats it instead of tiling it.
        .float_on_open();
        let (state, handle) = control.window(
            WindowSpec::from_app("projection", app)
                .state(state)
                .options(options),
        );
        Projector::window(handle, state)
    };

    let control = control.init(move || Presenter {
        tools,
        keybinds: Keybinds::defaults(),
        clip: Clip::None,
        cfg,
        displays,
        overlay: Overlay::None,
        presenting: false,
        projection,
        wakers: wakers.clone(),
        notice: settings_problem,
        startup: Some(startup),
        scrub: None,
    });

    vec![WindowSpec::from_app("control", control).into()]
}
