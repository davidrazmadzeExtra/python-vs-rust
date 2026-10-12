//! Rust port of `analyze_logs.py`.
//!
//! Streams a server log line by line and reports request counts by status
//! class, average response time and the five most requested endpoints. The
//! output format is byte-for-byte identical to the Python version.

use std::collections::HashMap;
use std::env;
use std::fmt;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

const DEFAULT_LOG: &str = "data/server.log";
const TOP_N: usize = 5;

#[derive(Debug, PartialEq)]
struct Results {
    total: u64,
    success: u64,
    client_errors: u64,
    server_errors: u64,
    average_ms: f64,
    top_endpoints: Vec<(String, u64)>,
}

#[derive(Debug)]
enum AnalyzeError {
    Io(io::Error),
    InvalidUtf8 { line: u64 },
    InvalidInt { line: u64, literal: String },
    Overflow { line: u64 },
}

impl fmt::Display for AnalyzeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "{e}"),
            Self::InvalidUtf8 { line } => write!(f, "line {line}: invalid UTF-8"),
            Self::InvalidInt { line, literal } => write!(
                f,
                "line {line}: invalid literal for int() with base 10: '{literal}'"
            ),
            Self::Overflow { line } => write!(f, "line {line}: integer overflow"),
        }
    }
}

impl std::error::Error for AnalyzeError {}

impl From<io::Error> for AnalyzeError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Equivalent of `collections.Counter` restricted to what the analyzer needs:
/// counts keyed by string, remembering first-insertion order so that ties in
/// `most_common` resolve exactly like Python's.
#[derive(Default)]
struct Counter {
    index: HashMap<String, usize>,
    entries: Vec<(String, u64)>,
}

impl Counter {
    fn increment(&mut self, key: &str) {
        match self.index.get(key) {
            Some(&i) => self.entries[i].1 += 1,
            None => {
                self.index.insert(key.to_owned(), self.entries.len());
                self.entries.push((key.to_owned(), 1));
            }
        }
    }

    /// Highest counts first; equal counts keep insertion order (stable sort),
    /// matching `Counter.most_common(n)`.
    fn most_common(&self, n: usize) -> Vec<(String, u64)> {
        let mut sorted = self.entries.clone();
        sorted.sort_by_key(|&(_, count)| std::cmp::Reverse(count));
        sorted.truncate(n);
        sorted
    }
}

/// Whitespace as understood by Python's `str.split()`: Unicode White_Space
/// plus the ASCII information separators U+001C..U+001F.
fn is_py_whitespace(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// Splits like `line.split()` and returns the fields only if there are exactly six.
fn split_six_fields(line: &str) -> Option<[&str; 6]> {
    let mut parts = line.split(is_py_whitespace).filter(|s| !s.is_empty());
    let mut fields = [""; 6];
    for field in &mut fields {
        *field = parts.next()?;
    }
    parts.next().is_none().then_some(fields)
}

/// Parses a decimal literal the way Python's `int()` does for ASCII input:
/// optional sign, digits, and single underscores between digits.
fn parse_py_int(s: &str) -> Option<i64> {
    let (negative, digits) = match s.as_bytes().first()? {
        b'+' => (false, &s[1..]),
        b'-' => (true, &s[1..]),
        _ => (false, s),
    };
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
    {
        return None;
    }
    let mut value: i64 = 0;
    for b in digits.bytes() {
        match b {
            b'_' => {}
            b'0'..=b'9' => {
                value = value.checked_mul(10)?.checked_add(i64::from(b - b'0'))?;
            }
            _ => return None,
        }
    }
    Some(if negative { -value } else { value })
}

fn analyze_logs(path: &Path) -> Result<Results, AnalyzeError> {
    let file = File::open(path)?;
    analyze_reader(BufReader::with_capacity(64 * 1024, file))
}

fn analyze_reader<R: BufRead>(mut reader: R) -> Result<Results, AnalyzeError> {
    let mut total: u64 = 0;
    let mut success: u64 = 0;
    let mut client_errors: u64 = 0;
    let mut server_errors: u64 = 0;
    let mut total_duration: i64 = 0;
    let mut endpoints = Counter::default();

    // One reusable buffer: memory use is bounded by the longest line,
    // not by the file size.
    let mut buf = Vec::new();
    let mut line_no: u64 = 0;

    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf)? == 0 {
            break;
        }
        line_no += 1;
        let text =
            std::str::from_utf8(&buf).map_err(|_| AnalyzeError::InvalidUtf8 { line: line_no })?;

        // Python's text mode uses universal newlines, so a lone '\r' also
        // ends a line. Splitting on it reproduces that.
        for line in text.split('\r') {
            let Some([_timestamp, _ip, _method, path, status, duration]) = split_six_fields(line)
            else {
                continue;
            };

            let invalid = |literal: &str| AnalyzeError::InvalidInt {
                line: line_no,
                literal: literal.to_owned(),
            };
            let status = parse_py_int(status).ok_or_else(|| invalid(status))?;
            let duration_str = duration.strip_suffix("ms").unwrap_or(duration);
            let duration = parse_py_int(duration_str).ok_or_else(|| invalid(duration_str))?;

            total += 1;
            total_duration = total_duration
                .checked_add(duration)
                .ok_or(AnalyzeError::Overflow { line: line_no })?;
            endpoints.increment(path);

            match status {
                200..300 => success += 1,
                400..500 => client_errors += 1,
                500..600 => server_errors += 1,
                _ => {}
            }
        }
    }

    Ok(Results {
        total,
        success,
        client_errors,
        server_errors,
        average_ms: if total > 0 {
            total_duration as f64 / total as f64
        } else {
            0.0
        },
        top_endpoints: endpoints.most_common(TOP_N),
    })
}

/// Formats an integer with comma thousands separators, like Python's `{:,}`.
fn with_commas(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

fn main() -> ExitCode {
    let path = env::args_os()
        .nth(1)
        .map_or_else(|| PathBuf::from(DEFAULT_LOG), PathBuf::from);

    let start = Instant::now();
    let results = match analyze_logs(&path) {
        Ok(results) => results,
        Err(e) => {
            eprintln!("error: {}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };
    let elapsed = start.elapsed().as_secs_f64();

    println!("\n=== Server Log Analysis ===");
    println!("Total requests: {}", with_commas(results.total));
    println!("Successful: {}", with_commas(results.success));
    println!("Client errors: {}", with_commas(results.client_errors));
    println!("Server errors: {}", with_commas(results.server_errors));
    println!("Average response time: {:.2} ms", results.average_ms);

    println!("\nTop endpoints:");
    for (path, count) in &results.top_endpoints {
        println!("  {path}: {}", with_commas(*count));
    }

    println!("\nExecution time: {elapsed:.4} seconds");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analyze_str(s: &str) -> Results {
        analyze_reader(s.as_bytes()).unwrap()
    }

    #[test]
    fn counts_status_classes_and_average() {
        let r = analyze_str(
            "t 1.1.1.1 GET / 200 10ms\n\
             t 1.1.1.1 GET /a 404 20ms\n\
             t 1.1.1.1 GET /a 503 30ms\n\
             t 1.1.1.1 GET /b 302 40ms\n",
        );
        assert_eq!(
            (r.total, r.success, r.client_errors, r.server_errors),
            (4, 1, 1, 1)
        );
        assert_eq!(r.average_ms, 25.0);
    }

    #[test]
    fn skips_lines_without_six_fields() {
        let r = analyze_str(
            "\n   \nonly five fields here ok\nt i GET / 200 1ms extra\nt i GET / 200 7ms\n",
        );
        assert_eq!(r.total, 1);
        assert_eq!(r.average_ms, 7.0);
    }

    #[test]
    fn empty_input_gives_zero_average() {
        let r = analyze_str("");
        assert_eq!(r.total, 0);
        assert_eq!(r.average_ms, 0.0);
        assert!(r.top_endpoints.is_empty());
    }

    #[test]
    fn most_common_breaks_ties_by_first_seen() {
        let r = analyze_str(
            "t i GET /c 200 1ms\nt i GET /a 200 1ms\nt i GET /b 200 1ms\n\
             t i GET /a 200 1ms\nt i GET /c 200 1ms\n",
        );
        let names: Vec<_> = r
            .top_endpoints
            .iter()
            .map(|(p, c)| (p.as_str(), *c))
            .collect();
        assert_eq!(names, [("/c", 2), ("/a", 2), ("/b", 1)]);
    }

    #[test]
    fn whitespace_and_newlines_match_python() {
        // Tabs, CRLF, a lone CR acting as a line break, and U+001F as a separator.
        let r = analyze_str("t\ti GET / 200 1ms\r\nt i GET / 200 2ms\rt i GET\u{1f}/ 200 3ms\n");
        assert_eq!(r.total, 3);
    }

    #[test]
    fn removes_ms_suffix_only_once() {
        assert!(analyze_reader("t i GET / 200 5msms\n".as_bytes()).is_err());
        assert_eq!(analyze_str("t i GET / 200 5\n").average_ms, 5.0);
    }

    #[test]
    fn py_int_rules() {
        assert_eq!(parse_py_int("200"), Some(200));
        assert_eq!(parse_py_int("+7"), Some(7));
        assert_eq!(parse_py_int("-7"), Some(-7));
        assert_eq!(parse_py_int("007"), Some(7));
        assert_eq!(parse_py_int("1_000"), Some(1000));
        for bad in ["", "-", "_1", "1_", "1__0", "1.5", "abc", "0x10"] {
            assert_eq!(parse_py_int(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn invalid_int_is_an_error() {
        assert!(analyze_reader("t i GET / OK 5ms\n".as_bytes()).is_err());
    }

    #[test]
    fn thousands_separators() {
        assert_eq!(with_commas(0), "0");
        assert_eq!(with_commas(999), "999");
        assert_eq!(with_commas(1000), "1,000");
        assert_eq!(with_commas(10_000_000), "10,000,000");
    }
}
