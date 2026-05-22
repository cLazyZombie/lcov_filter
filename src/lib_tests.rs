use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

const FIXTURE_CONTENT: &str = r#"pub fn demo(x: i32) -> i32 {
    if x == 0 { return 0; }
    let a = x + 1; // LCOV_EXCL_LINE
    let b = x + 2; // lcov_excl_line
    // LCOV_EXCL_START
    let c = a + b;
    let d = c * 2;
    // LCOV_EXCL_STOP
    // lcov_excl_start
    let e = d + 1;
    // lcov_excl_stop
    e
}"#;

static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

struct TempSource {
    path: PathBuf,
}

impl Drop for TempSource {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn unique_temp_path(label: &str) -> PathBuf {
    let seq = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "lcov_filter_{label}_{}_{}.rs",
        std::process::id(),
        seq
    ))
}

fn create_temp_fixture() -> Result<(TempSource, PathBuf), Box<dyn Error>> {
    let path = unique_temp_path("fixture");
    let mut file = fs::File::create(&path)?;
    file.write_all(FIXTURE_CONTENT.as_bytes())?;
    Ok((TempSource { path: path.clone() }, path))
}

fn load_fixture_lines() -> Vec<String> {
    FIXTURE_CONTENT
        .lines()
        .map(|line| line.to_string())
        .collect()
}

fn build_lcov_for_path(path: &Path) -> String {
    let lines = load_fixture_lines();
    let line_count = lines.len();
    let mut lcov = String::new();
    lcov.push_str("TN:\n");
    lcov.push_str(&format!("SF:{}\n", path.display()));
    lcov.push_str("FN:1,demo\n");
    lcov.push_str("FNDA:1,demo\n");
    for line_no in 1..=line_count {
        lcov.push_str(&format!("DA:{line_no},1\n"));
    }
    lcov.push_str("FNF:1\n");
    lcov.push_str("FNH:1\n");
    lcov.push_str(&format!("LF:{line_count}\n"));
    lcov.push_str(&format!("LH:{line_count}\n"));
    lcov.push_str("end_of_record\n");
    lcov
}

fn build_lcov_for_path_with_misses(path: &Path, misses: &HashSet<usize>) -> String {
    let lines = load_fixture_lines();
    let line_count = lines.len();
    let mut lcov = String::new();
    lcov.push_str("TN:\n");
    lcov.push_str(&format!("SF:{}\n", path.display()));
    lcov.push_str("FN:1,demo\n");
    lcov.push_str("FNDA:1,demo\n");
    for line_no in 1..=line_count {
        let hits = if misses.contains(&line_no) { 0 } else { 1 };
        lcov.push_str(&format!("DA:{line_no},{hits}\n"));
    }
    lcov.push_str("FNF:1\n");
    lcov.push_str("FNH:1\n");
    lcov.push_str(&format!("LF:{line_count}\n"));
    lcov.push_str(&format!("LH:{line_count}\n"));
    lcov.push_str("end_of_record\n");
    lcov
}

fn compute_expected_excluded(lines: &[String], markers: &Markers) -> HashSet<usize> {
    let mut excluded = HashSet::new();
    let mut in_block = false;

    for (idx, line) in lines.iter().enumerate() {
        let line_no = idx + 1;
        let has_line = !markers.line.is_empty() && line.contains(&markers.line);
        let has_start = !markers.start.is_empty() && line.contains(&markers.start);
        let has_stop = !markers.stop.is_empty() && line.contains(&markers.stop);

        if has_line {
            excluded.insert(line_no);
        }
        if in_block || has_start {
            excluded.insert(line_no);
        }
        if has_start {
            in_block = true;
        }
        if has_stop {
            excluded.insert(line_no);
            in_block = false;
        }
    }

    excluded
}

fn extract_da_lines(output: &str) -> HashSet<usize> {
    output
        .lines()
        .filter_map(|line| parse_da_line(line).map(|(line_no, _)| line_no))
        .collect()
}

fn extract_summary_value(output: &str, key: &str) -> usize {
    output
        .lines()
        .find_map(|line| line.strip_prefix(key))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0)
}

#[test]
fn test_format_missing_lines_with_text_output() -> Result<(), Box<dyn Error>> {
    let (_temp_file, path) = create_temp_fixture()?;
    let fixture_lines = load_fixture_lines();
    let markers = Markers::default();
    let excluded = compute_expected_excluded(&fixture_lines, &markers);
    let mut misses = HashSet::new();
    for line_no in 1..=fixture_lines.len() {
        if !excluded.contains(&line_no) {
            misses.insert(line_no);
            if misses.len() == 2 {
                break;
            }
        }
    }

    let input = build_lcov_for_path_with_misses(&path, &misses);
    let records = filter_lcov_records(&input, &markers);
    let output = format_missing_lines(&records, None);
    let mut expected_missing: Vec<usize> = misses
        .into_iter()
        .filter(|line_no| !excluded.contains(line_no))
        .collect();
    expected_missing.sort_unstable();

    let expected_line = format!(
        "{}: {}",
        path.display(),
        expected_missing
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );

    assert_eq!(
        output.trim(),
        format!(
            "Missing Lines ({})\n{expected_line}",
            expected_missing.len()
        )
    );
    Ok(())
}

#[test]
fn test_filter_lcov_when_default_markers_then_filters_case_sensitive() -> Result<(), Box<dyn Error>>
{
    let (_temp_file, path) = create_temp_fixture()?;
    let output = filter_lcov(&build_lcov_for_path(&path), &Markers::default());
    let fixture_lines = load_fixture_lines();
    let expected = compute_expected_excluded(&fixture_lines, &Markers::default());
    let da_lines = extract_da_lines(&output);

    for line_no in &expected {
        assert!(
            !da_lines.contains(line_no),
            "line {line_no} should be excluded"
        );
    }

    let lower_marker_line = fixture_lines
        .iter()
        .enumerate()
        .find(|(_, line)| line.contains("lcov_excl_line"))
        .map(|(idx, _)| idx + 1)
        .ok_or_else(|| std::io::Error::other("lowercase marker line should exist"))?;
    assert!(da_lines.contains(&lower_marker_line));
    assert_eq!(
        extract_summary_value(&output, "LF:"),
        fixture_lines.len() - expected.len()
    );
    assert_eq!(
        extract_summary_value(&output, "LH:"),
        fixture_lines.len() - expected.len()
    );
    Ok(())
}

#[test]
fn test_filter_lcov_with_custom_lowercase_markers() -> Result<(), Box<dyn Error>> {
    let (_temp_file, path) = create_temp_fixture()?;
    let markers = Markers {
        line: "lcov_excl_line".to_string(),
        start: "lcov_excl_start".to_string(),
        stop: "lcov_excl_stop".to_string(),
    };

    let output = filter_lcov(&build_lcov_for_path(&path), &markers);
    let fixture_lines = load_fixture_lines();
    let expected = compute_expected_excluded(&fixture_lines, &markers);
    let da_lines = extract_da_lines(&output);

    for line_no in &expected {
        assert!(
            !da_lines.contains(line_no),
            "line {line_no} should be excluded"
        );
    }

    let upper_marker_line = fixture_lines
        .iter()
        .enumerate()
        .find(|(_, line)| line.contains("LCOV_EXCL_LINE"))
        .map(|(idx, _)| idx + 1)
        .ok_or_else(|| std::io::Error::other("uppercase marker line should exist"))?;
    assert!(da_lines.contains(&upper_marker_line));
    assert_eq!(
        extract_summary_value(&output, "LF:"),
        fixture_lines.len() - expected.len()
    );
    assert_eq!(
        extract_summary_value(&output, "LH:"),
        fixture_lines.len() - expected.len()
    );
    Ok(())
}

#[test]
fn test_compute_excluded_lines_from_content_with_line_and_block_markers() {
    let excluded = compute_excluded_lines_from_content(FIXTURE_CONTENT, &Markers::default());

    assert!(excluded.contains(&3));
    assert!(excluded.contains(&5));
    assert!(excluded.contains(&6));
    assert!(excluded.contains(&7));
    assert!(excluded.contains(&8));
    assert!(!excluded.contains(&4));
}

#[test]
fn test_split_records_when_end_of_record_is_missing() {
    let records = split_records("TN:\nSF:/test.rs\nDA:1,1");

    assert_eq!(records.len(), 1);
    assert!(records[0].contains(&"end_of_record".to_string()));
}

#[test]
fn test_parse_brda_line_with_valid_and_invalid_input() -> Result<(), Box<dyn Error>> {
    let Some((line_no, block, branch, taken)) = parse_brda_line("BRDA:10,0,0,1") else {
        return Err(std::io::Error::other("BRDA line should parse").into());
    };

    assert_eq!(line_no, 10);
    assert_eq!(block, "0");
    assert_eq!(branch, "0");
    assert_eq!(taken, "1");
    assert!(parse_brda_line("BRDA:invalid").is_none());
    assert!(parse_brda_line("DA:1,1").is_none());
    Ok(())
}

#[test]
fn test_process_record_when_sf_line_is_missing() {
    let mut cache = HashMap::new();
    let record = vec![
        "TN:".to_string(),
        "DA:1,1".to_string(),
        "end_of_record".to_string(),
    ];

    let result = process_record(&record, &mut cache, &Markers::default());

    assert!(result.source_path.is_none());
    assert_eq!(result.lines, record);
}

#[test]
fn test_process_record_when_source_is_unreadable() {
    let mut cache = HashMap::new();
    let record = vec![
        "TN:".to_string(),
        "SF:/nonexistent/path/to/file.rs".to_string(),
        "DA:1,0".to_string(),
        "DA:2,1".to_string(),
        "end_of_record".to_string(),
    ];

    let result = process_record(&record, &mut cache, &Markers::default());

    assert!(result.source_path.is_some());
    assert_eq!(result.missing_lines, vec![1]);
}

#[test]
fn test_process_record_with_brda_lines() -> Result<(), Box<dyn Error>> {
    let (_temp_file, path) = create_temp_fixture()?;
    let mut cache = HashMap::new();
    let record = vec![
        "TN:".to_string(),
        format!("SF:{}", path.display()),
        "FN:1,demo".to_string(),
        "FNDA:1,demo".to_string(),
        "DA:1,1".to_string(),
        "DA:2,1".to_string(),
        "BRDA:1,0,0,1".to_string(),
        "BRDA:2,0,0,-".to_string(),
        "BRF:2".to_string(),
        "BRH:1".to_string(),
        "end_of_record".to_string(),
    ];

    let result = process_record(&record, &mut cache, &Markers::default());
    let output = result.lines.join("\n");

    assert!(output.contains("BRF:"));
    assert!(output.contains("BRH:"));
    Ok(())
}

#[test]
fn test_process_record_with_malformed_lines() -> Result<(), Box<dyn Error>> {
    let (_temp_file, path) = create_temp_fixture()?;
    let mut cache = HashMap::new();
    let record = vec![
        "TN:".to_string(),
        format!("SF:{}", path.display()),
        "FN:invalid".to_string(),
        "FNDA:invalid".to_string(),
        "DA:invalid".to_string(),
        "BRDA:invalid".to_string(),
        "end_of_record".to_string(),
    ];

    let result = process_record(&record, &mut cache, &Markers::default());
    let output = result.lines.join("\n");

    assert!(output.contains("FN:invalid"));
    assert!(output.contains("FNDA:invalid"));
    assert!(output.contains("DA:invalid"));
    assert!(output.contains("BRDA:invalid"));
    Ok(())
}

#[test]
fn test_format_missing_lines_without_source_path() {
    let records = vec![RecordResult {
        lines: vec!["TN:".to_string()],
        source_path: None,
        missing_lines: vec![1, 2, 3],
    }];

    assert_eq!(format_missing_lines(&records, None), FULL_COVERAGE_MESSAGE);
}

#[test]
fn test_format_missing_lines_with_empty_missing_lines() {
    let records = vec![RecordResult {
        lines: vec!["TN:".to_string()],
        source_path: Some(PathBuf::from("/test.rs")),
        missing_lines: vec![],
    }];

    assert_eq!(format_missing_lines(&records, None), FULL_COVERAGE_MESSAGE);
}

#[test]
fn test_format_missing_lines_with_empty_records() {
    let records = Vec::new();

    assert_eq!(format_missing_lines(&records, None), FULL_COVERAGE_MESSAGE);
}

#[test]
fn test_format_missing_lines_with_grep() {
    let records = sample_missing_records();

    let output = format_missing_lines(&records, Some("crate_a"));
    assert!(output.starts_with("Missing Lines (2)"));
    assert!(output.contains("crate_a"));
    assert!(!output.contains("crate_b"));
    assert!(!output.contains("crate_c"));

    let output = format_missing_lines(&records, Some("crate_b"));
    assert!(output.starts_with("Missing Lines (2)"));
    assert!(!output.contains("crate_a"));
    assert!(output.contains("crate_b"));
    assert!(!output.contains("crate_c"));

    let output = format_missing_lines(&records, None);
    assert!(output.starts_with("Missing Lines (5)"));
}

#[test]
fn test_format_missing_lines_when_grep_has_no_matches() {
    let records = vec![RecordResult {
        lines: vec![],
        source_path: Some(PathBuf::from("/project/crates/crate_a/src/lib.rs")),
        missing_lines: vec![10, 20],
    }];

    assert_eq!(
        format_missing_lines(&records, Some("missing_crate")),
        FULL_COVERAGE_MESSAGE
    );
}

#[test]
fn test_format_missing_lines_when_grep_match_has_no_missing_lines() {
    let records = vec![
        RecordResult {
            lines: vec![],
            source_path: Some(PathBuf::from("/project/crates/crate_a/src/lib.rs")),
            missing_lines: vec![],
        },
        RecordResult {
            lines: vec![],
            source_path: Some(PathBuf::from("/project/crates/crate_b/src/lib.rs")),
            missing_lines: vec![30, 40],
        },
    ];

    assert_eq!(
        format_missing_lines(&records, Some("crate_a")),
        FULL_COVERAGE_MESSAGE
    );
}

#[test]
fn test_has_missing_lines_with_grep() {
    let records = vec![RecordResult {
        lines: vec![],
        source_path: Some(PathBuf::from("/project/crates/crate_a/src/lib.rs")),
        missing_lines: vec![10, 20],
    }];

    assert!(has_missing_lines(&records, None));
    assert!(has_missing_lines(&records, Some("crate_a")));
    assert!(!has_missing_lines(&records, Some("crate_b")));
}

#[test]
fn test_has_missing_lines_when_source_path_is_none() {
    let records = vec![RecordResult {
        lines: vec![],
        source_path: None,
        missing_lines: vec![10, 20],
    }];

    assert!(!has_missing_lines(&records, None));
}

fn sample_missing_records() -> Vec<RecordResult> {
    vec![
        RecordResult {
            lines: vec![],
            source_path: Some(PathBuf::from("/project/crates/crate_a/src/lib.rs")),
            missing_lines: vec![10, 20],
        },
        RecordResult {
            lines: vec![],
            source_path: Some(PathBuf::from("/project/crates/crate_b/src/lib.rs")),
            missing_lines: vec![30, 40],
        },
        RecordResult {
            lines: vec![],
            source_path: Some(PathBuf::from("/project/crates/crate_c/src/lib.rs")),
            missing_lines: vec![50],
        },
    ]
}
