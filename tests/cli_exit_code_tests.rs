use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TempSource {
    path: std::path::PathBuf,
}

impl TempSource {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("lcov_filter_cli_{}_{}.rs", std::process::id(), seq));
        fs::File::create(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempSource {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn run_lcov_filter_text_mode(input: &str) -> Result<Output, Box<dyn std::error::Error>> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_lcov_filter"))
        .arg("--text")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| std::io::Error::other("stdin should be available"))?;
    stdin.write_all(input.as_bytes())?;
    drop(stdin);

    Ok(child.wait_with_output()?)
}

fn build_lcov(path: &Path, hits: u64) -> String {
    let lh = if hits == 0 { 0 } else { 1 };
    format!(
        "TN:\nSF:{}\nDA:1,{hits}\nLF:1\nLH:{lh}\nend_of_record\n",
        path.display()
    )
}

#[test]
fn test_text_mode_exit_code_when_missing_lines() -> Result<(), Box<dyn std::error::Error>> {
    let source = TempSource::new()?;
    let mut file = fs::OpenOptions::new().write(true).open(source.path())?;
    writeln!(file, "fn sample() {{}}")?;

    let output = run_lcov_filter_text_mode(&build_lcov(source.path(), 0))?;

    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("Missing Lines (1)\n{}: 1\n", source.path().display())
    );
    assert_eq!(output.status.code(), Some(1));
    Ok(())
}

#[test]
fn test_text_mode_exit_code_when_full_coverage() -> Result<(), Box<dyn std::error::Error>> {
    let source = TempSource::new()?;
    let mut file = fs::OpenOptions::new().write(true).open(source.path())?;
    writeln!(file, "fn sample() {{}}")?;

    let output = run_lcov_filter_text_mode(&build_lcov(source.path(), 1))?;

    assert_eq!(String::from_utf8_lossy(&output.stdout), "100% coverage\n");
    assert_eq!(output.status.code(), Some(0));
    Ok(())
}
