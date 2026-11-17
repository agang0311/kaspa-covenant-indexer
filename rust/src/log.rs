//! JSON-lines operator log on stdout, same shape as the JS build: {"at": ISO, "kind": K, "detail": D}.
use serde_json::{Value, json};
use std::io::Write;

pub fn log(kind: &str, detail: Value) {
    let line = json!({"at": crate::util::iso_now(), "kind": kind, "detail": detail});
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}
