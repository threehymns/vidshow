//! ffmpeg is vidshow's decoder, so everything ffmpeg can read, vidshow can present.
//!
//! `Tools` finds the binaries, `probe` asks ffprobe what a file is, and the `spawn_*` functions
//! start ffmpeg writing raw frames (or PCM) to a pipe. Rune's own `Player` is macOS-only, so the
//! decode path here is ours: BGRA frames pushed into a `NativeSurface`.

use serde_json::Value;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

/// Hidden console windows: a GUI app spawning ffmpeg on Windows must not flash a console.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn command(program: &Path) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let mut command = Command::new(program);
        command.creation_flags(CREATE_NO_WINDOW);
        command
    }
    #[cfg(not(windows))]
    {
        Command::new(program)
    }
}

fn exe_name(stem: &str) -> String {
    if cfg!(windows) {
        format!("{stem}.exe")
    } else {
        stem.to_string()
    }
}

#[derive(Clone, Debug)]
pub struct Tools {
    pub ffmpeg: PathBuf,
    pub ffprobe: PathBuf,
}

impl Tools {
    /// Search order: `VIDSHOW_FFMPEG` (a directory or the ffmpeg binary), then `ffmpeg\`,
    /// `ffmpeg\bin\`, `bin\` and the exe's own folder, then `PATH`. ffprobe is expected beside
    /// ffmpeg.
    pub fn discover() -> Result<Self, String> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        let mut direct: Option<PathBuf> = None;
        if let Some(value) = std::env::var_os("VIDSHOW_FFMPEG") {
            let path = PathBuf::from(value);
            if path.is_file() {
                direct = path.parent().map(Path::to_path_buf);
            } else if path.is_dir() {
                dirs.push(path);
            }
        }
        if let Some(dir) = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf))
        {
            dirs.push(dir.join("ffmpeg"));
            dirs.push(dir.join("ffmpeg").join("bin"));
            dirs.push(dir.join("bin"));
            dirs.push(dir);
        }
        if let Some(path) = std::env::var_os("PATH") {
            dirs.extend(std::env::split_paths(&path));
        }
        if let Some(dir) = direct {
            dirs.insert(0, dir);
        }

        for dir in &dirs {
            let ffmpeg = dir.join(exe_name("ffmpeg"));
            let ffprobe = dir.join(exe_name("ffprobe"));
            if ffmpeg.is_file() && ffprobe.is_file() {
                return Ok(Self { ffmpeg, ffprobe });
            }
        }
        let tried: Vec<String> = dirs
            .iter()
            .take(4)
            .map(|d| d.display().to_string())
            .collect();
        Err(format!(
            "ffmpeg and ffprobe were not found. Put ffmpeg.exe and ffprobe.exe beside vidshow.exe \
             (or in a bin folder beside it), add them to PATH, or set VIDSHOW_FFMPEG to their \
             folder. Searched: {}",
            tried.join("; ")
        ))
    }
}

/// The frame rate, held as both ffmpeg's `-r` argument and the number it means. Only
/// [`FrameRate::new`] builds one, so the two can never describe different rates.
#[derive(Clone, Debug)]
pub struct FrameRate {
    arg: String,
    value: f64,
}

impl Default for FrameRate {
    fn default() -> Self {
        Self {
            arg: "30/1".to_string(),
            value: 30.0,
        }
    }
}

impl FrameRate {
    /// A rate a decoder can be paced against. An absurd one is refused rather than clamped, so the
    /// number and the argument told to ffmpeg can never disagree.
    fn new(arg: String, value: f64) -> Option<Self> {
        (1.0..=480.0)
            .contains(&value)
            .then_some(Self { arg, value })
    }

    /// Exactly what ffmpeg is told, so index-based timing matches its output.
    pub fn arg(&self) -> &str {
        &self.arg
    }

    pub fn value(&self) -> f64 {
        self.value
    }
}

/// How far the picture is turned to stand upright. Only quarter turns are ever applied, so a
/// display matrix that says anything else is no rotation at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rotation {
    #[default]
    None,
    Cw90,
    Cw180,
    Cw270,
}

impl Rotation {
    /// From ffprobe's display matrix, whose rotation is counter-clockwise degrees.
    fn from_display_matrix(degrees: f64) -> Self {
        let clockwise = (360.0 - degrees.round()).rem_euclid(360.0) as i64;
        match clockwise {
            90 => Self::Cw90,
            180 => Self::Cw180,
            270 => Self::Cw270,
            _ => Self::None,
        }
    }

    /// Whether applying it swaps width and height.
    fn swaps_dimensions(self) -> bool {
        matches!(self, Self::Cw90 | Self::Cw270)
    }

    /// The degrees clockwise, for display.
    pub fn degrees(self) -> i32 {
        match self {
            Self::None => 0,
            Self::Cw90 => 90,
            Self::Cw180 => 180,
            Self::Cw270 => 270,
        }
    }

    /// The ffmpeg filter that applies it.
    fn filter(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Cw90 => Some("transpose=1"),
            Self::Cw180 => Some("hflip,vflip"),
            Self::Cw270 => Some("transpose=2"),
        }
    }
}

/// What vidshow needs to know before decoding: how big, how fast and how long.
#[derive(Clone, Debug)]
pub struct MediaInfo {
    pub path: PathBuf,
    pub duration: f64,
    /// Upright dimensions: already swapped for a quarter turn.
    pub width: u32,
    pub height: u32,
    pub fps: FrameRate,
    pub rotation: Rotation,
    /// Read by the audio back end; platforms without one play silently.
    pub has_audio: bool,
}

impl MediaInfo {
    pub fn frame_bytes(&self) -> usize {
        self.width as usize * self.height as usize * 4
    }

    /// The file's own name, for the control window's title.
    pub fn title(&self) -> String {
        title_of(&self.path)
    }
}

fn run_json(program: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let output = command(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("can't run {}: {e}", program.display()))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let line = err
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("unknown error");
        return Err(format!(
            "{} couldn't read this file: {line}",
            program.display()
        ));
    }
    Ok(output.stdout)
}

fn ratio(value: Option<&Value>) -> Option<(String, f64)> {
    let text = value?.as_str()?;
    let (num, den) = text.split_once('/')?;
    let (num, den) = (num.parse::<f64>().ok()?, den.parse::<f64>().ok()?);
    if num <= 0.0 || den <= 0.0 {
        return None;
    }
    Some((format!("{}/{den}", num as i64), num / den))
}

pub fn probe(tools: &Tools, path: &Path) -> Result<MediaInfo, String> {
    let path_str = path.to_string_lossy().into_owned();
    let json = run_json(
        &tools.ffprobe,
        &[
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            &path_str,
        ],
    )?;
    let root: Value =
        serde_json::from_slice(&json).map_err(|e| format!("can't read ffprobe's answer: {e}"))?;
    let streams = root["streams"].as_array().cloned().unwrap_or_default();

    let video = streams
        .iter()
        .find(|s| {
            s["codec_type"] == "video"
                && s["disposition"]["attached_pic"].as_i64().unwrap_or(0) == 0
        })
        .ok_or_else(|| format!("{} has no video stream", title_of(path)))?;
    let width = video["width"].as_u64().unwrap_or(0) as u32;
    let height = video["height"].as_u64().unwrap_or(0) as u32;
    if width == 0 || height == 0 {
        return Err(format!("{} reports no picture size", title_of(path)));
    }

    let fps = ratio(video.get("avg_frame_rate"))
        .or_else(|| ratio(video.get("r_frame_rate")))
        .and_then(|(arg, value)| FrameRate::new(arg, value))
        .unwrap_or_default();

    let rotation = video
        .get("side_data_list")
        .and_then(|list| list.as_array())
        .and_then(|list| {
            list.iter()
                .find_map(|entry| entry.get("rotation").and_then(Value::as_f64))
        })
        .or_else(|| {
            video["tags"]["rotate"]
                .as_str()
                .and_then(|text| text.trim().parse::<f64>().ok())
        })
        .map(Rotation::from_display_matrix)
        .unwrap_or_default();

    let duration = root["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse::<f64>().ok())
        .or_else(|| {
            video["duration"]
                .as_str()
                .and_then(|d| d.parse::<f64>().ok())
        })
        .unwrap_or(0.0);

    let has_audio = streams.iter().any(|s| s["codec_type"] == "audio");

    let (width, height) = if rotation.swaps_dimensions() {
        (height, width)
    } else {
        (width, height)
    };
    Ok(MediaInfo {
        path: path.to_path_buf(),
        duration,
        width,
        height,
        fps,
        rotation,
        has_audio,
    })
}

fn title_of(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// The last few lines ffmpeg wrote to stderr, for error messages when it exits early.
pub type Stderr = Arc<Mutex<String>>;

fn drain_stderr(stderr: std::process::ChildStderr) -> Stderr {
    let log = Arc::new(Mutex::new(String::new()));
    let sink = log.clone();
    std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            let mut text = sink.lock().unwrap();
            text.push_str(&line);
            text.push('\n');
            if text.len() > 4096 {
                let tail: Vec<String> = text.lines().rev().take(8).map(str::to_string).collect();
                *text = tail.into_iter().rev().collect::<Vec<_>>().join("\n");
                text.push('\n');
            }
        }
    });
    log
}

fn spawn_with_stderr(mut command: Command) -> Result<(Child, Stderr), String> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .map_err(|e| format!("can't start ffmpeg: {e}"))?;
    let log = match child.stderr.take() {
        Some(stderr) => drain_stderr(stderr),
        None => Arc::new(Mutex::new(String::new())),
    };
    Ok((child, log))
}

/// Stops a decoder process and reaps it, so the next pass can start at once.
pub fn reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// The options both decoders share: quiet, seeking before the input, and no subtitle or data
/// streams. `input` holds the input options the caller needs, which ffmpeg wants before `-i`.
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
fn decoder(tools: &Tools, info: &MediaInfo, start: f64, input: &[&str]) -> Command {
    let mut command = command(&tools.ffmpeg);
    command.args(["-nostdin", "-hide_banner", "-loglevel", "error"]);
    command.args(input);
    if start > 0.05 {
        command.arg("-ss").arg(format!("{start:.3}"));
    }
    command.arg("-i").arg(&info.path);
    command.args(["-sn", "-dn"]);
    command
}

/// Starts decoding video to raw BGRA on stdout, from `start` seconds in.
pub fn spawn_video(tools: &Tools, info: &MediaInfo, start: f64) -> Result<(Child, Stderr), String> {
    // Autorotate is off so the output size is exactly what `probe` computed; the rotation is
    // applied here instead.
    let mut command = decoder(tools, info, start, &["-noautorotate"]);
    command.args(["-map", "0:v:0", "-an"]);
    if let Some(filter) = info.rotation.filter() {
        command.args(["-vf", filter]);
    }
    command.args([
        "-r",
        info.fps.arg(),
        "-f",
        "rawvideo",
        "-pix_fmt",
        "bgra",
        "pipe:1",
    ]);
    spawn_with_stderr(command)
}

/// Starts decoding the first audio track to interleaved f32 PCM at the output device's rate.
#[cfg_attr(not(any(windows, target_os = "linux")), allow(dead_code))]
pub fn spawn_audio(
    tools: &Tools,
    info: &MediaInfo,
    start: f64,
    rate: u32,
    channels: u16,
) -> Result<(Child, Stderr), String> {
    let (rate, channels) = (rate.to_string(), channels.to_string());
    let mut command = decoder(tools, info, start, &[]);
    command.args(["-map", "0:a:0", "-vn"]);
    command.args(["-ac", &channels, "-ar", &rate]);
    command.args(["-f", "f32le", "pipe:1"]);
    spawn_with_stderr(command)
}

/// Reads exactly `size` bytes; `Ok(false)` at a clean end of stream.
pub fn read_frame(reader: &mut impl Read, buffer: &mut [u8]) -> Result<bool, String> {
    let mut filled = 0;
    while filled < buffer.len() {
        match reader.read(&mut buffer[filled..]) {
            Ok(0) => {
                if filled == 0 {
                    return Ok(false);
                }
                return Err("ffmpeg stopped mid-frame".into());
            }
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(format!("can't read decoded frames: {e}")),
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_becomes_clockwise_quarter_turns() {
        use Rotation::{Cw90, Cw180, Cw270};
        assert_eq!(Rotation::from_display_matrix(-90.0), Cw90);
        assert_eq!(Rotation::from_display_matrix(90.0), Cw270);
        assert_eq!(Rotation::from_display_matrix(180.0), Cw180);
        assert_eq!(Rotation::from_display_matrix(0.0), Rotation::None);
        assert_eq!(Rotation::from_display_matrix(-270.0), Cw270);
        // A rotation that is not a quarter turn cannot be applied to the frame size.
        assert_eq!(Rotation::from_display_matrix(45.0), Rotation::None);
        assert_eq!(Cw90.degrees(), 90);
    }

    /// The real command line, against a real ffmpeg: probe a generated clip, then decode a frame
    /// and check it is exactly the BGRA size `probe` promised. Skipped without ffmpeg.
    #[test]
    fn decodes_frames_at_the_size_probe_reported() {
        let Some(tools) = test_tools() else {
            eprintln!("skipping the decode test: ffmpeg and ffprobe are not available");
            return;
        };
        let dir = std::env::temp_dir().join(format!("vidshow-decode-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let clip = dir.join("clip.mp4");
        let made = command(&tools.ffmpeg)
            .args([
                "-nostdin",
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=320x240:rate=25",
                "-t",
                "2",
                "-pix_fmt",
                "yuv420p",
            ])
            .arg(&clip)
            .status()
            .unwrap();
        assert!(made.success(), "ffmpeg could not write the test clip");

        let info = probe(&tools, &clip).unwrap();
        assert_eq!((info.width, info.height), (320, 240));
        assert!(
            (info.fps.value() - 25.0).abs() < 0.1,
            "fps {}",
            info.fps.value()
        );
        assert!(
            (info.duration - 2.0).abs() < 0.4,
            "duration {}",
            info.duration
        );
        assert_eq!(info.rotation, Rotation::None);

        let (mut child, _log) = spawn_video(&tools, &info, 0.0).unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut frame = vec![0u8; info.frame_bytes()];
        assert!(
            read_frame(&mut reader, &mut frame).unwrap(),
            "a frame arrives"
        );
        assert!(
            frame.iter().any(|byte| *byte != 0),
            "testsrc has colour in it"
        );
        let _ = child.kill();
        let _ = child.wait();

        // A seek reopens ffmpeg late in the clip and still yields a whole frame.
        let (mut child, _log) = spawn_video(&tools, &info, 1.0).unwrap();
        let mut reader = BufReader::new(child.stdout.take().unwrap());
        let mut frame = vec![0u8; info.frame_bytes()];
        assert!(
            read_frame(&mut reader, &mut frame).unwrap(),
            "a frame after seeking"
        );
        let _ = child.kill();
        let _ = child.wait();

        let _ = std::fs::remove_dir_all(&dir);
    }

    fn test_tools() -> Option<Tools> {
        let local = Path::new(env!("CARGO_MANIFEST_DIR")).join(".tools");
        let (ffmpeg, ffprobe) = (
            local.join(exe_name("ffmpeg")),
            local.join(exe_name("ffprobe")),
        );
        if ffmpeg.is_file() && ffprobe.is_file() {
            return Some(Tools { ffmpeg, ffprobe });
        }
        Tools::discover().ok()
    }

    #[test]
    fn frame_rate_strings_keep_their_ratio() {
        let value = serde_json::json!("30000/1001");
        let (arg, fps) = ratio(Some(&value)).unwrap();
        let rate = FrameRate::new(arg, fps).unwrap();
        assert_eq!(rate.arg(), "30000/1001");
        assert!((rate.value() - 29.97).abs() < 0.01);
        assert!(ratio(Some(&serde_json::json!("0/0"))).is_none());
        // A rate nothing can be paced against is refused, so arg and value stay in agreement.
        assert!(FrameRate::new("100000/1".to_string(), 100_000.0).is_none());
    }
}
