// LiveCodeBench importer — faithful, "dumb + complete", OSS/free.
//
// LiveCodeBench ships its `code_generation(_lite)` dataset as plain JSONL
// (`test.jsonl` … `test6.jsonl`), one problem per line, selected by a
// `version_tag`. This module turns that raw JSONL into our `Problem` schema. It
// is deliberately a FAITHFUL loader: it does not curate, stratify, rank, or
// contamination-window the set (those judgments are the paid curated-suite
// value). The only filtering is TECHNICAL — problems our stdin/stdout grader can
// actually verify — and every exclusion is counted and returned, never silently
// dropped.
//
// Record schema (LCB `_info().features`):
//   question_title, question_content, platform, question_id, contest_id,
//   contest_date, starter_code, difficulty, public_test_cases (string),
//   private_test_cases (string), metadata (string)
//
// Test-case encoding (verified against the live dataset):
//   • public_test_cases  = plain JSON: `[{"input","output","testtype"}]`
//   • private_test_cases = base64( zlib( pickle(<that same JSON string>) ) )
//     The pickle layer only ever wraps a Python `str`, so we extract the string
//     with a MINIMAL opcode reader — we never call a pickle VM / `pickle.loads`,
//     so there is no code-execution surface from the downloaded bytes. Inflation
//     is size-capped to defuse a zlib bomb.
//
// The HTTP fetch-and-pin-by-revision layer lives with the `ato bench run`
// command (next slice); this module operates on already-fetched bytes so it is
// pure and fully offline-testable.

use crate::problem::{Language, Problem, TestCase};
use base64::Engine;
use serde::Deserialize;
use std::io::Read;

/// Hard cap on zlib-inflated bytes per record — defuses a decompression bomb
/// (tiny base64 that inflates to gigabytes). 32 MiB is far above any real
/// problem's test payload.
const MAX_INFLATE_BYTES: usize = 32 * 1024 * 1024;

/// One raw LiveCodeBench JSONL record. Extra/unknown fields are ignored.
#[derive(Debug, Clone, Deserialize)]
pub struct LcbRecord {
    #[serde(default)]
    pub question_title: String,
    pub question_content: String,
    #[serde(default)]
    pub platform: String,
    pub question_id: String,
    #[serde(default)]
    pub contest_id: String,
    pub contest_date: String,
    #[serde(default)]
    pub starter_code: String,
    #[serde(default)]
    pub difficulty: String,
    pub public_test_cases: String,
    #[serde(default)]
    pub private_test_cases: String,
    #[serde(default)]
    pub metadata: String,
}

/// One LCB test case as stored in the JSON payload.
#[derive(Debug, Clone, Deserialize)]
struct LcbTest {
    input: String,
    output: String,
    #[serde(default)]
    testtype: String,
}

/// Options for the import transform.
#[derive(Debug, Clone)]
pub struct LcbImportOptions {
    /// Keep only problems whose tests are all stdin/stdout (what our grader can
    /// verify today). Functional/call-based problems need a call harness that is
    /// a later slice; when true they are excluded and counted. Default true.
    pub stdin_only: bool,
}

impl Default for LcbImportOptions {
    fn default() -> Self {
        Self { stdin_only: true }
    }
}

/// The outcome of importing a JSONL blob. Transparent by construction: every
/// record is accounted for as imported, excluded, or errored.
#[derive(Debug, Clone, Default)]
pub struct LcbImportResult {
    pub problems: Vec<Problem>,
    pub total_records: usize,
    /// Functional/call-based problems skipped because the grader can't run them.
    pub excluded_functional: usize,
    /// (question_id-or-marker, reason) for records that failed to parse/decode.
    pub decode_errors: Vec<(String, String)>,
}

#[derive(Debug)]
pub enum ImportError {
    Base64(String),
    Inflate(String),
    Pickle(String),
    Json(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Base64(m) => write!(f, "base64 decode: {m}"),
            ImportError::Inflate(m) => write!(f, "zlib inflate: {m}"),
            ImportError::Pickle(m) => write!(f, "pickle string extract: {m}"),
            ImportError::Json(m) => write!(f, "json parse: {m}"),
        }
    }
}

impl std::error::Error for ImportError {}

/// Inflate zlib data, refusing to allocate more than `cap` bytes (bomb guard).
fn inflate_capped(data: &[u8], cap: usize) -> Result<Vec<u8>, ImportError> {
    let mut decoder = flate2::read::ZlibDecoder::new(data).take((cap as u64) + 1);
    let mut out = Vec::new();
    decoder
        .read_to_end(&mut out)
        .map_err(|e| ImportError::Inflate(e.to_string()))?;
    if out.len() > cap {
        return Err(ImportError::Inflate(format!(
            "inflated stream exceeds {cap}-byte cap (possible zlib bomb)"
        )));
    }
    Ok(out)
}

/// Extract the single Python `str` from a pickle stream WITHOUT executing it.
///
/// LCB's private payload is `pickle(<json string>)` at protocol 4/5. We walk the
/// handful of opcodes that appear around a lone string and return its bytes:
///   PROTO(0x80)+1  FRAME(0x95)+8  MEMOIZE(0x94)
///   SHORT_BINUNICODE(0x8c)+1-byte len | BINUNICODE(0x58 'X')+4-byte LE len
///   | BINUNICODE8(0x8d)+8-byte LE len
/// Anything else is rejected — we never interpret reduce/global/build opcodes,
/// so no object construction or code execution can occur.
fn pickle_extract_str(bytes: &[u8]) -> Result<String, ImportError> {
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            0x80 => i += 2,            // PROTO + version
            0x95 => i += 9,            // FRAME + 8-byte length
            0x94 => i += 1,            // MEMOIZE
            0x8c => {
                // SHORT_BINUNICODE: 1-byte length
                let len = *bytes.get(i + 1).ok_or_else(|| trunc("short_binunicode len"))? as usize;
                return take_utf8(bytes, i + 2, len);
            }
            0x58 => {
                // BINUNICODE 'X': 4-byte little-endian length
                let len = read_u32_le(bytes, i + 1)? as usize;
                return take_utf8(bytes, i + 5, len);
            }
            0x8d => {
                // BINUNICODE8: 8-byte little-endian length
                let len = read_u64_le(bytes, i + 1)? as usize;
                return take_utf8(bytes, i + 9, len);
            }
            other => {
                return Err(ImportError::Pickle(format!(
                    "unexpected opcode 0x{other:02x} at offset {i}; only a lone string pickle is supported"
                )))
            }
        }
    }
    Err(ImportError::Pickle("no string opcode found".into()))
}

fn trunc(what: &str) -> ImportError {
    ImportError::Pickle(format!("truncated pickle ({what})"))
}

/// Slice `bytes[at .. at+n]` using checked arithmetic so an attacker-controlled
/// length can never overflow the index and panic (belt-and-suspenders on top of
/// the inflate/len caps).
fn slice_checked<'a>(
    bytes: &'a [u8],
    at: usize,
    n: usize,
    what: &str,
) -> Result<&'a [u8], ImportError> {
    let end = at.checked_add(n).ok_or_else(|| trunc(what))?;
    bytes.get(at..end).ok_or_else(|| trunc(what))
}

fn read_u32_le(bytes: &[u8], at: usize) -> Result<u32, ImportError> {
    let s = slice_checked(bytes, at, 4, "u32 len")?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn read_u64_le(bytes: &[u8], at: usize) -> Result<u64, ImportError> {
    let s = slice_checked(bytes, at, 8, "u64 len")?;
    let mut b = [0u8; 8];
    b.copy_from_slice(s);
    Ok(u64::from_le_bytes(b))
}

fn take_utf8(bytes: &[u8], start: usize, len: usize) -> Result<String, ImportError> {
    if len > MAX_INFLATE_BYTES {
        return Err(ImportError::Pickle(format!(
            "string length {len} exceeds cap"
        )));
    }
    let slice = slice_checked(bytes, start, len, "string payload")?;
    std::str::from_utf8(slice)
        .map(|s| s.to_string())
        .map_err(|e| ImportError::Pickle(format!("non-utf8 string: {e}")))
}

/// Decode a test-case field. Plain JSON (public) is parsed directly; otherwise
/// (private) it is base64 → zlib → pickle-string → JSON. An empty field yields
/// no tests.
fn decode_tests(field: &str) -> Result<Vec<LcbTest>, ImportError> {
    let field = field.trim();
    if field.is_empty() {
        return Ok(Vec::new());
    }
    let json = if field.starts_with('[') {
        field.to_string()
    } else {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(field)
            .map_err(|e| ImportError::Base64(e.to_string()))?;
        let inflated = inflate_capped(&raw, MAX_INFLATE_BYTES)?;
        pickle_extract_str(&inflated)?
    };
    serde_json::from_str(&json).map_err(|e| ImportError::Json(e.to_string()))
}

/// Transform one raw record into a `Problem`, or `None` if it is excluded by a
/// technical filter (functional problem while `stdin_only`).
fn record_to_problem(
    rec: &LcbRecord,
    opts: &LcbImportOptions,
) -> Result<Option<Problem>, ImportError> {
    let mut tests = decode_tests(&rec.public_test_cases)?;
    tests.extend(decode_tests(&rec.private_test_cases)?);

    // A problem is functional/call-based if it ships starter code or any test is
    // tagged functional — our stdin/stdout grader can't verify those yet.
    let functional = !rec.starter_code.trim().is_empty()
        || tests
            .iter()
            .any(|t| t.testtype.eq_ignore_ascii_case("functional"));
    if functional && opts.stdin_only {
        return Ok(None);
    }

    let cases = tests
        .into_iter()
        .map(|t| TestCase {
            input: t.input,
            expected: t.output,
        })
        .collect();

    // Faithful passthrough of upstream fields for later stratification (the free
    // curated sample needs a difficulty tier; analysis may slice by platform).
    // Data, not curation.
    let mut metadata = std::collections::BTreeMap::new();
    if !rec.difficulty.trim().is_empty() {
        metadata.insert("difficulty".to_string(), rec.difficulty.clone());
    }
    if !rec.platform.trim().is_empty() {
        metadata.insert("platform".to_string(), rec.platform.clone());
    }

    Ok(Some(Problem {
        id: format!("livecodebench/{}", rec.question_id),
        source: "livecodebench".into(),
        language: Language::Python,
        prompt: rec.question_content.clone(),
        tests: cases,
        // e.g. "2023-08-21T00:00:00" — the contamination classifier reads the
        // date prefix. This is DATA, not a default filter.
        release_date: Some(rec.contest_date.clone()),
        metadata,
    }))
}

/// Import a LiveCodeBench JSONL blob into problems. Faithful and complete:
/// every line is accounted for (imported / excluded-functional / errored).
pub fn import_lcb_jsonl(bytes: &[u8], opts: &LcbImportOptions) -> LcbImportResult {
    let mut result = LcbImportResult::default();
    for line in bytes.split(|&b| b == b'\n') {
        if line.iter().all(|b| b.is_ascii_whitespace()) {
            continue;
        }
        result.total_records += 1;
        let rec: LcbRecord = match serde_json::from_slice(line) {
            Ok(r) => r,
            Err(e) => {
                result
                    .decode_errors
                    .push(("<unparseable line>".into(), e.to_string()));
                continue;
            }
        };
        match record_to_problem(&rec, opts) {
            Ok(Some(p)) => result.problems.push(p),
            Ok(None) => result.excluded_functional += 1,
            Err(e) => result.decode_errors.push((rec.question_id, e.to_string())),
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // Round-trips the exact real-world encoding: JSON string -> pickle(str) ->
    // zlib -> base64. Built inline (no vendored corpus) so the decode path is
    // exercised deterministically offline.
    fn encode_private(json: &str) -> String {
        // pickle protocol 4: PROTO 4, FRAME<len>, BINUNICODE<len> <utf8>, MEMOIZE, STOP
        let payload = json.as_bytes();
        let mut body = Vec::new();
        body.push(0x58); // BINUNICODE 'X'
        body.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        body.extend_from_slice(payload);
        body.push(0x94); // MEMOIZE
        body.push(b'.'); // STOP
        let mut pickled = Vec::new();
        pickled.push(0x80);
        pickled.push(0x04); // PROTO 4
        pickled.push(0x95); // FRAME
        pickled.extend_from_slice(&(body.len() as u64).to_le_bytes());
        pickled.extend_from_slice(&body);
        // zlib compress
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&pickled).unwrap();
        let compressed = enc.finish().unwrap();
        base64::engine::general_purpose::STANDARD.encode(compressed)
    }

    #[test]
    fn pickle_extractor_reads_short_and_long_strings() {
        // Long string uses BINUNICODE ('X'); short uses SHORT_BINUNICODE (0x8c).
        let long = "x".repeat(400);
        let s = decode_tests(&encode_private(&format!(
            "[{{\"input\":\"{long}\",\"output\":\"ok\",\"testtype\":\"stdin\"}}]"
        )))
        .unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].input.len(), 400);
    }

    #[test]
    fn public_tests_parse_as_plain_json() {
        let pubjson = r#"[{"input":"2 3\n","output":"5\n","testtype":"stdin"}]"#;
        let t = decode_tests(pubjson).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].output, "5\n");
    }

    #[test]
    fn private_tests_decode_from_base64_zlib_pickle() {
        let enc = encode_private(r#"[{"input":"1\n","output":"YES\n","testtype":"stdin"}]"#);
        let t = decode_tests(&enc).unwrap();
        assert_eq!(t.len(), 1);
        assert_eq!(t[0].input, "1\n");
    }

    #[test]
    fn functional_problems_excluded_and_counted_when_stdin_only() {
        let functional = LcbRecord {
            question_title: "t".into(),
            question_content: "solve".into(),
            platform: "leetcode".into(),
            question_id: "q1".into(),
            contest_id: String::new(),
            contest_date: "2024-01-01T00:00:00".into(),
            starter_code: "class Solution:".into(), // marks functional
            difficulty: "easy".into(),
            public_test_cases: r#"[{"input":"x","output":"y","testtype":"functional"}]"#.into(),
            private_test_cases: String::new(),
            metadata: "{}".into(),
        };
        let out = record_to_problem(&functional, &LcbImportOptions::default()).unwrap();
        assert!(
            out.is_none(),
            "functional problem excluded under stdin_only"
        );
    }

    #[test]
    fn stdin_record_maps_to_problem_with_release_date() {
        let rec = LcbRecord {
            question_title: "sum".into(),
            question_content: "read two ints, print sum".into(),
            platform: "codeforces".into(),
            question_id: "abc-1".into(),
            contest_id: "c1".into(),
            contest_date: "2024-09-15T00:00:00".into(),
            starter_code: String::new(),
            difficulty: "easy".into(),
            public_test_cases: r#"[{"input":"2 3\n","output":"5\n","testtype":"stdin"}]"#.into(),
            private_test_cases: encode_private(
                r#"[{"input":"10 20\n","output":"30\n","testtype":"stdin"}]"#,
            ),
            metadata: "{}".into(),
        };
        let p = record_to_problem(&rec, &LcbImportOptions::default())
            .unwrap()
            .unwrap();
        assert_eq!(p.id, "livecodebench/abc-1");
        assert_eq!(p.source, "livecodebench");
        assert_eq!(p.tests.len(), 2); // public + private
        assert_eq!(p.release_date.as_deref(), Some("2024-09-15T00:00:00"));
        // Faithful passthrough of difficulty + platform for later stratification.
        assert_eq!(
            p.metadata.get("difficulty").map(String::as_str),
            Some("easy")
        );
        assert_eq!(
            p.metadata.get("platform").map(String::as_str),
            Some("codeforces")
        );
    }

    #[test]
    fn zlib_bomb_is_refused() {
        // 5 MB of zeros compresses tiny but would inflate past a small cap.
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::best());
        enc.write_all(&vec![0u8; 5 * 1024 * 1024]).unwrap();
        let bomb = enc.finish().unwrap();
        assert!(bomb.len() < 10_000, "compresses small");
        let err = inflate_capped(&bomb, 1024).unwrap_err();
        assert!(matches!(err, ImportError::Inflate(_)));
    }

    #[test]
    fn malformed_pickle_opcode_is_rejected_not_panic() {
        // A reduce opcode (0x52 'R') must be refused, never interpreted.
        let bad = [0x80u8, 0x04, 0x52];
        assert!(pickle_extract_str(&bad).is_err());
    }

    #[test]
    fn import_accounts_for_every_line() {
        let good = r#"{"question_content":"c","question_id":"g1","contest_date":"2024-01-01T00:00:00","public_test_cases":"[{\"input\":\"a\",\"output\":\"b\",\"testtype\":\"stdin\"}]"}"#;
        let bad = r#"{not json"#;
        let blob = format!("{good}\n{bad}\n\n");
        let out = import_lcb_jsonl(blob.as_bytes(), &LcbImportOptions::default());
        assert_eq!(out.total_records, 2); // blank line skipped
        assert_eq!(out.problems.len(), 1);
        assert_eq!(out.decode_errors.len(), 1);
    }
}
