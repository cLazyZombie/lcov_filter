use std::collections::{HashMap, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub mod cli;

pub const FULL_COVERAGE_MESSAGE: &str = "100% coverage\n";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Markers {
    pub line: String,
    pub start: String,
    pub stop: String,
}

impl Default for Markers {
    fn default() -> Self {
        Self {
            line: "LCOV_EXCL_LINE".to_string(),
            start: "LCOV_EXCL_START".to_string(),
            stop: "LCOV_EXCL_STOP".to_string(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputMode {
    Lcov,
    Text,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordResult {
    pub lines: Vec<String>,
    pub source_path: Option<PathBuf>,
    pub missing_lines: Vec<usize>,
}

fn read_to_string_lossy(path: &Path) -> io::Result<String> {
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(_) => {
            let bytes = fs::read(path)?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        }
    }
}

pub fn compute_excluded_lines(path: &Path, markers: &Markers) -> io::Result<HashSet<usize>> {
    let content = read_to_string_lossy(path)?;
    Ok(compute_excluded_lines_from_content(&content, markers))
}

pub fn compute_excluded_lines_from_content(content: &str, markers: &Markers) -> HashSet<usize> {
    let mut excluded = HashSet::new();
    let mut in_block = false;

    for (idx, line) in content.lines().enumerate() {
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

fn split_records(input: &str) -> Vec<Vec<String>> {
    let mut records = Vec::new();
    let mut current = Vec::new();

    for line in input.lines() {
        if line == "end_of_record" {
            current.push(line.to_string());
            records.push(current);
            current = Vec::new();
        } else {
            current.push(line.to_string());
        }
    }

    if !current.is_empty() {
        current.push("end_of_record".to_string());
        records.push(current);
    }

    records
}

fn parse_fn_line(line: &str) -> Option<(usize, &str)> {
    let rest = line.strip_prefix("FN:")?;
    let mut parts = rest.splitn(2, ',');
    let line_no = parts.next()?.parse().ok()?;
    let name = parts.next()?;
    Some((line_no, name))
}

fn parse_fnda_line(line: &str) -> Option<(u64, &str)> {
    let rest = line.strip_prefix("FNDA:")?;
    let mut parts = rest.splitn(2, ',');
    let hits = parts.next()?.parse().ok()?;
    let name = parts.next()?;
    Some((hits, name))
}

fn parse_da_line(line: &str) -> Option<(usize, u64)> {
    let rest = line.strip_prefix("DA:")?;
    let mut parts = rest.split(',');
    let line_no = parts.next()?.parse().ok()?;
    let hits = parts.next()?.parse().ok()?;
    Some((line_no, hits))
}

fn parse_brda_line(line: &str) -> Option<(usize, &str, &str, &str)> {
    let rest = line.strip_prefix("BRDA:")?;
    let mut parts = rest.splitn(4, ',');
    let line_no = parts.next()?.parse().ok()?;
    let block = parts.next()?;
    let branch = parts.next()?;
    let taken = parts.next()?;
    Some((line_no, block, branch, taken))
}

fn is_summary_line(line: &str) -> bool {
    line.starts_with("LF:")
        || line.starts_with("LH:")
        || line.starts_with("BRF:")
        || line.starts_with("BRH:")
        || line.starts_with("FNF:")
        || line.starts_with("FNH:")
}

fn process_unreadable_source_record(record: &[String], source_path: PathBuf) -> RecordResult {
    eprintln!(
        "warning: could not read source file: {}",
        source_path.display()
    );
    let mut missing_lines = Vec::new();
    for line in record {
        if let Some((line_no, hits)) = parse_da_line(line)
            && hits == 0
        {
            missing_lines.push(line_no);
        }
    }
    RecordResult {
        lines: record.to_vec(),
        source_path: Some(source_path),
        missing_lines,
    }
}

fn function_entries(
    record: &[String],
    excluded_lines: &HashSet<usize>,
) -> (Vec<(usize, String)>, bool, bool) {
    let mut entries = Vec::new();
    let mut had_fn = false;
    let mut had_fn_summary = false;

    for line in record {
        if line.starts_with("FN:") {
            had_fn = true;
            if let Some((line_no, name)) = parse_fn_line(line)
                && !excluded_lines.contains(&line_no)
            {
                entries.push((line_no, name.to_string()));
            }
        } else if line.starts_with("FNF:") || line.starts_with("FNH:") {
            had_fn_summary = true;
        }
    }

    (entries, had_fn, had_fn_summary)
}

fn process_record(
    record: &[String],
    excluded_cache: &mut HashMap<PathBuf, Option<HashSet<usize>>>,
    markers: &Markers,
) -> RecordResult {
    let sf_line = record.iter().find(|line| line.starts_with("SF:"));
    let Some(sf_line) = sf_line else {
        return RecordResult {
            lines: record.to_vec(),
            source_path: None,
            missing_lines: Vec::new(),
        };
    };

    let source_path = PathBuf::from(sf_line.trim_start_matches("SF:"));
    let excluded = excluded_cache
        .entry(source_path.clone())
        .or_insert_with(|| compute_excluded_lines(&source_path, markers).ok());
    let Some(excluded_lines) = excluded else {
        return process_unreadable_source_record(record, source_path);
    };

    let (fn_entries, mut had_fn, mut had_fn_summary) = function_entries(record, excluded_lines);
    let allowed_fn_names: HashSet<&str> =
        fn_entries.iter().map(|(_, name)| name.as_str()).collect();
    let mut had_fnda = false;
    let mut had_da = false;
    let mut had_brda = false;
    let mut had_line_summary = false;
    let mut had_branch_summary = false;

    let mut out = Vec::new();
    let mut da_lines = Vec::new();
    let mut brda_lines = Vec::new();
    let mut fnda_lines = Vec::new();
    let mut missing_lines = Vec::new();

    for line in record {
        if line == "end_of_record" {
            continue;
        }
        if is_summary_line(line) {
            had_line_summary |= line.starts_with("LF:") || line.starts_with("LH:");
            had_branch_summary |= line.starts_with("BRF:") || line.starts_with("BRH:");
            had_fn_summary |= line.starts_with("FNF:") || line.starts_with("FNH:");
            continue;
        }
        if line.starts_with("FN:") {
            had_fn = true;
            push_allowed_fn_line(line, &allowed_fn_names, &mut out);
            continue;
        }
        if line.starts_with("FNDA:") {
            had_fnda = true;
            push_allowed_fnda_line(line, &allowed_fn_names, &mut out, &mut fnda_lines);
            continue;
        }
        if line.starts_with("DA:") {
            had_da = true;
            push_allowed_da_line(
                line,
                excluded_lines,
                &mut out,
                &mut da_lines,
                &mut missing_lines,
            );
            continue;
        }
        if line.starts_with("BRDA:") {
            had_brda = true;
            push_allowed_brda_line(line, excluded_lines, &mut out, &mut brda_lines);
            continue;
        }
        out.push(line.clone());
    }

    append_summaries(
        &mut out,
        SummaryInput {
            had_fn,
            had_fnda,
            had_da,
            had_brda,
            had_fn_summary,
            had_line_summary,
            had_branch_summary,
            fn_entries: &fn_entries,
            fnda_lines: &fnda_lines,
            da_lines: &da_lines,
            brda_lines: &brda_lines,
        },
    );
    out.push("end_of_record".to_string());

    RecordResult {
        lines: out,
        source_path: Some(source_path),
        missing_lines,
    }
}

fn push_allowed_fn_line(line: &str, allowed_fn_names: &HashSet<&str>, out: &mut Vec<String>) {
    if let Some((_, name)) = parse_fn_line(line) {
        if allowed_fn_names.contains(name) {
            out.push(line.to_string());
        }
    } else {
        out.push(line.to_string());
    }
}

fn push_allowed_fnda_line(
    line: &str,
    allowed_fn_names: &HashSet<&str>,
    out: &mut Vec<String>,
    fnda_lines: &mut Vec<String>,
) {
    if let Some((_, name)) = parse_fnda_line(line) {
        if allowed_fn_names.contains(name) {
            out.push(line.to_string());
            fnda_lines.push(line.to_string());
        }
    } else {
        out.push(line.to_string());
    }
}

fn push_allowed_da_line(
    line: &str,
    excluded_lines: &HashSet<usize>,
    out: &mut Vec<String>,
    da_lines: &mut Vec<String>,
    missing_lines: &mut Vec<usize>,
) {
    if let Some((line_no, hits)) = parse_da_line(line) {
        if !excluded_lines.contains(&line_no) {
            out.push(line.to_string());
            da_lines.push(line.to_string());
            if hits == 0 {
                missing_lines.push(line_no);
            }
        }
    } else {
        out.push(line.to_string());
    }
}

fn push_allowed_brda_line(
    line: &str,
    excluded_lines: &HashSet<usize>,
    out: &mut Vec<String>,
    brda_lines: &mut Vec<String>,
) {
    if let Some((line_no, _, _, _)) = parse_brda_line(line) {
        if !excluded_lines.contains(&line_no) {
            out.push(line.to_string());
            brda_lines.push(line.to_string());
        }
    } else {
        out.push(line.to_string());
    }
}

struct SummaryInput<'a> {
    had_fn: bool,
    had_fnda: bool,
    had_da: bool,
    had_brda: bool,
    had_fn_summary: bool,
    had_line_summary: bool,
    had_branch_summary: bool,
    fn_entries: &'a [(usize, String)],
    fnda_lines: &'a [String],
    da_lines: &'a [String],
    brda_lines: &'a [String],
}

fn append_summaries(out: &mut Vec<String>, input: SummaryInput<'_>) {
    let lf = input.da_lines.len();
    let lh = input
        .da_lines
        .iter()
        .filter(|line| {
            parse_da_line(line)
                .map(|(_, hits)| hits > 0)
                .unwrap_or(false)
        })
        .count();
    let brf = input.brda_lines.len();
    let brh = input
        .brda_lines
        .iter()
        .filter(|line| {
            parse_brda_line(line)
                .map(|(_, _, _, taken)| taken != "-" && taken.parse::<u64>().unwrap_or(0) > 0)
                .unwrap_or(false)
        })
        .count();
    let fnf = input.fn_entries.len();
    let fnh = input
        .fnda_lines
        .iter()
        .filter(|line| {
            parse_fnda_line(line)
                .map(|(hits, _)| hits > 0)
                .unwrap_or(false)
        })
        .count();

    if input.had_fn || input.had_fnda || input.had_fn_summary || fnf > 0 {
        out.push(format!("FNF:{fnf}"));
        out.push(format!("FNH:{fnh}"));
    }
    if input.had_brda || input.had_branch_summary || brf > 0 {
        out.push(format!("BRF:{brf}"));
        out.push(format!("BRH:{brh}"));
    }
    if input.had_da || input.had_line_summary || lf > 0 {
        out.push(format!("LF:{lf}"));
        out.push(format!("LH:{lh}"));
    }
}

pub fn filter_lcov_records(input: &str, markers: &Markers) -> Vec<RecordResult> {
    let records = split_records(input);
    let mut excluded_cache = HashMap::new();

    records
        .into_iter()
        .map(|record| process_record(&record, &mut excluded_cache, markers))
        .collect()
}

pub fn filter_lcov(input: &str, markers: &Markers) -> String {
    let records = filter_lcov_records(input, markers);
    let mut lines = Vec::new();
    for record in records {
        lines.extend(record.lines);
    }
    lines.join("\n") + "\n"
}

pub fn format_missing_lines(records: &[RecordResult], grep: Option<&str>) -> String {
    let mut out = Vec::new();
    let mut total_missing = 0usize;

    for record in records {
        let Some(path) = record.source_path.as_ref() else {
            continue;
        };
        if let Some(pattern) = grep
            && !path.to_string_lossy().contains(pattern)
        {
            continue;
        }
        if record.missing_lines.is_empty() {
            continue;
        }
        total_missing += record.missing_lines.len();
        let mut lines = record.missing_lines.clone();
        lines.sort_unstable();
        let joined = lines
            .iter()
            .map(|line| line.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        out.push(format!("{}: {}", path.display(), joined));
    }

    if out.is_empty() {
        FULL_COVERAGE_MESSAGE.to_string()
    } else {
        let mut lines = Vec::with_capacity(out.len() + 1);
        lines.push(format!("Missing Lines ({total_missing})"));
        lines.extend(out);
        lines.join("\n") + "\n"
    }
}

pub fn has_missing_lines(records: &[RecordResult], grep: Option<&str>) -> bool {
    records.iter().any(|record| {
        let Some(path) = record.source_path.as_ref() else {
            return false;
        };
        if let Some(pattern) = grep
            && !path.to_string_lossy().contains(pattern)
        {
            return false;
        }
        !record.missing_lines.is_empty()
    })
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
