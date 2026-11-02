# vidshow

A small, utilitarian app for showing a video file to a room. One window has the controls; pressing
**Present** opens a second, borderless window covering the display you chose — the same idea as
PowerPoint's presenter output or an OBS projector — and that window shows nothing but the picture.

Built on [rune](https://github.com/anomalyco/rune), the native Rust UI framework, with **ffmpeg** as
the decoder, so it reads essentially anything ffmpeg reads.

## Download

A portable Windows build — the app with `ffmpeg.exe` and `ffprobe.exe` beside it, no installer — is
attached to each [release](../../releases). Unzip it anywhere and run `vidshow.exe`. The folder
contains a `vidshow.portable` marker, so settings are written to `settings.json` beside the exe;
delete that marker to keep them in `%APPDATA%\vidshow` instead.

## Requirements

- **Windows 10/11** is the target. It also builds and runs on Linux (X11 and Wayland, with sound)
  and on macOS (where video presents silently).
- **ffmpeg and ffprobe**. vidshow looks for them in this order:
  1. `VIDSHOW_FFMPEG` — a folder containing both, or the `ffmpeg.exe` itself;
  2. beside `vidshow.exe`, or in `ffmpeg\`, `ffmpeg\bin\`, `bin\` beside it;
  3. `PATH`.

  Any normal Windows build works, for example `winget install Gyan.FFmpeg` or the "release full"
  zip from <https://www.gyan.dev/ffmpeg/builds/>. Copy `ffmpeg.exe` and `ffprobe.exe` next to
  `vidshow.exe` if you would rather not add them to `PATH`.

If ffmpeg is missing the app still opens and says so, with the folders it searched.

## Build

```powershell
cargo build --release
# target\release\vidshow.exe
```

Rune is used as a pinned git dependency (`rev f5def6d4…` in `Cargo.toml`), so the first build pulls
and compiles it.

## Run

```powershell
vidshow.exe                          # opens empty; pick a file, or drop one on the window
vidshow.exe "D:\clips\keynote.mkv"   # opens that file
vidshow.exe "D:\clips\keynote.mkv" --present   # and presents on the chosen display straight away
vidshow.exe --no-play "D:\clips\b-roll.mp4"    # loads paused
```

You can also drag a video onto the control window, use **Open…**, or start it from Explorer (below).

## Presenting on a second monitor

- The **Present** button opens the projector on the display selected under *Present on*. With two
  monitors the default is the secondary display; with one it stays on that display.
- The projector is a borderless window sized and positioned to exactly cover its display, kept
  above other windows, with the mouse pointer hidden over it. It is not exclusive full screen, so
  alt-tab, notifications and screen sharing behave normally. On Wayland it is a layer-shell overlay
  pinned to that display instead of a window (see [Linux](#linux)).
- **Stop presenting** (or Escape, or closing the projector) hides it; the control window keeps
  playing, so you can check a moment before showing it again.
- *Fit* letterboxes, *Fill* crops to cover the display. Both apply live, to the preview and the
  projector.
- *Preview* draws the same frames in the control window (it can be turned off to save a little
  work on very large files). *Hide cursor* and *On top* control the projector.

Display choice is remembered in `%APPDATA%\vidshow\settings.json`, matched by monitor name,
resolution and position so it survives restarts and redocking. Displays are read when the app
starts: if you plug a monitor in while vidshow is running, restart it to see the new one.

```json
{
  "display": { "mode": "display", "name": "\\\\.\\DISPLAY2", "width": 1920, "height": 1080,
               "x": 2560, "y": 0, "primary": false },
  "always_on_top": true,
  "hide_cursor": true,
  "fit": "fit",
  "looping": false,
  "muted": false,
  "volume": 1.0,
  "preview": true,
  "last_dir": "D:\\clips"
}
```

`"display"` may also be `{ "mode": "secondary" }` (the default) or `{ "mode": "primary" }`.

`"volume"` is a gain: `1.0` is unity (0 dB, the fader's 0 mark), `0.0` is silence, and the top of
the fader is `2.0` (+6 dB), so a quiet file can be pushed past unity the way a player's 200%
setting can.

## Linux

The control window and the projector both work on X11 and on Wayland, and sound plays through
ALSA.

- **On Wayland the projector is a layer-shell overlay** pinned to the display chosen under
  *Present on*, covering it whole and sitting above ordinary windows. That is the one surface a
  Wayland client can put on a chosen output: an ordinary window opens wherever the compositor
  decides, which is normally the display the control window is on. Compositors with the
  `wlr-layer-shell` protocol (sway, Hyprland, niri, river, KDE) get the overlay; GNOME has no such
  protocol, so Rune falls back to an override-redirect XWayland window, which covers the display
  too.
- The trade is that the overlay is not a window: it is not in alt-tab or task switchers, and it
  takes no keyboard focus. Keys go to the control window, which has all of them (Escape there stops
  presenting); the projector-only shortcuts in the key table apply to the window, on Windows and
  X11.
- On X11 the projector is a borderless window placed on the chosen display exactly as on Windows,
  and it maps at its exact size so a tiling window manager (i3, bspwm, dwm) floats it instead of
  giving it a tile. Floating desktops just show a window that fills the display.
- *On top* only applies to the window; a layer-shell overlay is already above ordinary windows.
- Sound needs ALSA's development files at build time (`libasound2-dev` on Debian/Ubuntu,
  `alsa-lib-devel` on Fedora) and a working default output device; without them the picture still
  plays and the control window says why there is no sound.

## Open With in File Explorer

Register vidshow for the current user (no administrator rights, nothing outside `HKCU`):

```powershell
.\scripts\register-open-with.ps1 -ExePath 'C:\Tools\vidshow\vidshow.exe'
# or, after a release build in this repository:
.\scripts\register-open-with.ps1
```

That writes the app's ProgID, an `Applications\vidshow.exe` entry with its supported types, and an
`OpenWithProgids` value for each common video extension, then tells Explorer the associations
changed. Right-click a video → **Open with** → *vidshow* (or "Choose another app", where vidshow now
appears). Add `-OpenWithDefault` to also make it the default for those extensions.

Windows passes the file as the first argument, which is exactly what a normal launch handles.

To undo it:

```powershell
.\scripts\unregister-open-with.ps1
```

## Keys

| Key | Control window | Projector |
| --- | --- | --- |
| Space | play / pause | play / pause |
| ← / → | seek 5 s (Shift: 30 s) | seek 5 s (Shift: 30 s) |
| Home | back to the start | back to the start |
| ↑ / ↓ | volume ±3 dB | — |
| F | present / stop presenting | — |
| Escape | stop presenting | stop presenting |
| M | mute | mute |
| L | loop | loop |
| V | preview on/off | — |
| C | hide cursor on/off | — |
| Ctrl+O | open a file | — |
| F5 | present / stop presenting | — |

## Formats

Whatever the installed ffmpeg decodes: MP4/MOV (H.264, HEVC, AV1, ProRes), MKV, WebM, AVI, WMV/ASF,
MPEG-1/2/4, transport streams, Ogg/Theora, VP8/VP9, VOB, MXF, 3GP, and audio tracks in the usual
codecs. The file picker lists the common ones; **All files** shows everything.

Decoding is converted to 8-bit BGRA for display. HDR and 10-bit sources are converted to 8-bit SDR
by ffmpeg rather than tone-mapped carefully, and rotation metadata is applied from the file. Odd
sample-aspect-ratio files (old anamorphic content) are not un-squeezed.

## How it works

- The projector is a separate Rune window with its own state, created hidden at startup and placed
  with `WindowHandle::place(DisplayTarget::Id(..), OverlayPlacement::FullScreen)` when you press
  Present, so it never flashes on the wrong screen. It is built with `App::float_on_open` so a
  tiling window manager floats it rather than tiling it. On Wayland, where a window cannot be
  placed, the projector is a Rune layer-shell overlay (`OverlaySpec` with
  `OverlayPlacement::FullScreen` on the chosen display) instead.
- Rune's built-in `Player` is macOS-only, so vidshow runs its own pipeline: `ffprobe` reports size,
  frame rate, rotation and duration; an ffmpeg process writes raw `bgra` frames to a pipe; a decode
  thread paces each frame against a monotonic clock and pushes it into Rune's `NativeSurface`
  (`push_bytes`), which both windows draw. Seeks restart ffmpeg at the new timestamp; pausing stops
  the thread consuming frames, which back-pressures ffmpeg.
- Sound runs through a second ffmpeg piped into `cpal`'s default output device, with a 250 ms ring
  buffer. If the device or the track is unavailable the picture continues and the control window
  says why.
- `src/media.rs` finds ffmpeg and builds the commands, `src/player/` is the clock and the decode
  thread, `src/control/` the control window, `src/projection/` the projector, `src/settings.rs`
  the saved state, `src/cli.rs` the command line, `src/audio.rs` the sound.

## Limits

- Sound plays on Windows and Linux; on macOS video presents silently and the control window says so.
- On Wayland the projector is a layer-shell overlay rather than a window: no alt-tab entry, and
  keyboard control comes from the control window (see the Linux section).
- No exclusive full screen on purpose (see above), so a screen saver or a "stay on top" window can
  still cover the projector.
- A/V drift is not continuously corrected: audio and video are started together and left to run.
- Starting a second file from Explorer while vidshow is open starts a second vidshow; there is no
  single-instance forwarding.
- Displays are enumerated at startup; hot-plugging needs a restart.

## Development

```sh
cargo test        # unit tests, plus a real decode test when ffmpeg is available
cargo check --target x86_64-pc-windows-msvc   # type-check the Windows-only paths
```

On Windows these are ordinary native builds. The cross-target check compiles every dependency for
Windows, which would otherwise need a Windows C compiler for `blake3`'s assembly, so `blake3` is
taken with its `pure` feature: no C toolchain for the target, and no extra runtime library in the
shipped exe. The hashes are identical, just slower.

The decode test skips itself when ffmpeg and ffprobe cannot be found. Point `VIDSHOW_FFMPEG` at a
folder with both to run it.
