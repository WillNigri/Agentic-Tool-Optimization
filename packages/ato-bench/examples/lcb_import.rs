// Dev/usage example: import a LiveCodeBench JSONL file and print the summary.
//
//   cargo run --example lcb_import -- <path-to-test.jsonl>
//
// Fetch a pinned file first, e.g.:
//   curl -sL https://huggingface.co/datasets/livecodebench/code_generation_lite/\
//     resolve/<revision>/test.jsonl -o test.jsonl
//
// This crate never vendors the corpus; you fetch-and-pin it yourself.

use ato_bench::{import_lcb_jsonl, LcbImportOptions};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: lcb_import <path-to-jsonl>");
    let bytes = std::fs::read(&path).expect("read jsonl");
    let r = import_lcb_jsonl(&bytes, &LcbImportOptions::default());
    println!(
        "records={} imported={} excluded_functional={} decode_errors={}",
        r.total_records,
        r.problems.len(),
        r.excluded_functional,
        r.decode_errors.len()
    );
    if let Some(p) = r.problems.first() {
        println!(
            "first imported: id={} tests={} release_date={:?}",
            p.id,
            p.tests.len(),
            p.release_date
        );
    }
    for (id, e) in r.decode_errors.iter().take(3) {
        println!("  decode_error [{id}]: {e}");
    }
}
