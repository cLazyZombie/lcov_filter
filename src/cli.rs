use std::env;
use std::io::{self, Read, Write};

use crate::{
    Markers, OutputMode, filter_lcov, filter_lcov_records, format_missing_lines, has_missing_lines,
};

#[derive(Debug, PartialEq, Eq)]
pub struct Args {
    pub markers: Markers,
    pub mode: OutputMode,
    pub grep: Option<String>,
}

fn print_usage(program: &str) {
    eprintln!(
        "Usage: cat lcov.info | {program} [--lcov|--text] [--grep STR] [--marker-line STR] [--marker-start STR] [--marker-stop STR]"
    );
}

fn next_value<I>(iter: &mut I, option: &str) -> Result<String, String>
where
    I: Iterator<Item = String>,
{
    iter.next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn set_mode(
    mode: &mut OutputMode,
    explicit_mode: &mut Option<OutputMode>,
    next: OutputMode,
) -> Result<(), String> {
    if let Some(previous) = explicit_mode
        && *previous != next
    {
        return Err("Cannot use --lcov and --text together".to_string());
    }
    *mode = next;
    *explicit_mode = Some(next);
    Ok(())
}

pub fn parse_args_from<I>(program: &str, args: I) -> Result<Args, String>
where
    I: IntoIterator<Item = String>,
{
    let mut marker_line = "LCOV_EXCL_LINE".to_string();
    let mut marker_start = "LCOV_EXCL_START".to_string();
    let mut marker_stop = "LCOV_EXCL_STOP".to_string();
    let mut mode = OutputMode::Lcov;
    let mut explicit_mode = None;
    let mut grep = None;

    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage(program);
                return Err(String::new());
            }
            "--lcov" => set_mode(&mut mode, &mut explicit_mode, OutputMode::Lcov)?,
            "--text" => set_mode(&mut mode, &mut explicit_mode, OutputMode::Text)?,
            "--grep" => {
                grep = Some(next_value(&mut iter, "--grep")?);
            }
            "--marker-line" => {
                marker_line = next_value(&mut iter, "--marker-line")?;
            }
            "--marker-start" => {
                marker_start = next_value(&mut iter, "--marker-start")?;
            }
            "--marker-stop" => {
                marker_stop = next_value(&mut iter, "--marker-stop")?;
            }
            _ if arg.starts_with("--grep=") => {
                grep = Some(value_after_equals(&arg));
            }
            _ if arg.starts_with("--marker-line=") => {
                marker_line = value_after_equals(&arg);
            }
            _ if arg.starts_with("--marker-start=") => {
                marker_start = value_after_equals(&arg);
            }
            _ if arg.starts_with("--marker-stop=") => {
                marker_stop = value_after_equals(&arg);
            }
            _ if arg.starts_with('-') => {
                return Err(format!("Unknown option: {arg}"));
            }
            _ => {
                return Err("Unexpected positional argument (stdin only)".to_string());
            }
        }
    }

    Ok(Args {
        markers: Markers {
            line: marker_line,
            start: marker_start,
            stop: marker_stop,
        },
        mode,
        grep,
    })
}

fn value_after_equals(arg: &str) -> String {
    arg.split_once('=')
        .map(|(_, value)| value)
        .unwrap_or("")
        .to_string()
}

pub fn parse_env_args() -> Result<Args, String> {
    let mut args = env::args();
    let program = args.next().unwrap_or_else(|| "lcov_filter".to_string());
    parse_args_from(&program, args)
}

fn read_stdin_lossy() -> io::Result<String> {
    let mut input = Vec::new();
    io::stdin().read_to_end(&mut input)?;
    Ok(String::from_utf8_lossy(&input).into_owned())
}

pub fn render_output(input: &str, args: &Args) -> (String, bool) {
    match args.mode {
        OutputMode::Lcov => (filter_lcov(input, &args.markers), false),
        OutputMode::Text => {
            let records = filter_lcov_records(input, &args.markers);
            let grep = args.grep.as_deref();
            (
                format_missing_lines(&records, grep),
                has_missing_lines(&records, grep),
            )
        }
    }
}

pub fn run() -> i32 {
    let args = match parse_env_args() {
        Ok(args) => args,
        Err(err) => {
            if err.is_empty() {
                return 0;
            }
            eprintln!("{err}");
            return 2;
        }
    };

    let input = match read_stdin_lossy() {
        Ok(content) => content,
        Err(err) => {
            eprintln!("error: failed to read stdin: {err}");
            return 1;
        }
    };

    let (output, should_fail) = render_output(&input, &args);
    let mut stdout = io::stdout();
    if let Err(err) = stdout.write_all(output.as_bytes()) {
        eprintln!("error: failed to write output: {err}");
        return 1;
    }
    if should_fail { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Args, String> {
        parse_args_from("lcov_filter", args.iter().map(|arg| arg.to_string()))
    }

    #[test]
    fn test_parse_args_from_when_default_then_lcov_mode() -> Result<(), String> {
        let args = parse(&[])?;

        assert_eq!(args.mode, OutputMode::Lcov);
        assert_eq!(args.grep, None);
        assert_eq!(args.markers, Markers::default());
        Ok(())
    }

    #[test]
    fn test_parse_args_from_with_text_and_grep() -> Result<(), String> {
        let args = parse(&["--text", "--grep", "crate_a"])?;

        assert_eq!(args.mode, OutputMode::Text);
        assert_eq!(args.grep, Some("crate_a".to_string()));
        Ok(())
    }

    #[test]
    fn test_parse_args_from_with_custom_markers() -> Result<(), String> {
        let args = parse(&[
            "--marker-line=IGNORE_LINE",
            "--marker-start",
            "IGNORE_START",
            "--marker-stop",
            "IGNORE_STOP",
        ])?;

        assert_eq!(args.markers.line, "IGNORE_LINE");
        assert_eq!(args.markers.start, "IGNORE_START");
        assert_eq!(args.markers.stop, "IGNORE_STOP");
        Ok(())
    }

    #[test]
    fn test_parse_args_from_when_modes_conflict_then_error() -> Result<(), String> {
        let err = parse(&["--lcov", "--text"])
            .err()
            .ok_or_else(|| "conflicting output modes should fail".to_string())?;

        assert_eq!(err, "Cannot use --lcov and --text together");
        Ok(())
    }
}
