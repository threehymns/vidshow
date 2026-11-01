//! The command line: what Explorer, or a shell, asked vidshow to do.
//!
//! This is the boundary where a bad argument becomes a sentence for the control window to show, so
//! nothing further in has to care whether a path came from here, the file picker or a drop.

use std::path::PathBuf;

/// What the command line asked for, applied once the window exists.
#[derive(Clone, Debug)]
pub struct Startup {
    pub file: Option<PathBuf>,
    pub present: bool,
    pub autoplay: bool,
    /// A part of the command line vidshow could not follow, for the window to say.
    pub problem: Option<String>,
}

impl Startup {
    /// Reads the arguments rune left us; the rest is the file Explorer (or a shell) passed us.
    pub fn parse(args: &[String]) -> Self {
        let mut startup = Self {
            file: None,
            present: false,
            autoplay: true,
            problem: None,
        };
        for arg in args {
            match arg.as_str() {
                "--present" => startup.present = true,
                "--no-play" => startup.autoplay = false,
                other if other.starts_with('-') => {
                    startup.problem = Some(format!("vidshow has no option {other}"));
                }
                other => {
                    let path = PathBuf::from(other);
                    if !path.is_file() {
                        startup.problem = Some(format!("{other} is not a file"));
                    } else if startup.file.is_none() {
                        startup.file = Some(path);
                    }
                }
            }
        }
        startup
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|arg| arg.to_string()).collect()
    }

    #[test]
    fn flags_are_read_and_an_unknown_one_is_reported() {
        let startup = Startup::parse(&args(&["--present", "--no-play", "--wat"]));
        assert!(startup.present);
        assert!(!startup.autoplay);
        assert_eq!(
            startup.problem.as_deref(),
            Some("vidshow has no option --wat")
        );
    }

    #[test]
    fn a_path_that_is_not_a_file_is_reported_here() {
        let startup = Startup::parse(&args(&["no-such-clip.mkv"]));
        assert_eq!(startup.file, None);
        assert_eq!(
            startup.problem.as_deref(),
            Some("no-such-clip.mkv is not a file")
        );
    }
}
