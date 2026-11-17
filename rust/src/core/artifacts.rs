//! Pinned V2.0.2 publication bundle, embedded in the binary and re-verified at load
//! (`scripts/contract/bundle.mjs` inspectBundle/loadBundle + `scripts/core/src/artifacts.ts`).
//! Integrity is necessary but cannot approve a new Profile: the computed id must equal APPROVED_PROFILE_ID.
use super::bytes::{check, hex, unhex, unhex_len};
use super::state::{self as S, Frame, Module, Profile};
use anyhow::{Result, anyhow};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

macro_rules! art {
    ($p:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../artifacts/v202/contracts/",
            $p
        ))
    };
}

pub struct Bundle {
    pub pins: &'static [u8],
    pub profile_json: &'static [u8],
    pub layout: &'static [u8],
    pub build_report: &'static [u8],
    /// [open, sealed, refunding]: (template .sil, linked .sil, linked artifact .json, constructor args .json)
    pub modules: [(&'static [u8], &'static [u8], &'static [u8], &'static [u8]); 3],
}

pub static EMBEDDED: Bundle = Bundle {
    pins: art!("pins.json"),
    profile_json: art!("profile.json"),
    layout: art!("ledger-layout.json"),
    build_report: art!("artifacts/build-report.json"),
    modules: [
        (
            art!("open.sil"),
            art!("artifacts/open-linked.sil"),
            art!("artifacts/open-linked.json"),
            art!("artifacts/open-linked.args.json"),
        ),
        (
            art!("sealed.sil"),
            art!("artifacts/sealed-linked.sil"),
            art!("artifacts/sealed-linked.json"),
            art!("artifacts/sealed-linked.args.json"),
        ),
        (
            art!("refunding.sil"),
            art!("artifacts/refunding-linked.sil"),
            art!("artifacts/refunding-linked.json"),
            art!("artifacts/refunding-linked.args.json"),
        ),
    ],
};

pub const COMPILER_SHA256: &str =
    "81de9aa4157dbde3633ebab629e86c5975770fc13ee2d2093e52d7f725616a00";
pub const BUNDLE_NETWORK_GENESIS: &str =
    "f896a3034873be1739fc4359236899fd3d65d2bc94f9780df0d0da3eb1cc4370";
const ORDER: [Module; 3] = [Module::Refunding, Module::Sealed, Module::Open];

fn deps(m: Module) -> &'static [Module] {
    match m {
        Module::Refunding => &[],
        Module::Sealed => &[Module::Refunding],
        Module::Open => &[Module::Sealed, Module::Refunding],
    }
}

fn markers(m: Module) -> &'static [&'static str] {
    match m {
        Module::Open => &[
            "Kaswin OPEN Covenant V2.0.2",
            "int constant HEADER_BYTES = 80;",
            "int constant RECORD_BYTES = 36;",
            "int constant DEPOSIT = 400000000;",
            "byte[](0x4b573231)",
            "for (fo, 0, tx.outputs.length, 2)",
            "tx.outputs.length <= 2);",
            "require(totalIn - totalOut == fee);",
        ],
        Module::Sealed => &[
            "Kaswin SEALED Covenant V2.0.2",
            "int constant HEADER_BYTES = 80;",
            "int constant RECORD_BYTES = 36;",
            "int constant DEPOSIT = 400000000;",
            "byte[](0x4b573231)",
            "for (fo, 0, tx.outputs.length, 3)",
            "tx.outputs.length <= 3);",
            "require(totalIn - totalOut == fee);",
            "+ byte[](0x0000000000000000));",
        ],
        Module::Refunding => &[
            "Kaswin REFUNDING Covenant V2.0.2",
            "int constant HEADER_BYTES = 80;",
            "int constant RECORD_BYTES = 36;",
            "int constant DEPOSIT = 400000000;",
            "int constant REFUND_FEE = 2000000;",
            "byte[](0x4b573231)",
            "for (fo, 0, tx.outputs.length, 34)",
            "ordinary(a10Batch + 1, externalValue + a10Pool - fee, actorPk);",
            "ordinary(a10Batch, DEPOSIT - REFUND_FEE, ownerKey());",
            "require(totalIn - totalOut == fee);",
        ],
    }
}

pub fn sha256_hex(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

fn contract_name(m: Module) -> &'static str {
    match m {
        Module::Open => "KaswinOpen",
        Module::Sealed => "KaswinSealed",
        Module::Refunding => "KaswinRefunding",
    }
}

fn abi(
    m: Module,
) -> (
    &'static [&'static str],
    &'static [(&'static str, Option<u64>)],
) {
    match m {
        Module::Open => (
            &[
                "action",
                "originTxId",
                "originIndex",
                "programTailBytes",
                "nextTail",
                "data",
                "actorPk",
                "fee",
            ],
            &[
                ("int", None),
                ("fixed_bytes", Some(32)),
                ("int", None),
                ("int", None),
                ("bytes", None),
                ("bytes", None),
                ("pubkey", None),
                ("int", None),
            ],
        ),
        _ => (
            &[
                "action",
                "programTailBytes",
                "nextTail",
                "data",
                "actorPk",
                "fee",
            ],
            &[
                ("int", None),
                ("int", None),
                ("bytes", None),
                ("bytes", None),
                ("pubkey", None),
                ("int", None),
            ],
        ),
    }
}

fn obj<'a>(x: Option<&'a Value>) -> Result<&'a serde_json::Map<String, Value>> {
    x.and_then(|v| v.as_object())
        .ok_or_else(|| anyhow!("ARTIFACT_OBJECT"))
}

fn art_bytes(x: Option<&Value>) -> Result<Vec<u8>> {
    match x {
        Some(Value::String(s)) => unhex(s.strip_prefix("0x").unwrap_or(s)),
        Some(Value::Array(a)) => {
            let mut out = Vec::with_capacity(a.len());
            for b in a {
                let n = b
                    .as_u64()
                    .filter(|n| *n <= 255)
                    .ok_or_else(|| anyhow!("ARTIFACT_BYTES"))?;
                out.push(n as u8);
            }
            Ok(out)
        }
        _ => Err(anyhow!("ARTIFACT_BYTES")),
    }
}

/// Structural parser of a compiled frame; ABI/hash validity is not approval.
pub fn parse_compiled_frame(m: Module, doc: &Value, source_sha256: &str) -> Result<Frame> {
    unhex_len(source_sha256, 32)?;
    let d = obj(Some(doc))?;
    check(
        d.get("schema_version") == Some(&json!(1))
            && d.get("compiler_version") == Some(&json!("0.1.0")),
        "ARTIFACT_VERSION",
    )?;
    let c = obj(obj(d.get("contracts"))?.get(contract_name(m)))?;
    let rt = obj(c.get("runtime_state"))?;
    let fields = rt.get("fields").and_then(|f| f.as_array());
    check(
        fields
            .map(|f| f.len() == 1 && f[0].get("name") == Some(&json!("ledger")))
            .unwrap_or(false),
        "ARTIFACT_STATE",
    )?;
    let a = obj(c.get("compiled"))?;
    let script = art_bytes(a.get("bytecode"))?;
    let span = obj(a.get("state_span"))?;
    let len = span.get("len").and_then(|v| v.as_u64());
    check(
        span.get("offset") == Some(&json!(1)) && len.is_some(),
        "ARTIFACT_FRAME",
    )?;
    let len = len.unwrap() as usize;
    check(
        script.first() == Some(&0x6b) && len > 0 && len + 1 < script.len(),
        "ARTIFACT_FRAME",
    )?;
    let tail = script[1 + len..].to_vec();
    check(tail.first() == Some(&0x6c), "ARTIFACT_FRAME")?;
    let hash = S::template_hash(&tail)?;
    check(
        hash == hex(&art_bytes(a.get("template_hash"))?),
        "ARTIFACT_HASH",
    )?;
    let entry = obj(obj(c.get("entries"))?.get("spend"))?;
    let params = entry.get("params").and_then(|p| p.as_array());
    let (names, types) = abi(m);
    check(
        params
            .map(|p| {
                p.len() == names.len()
                    && p.iter()
                        .zip(names)
                        .all(|(x, n)| x.get("name") == Some(&json!(n)))
            })
            .unwrap_or(false),
        "ENTRY_ABI_CHANGED",
    )?;
    let params = params.unwrap();
    check(
        params.iter().zip(types).all(|(x, (kind, l))| {
            let t = x.get("type");
            t.and_then(|t| t.get("kind")) == Some(&json!(kind))
                && (*kind != "fixed_bytes"
                    || t.and_then(|t| t.get("len")).and_then(|v| v.as_u64()) == *l)
        }),
        "ENTRY_ABI_TYPE_CHANGED",
    )?;
    let tag = entry
        .get("dispatch_tag")
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("ARTIFACT_TAG"))?;
    unhex_len(tag, 4)?;
    Ok(Frame {
        tail,
        template_hash: hash,
        dispatch_tag: tag.to_string(),
        source_sha256: source_sha256.to_string(),
    })
}

fn link_source(m: Module, source: &str, frames: &[Option<Frame>; 3]) -> Result<String> {
    let re = regex::Regex::new(
        r"byte\[32\] constant ([A-Z]+)_TEMPLATE_HASH = byte\[32\]\(0x([0-9a-f]{64})\);",
    )
    .unwrap();
    let names: Vec<String> = re
        .captures_iter(source)
        .map(|c| c[1].to_lowercase())
        .collect();
    let want: Vec<String> = deps(m).iter().map(|d| d.name().to_string()).collect();
    check(names == want, "BUNDLE_UNEXPECTED_FORWARD_DEPENDENCIES")?;
    let mut linked = source.to_string();
    for d in deps(m) {
        let hash = frames[d.idx()]
            .as_ref()
            .map(|f| f.template_hash.clone())
            .unwrap_or_default();
        check(
            hash.len() == 64 && hash != "00".repeat(32),
            "BUNDLE_MISSING_DEPENDENCY_HASH",
        )?;
        let up = d.name().to_uppercase();
        let marker = format!(
            "byte[32] constant {up}_TEMPLATE_HASH = byte[32](0x{});",
            "00".repeat(32)
        );
        check(linked.matches(&marker).count() == 1, "BUNDLE_PLACEHOLDER")?;
        linked = linked.replacen(
            &marker,
            &format!("byte[32] constant {up}_TEMPLATE_HASH = byte[32](0x{hash});"),
            1,
        );
    }
    Ok(linked)
}

pub struct LoadedBundle {
    pub profile: Profile,
    pub pins: Value,
    /// sha256 over the concatenated sha256 of every embedded file (reported, not trusted).
    pub bundle_digest: String,
}

pub fn load_bundle(b: &Bundle) -> Result<LoadedBundle> {
    let fail = |m: &str| anyhow!("V202_BUNDLE_INVALID {m}");
    let pins: Value = serde_json::from_slice(b.pins)?;
    check(
        pins.get("approvalStatus") != Some(&json!("UNAPPROVED_CANDIDATE")),
        "V202_BUNDLE_UNAPPROVED",
    )?;
    check(
        pins.get("contractVersion") == Some(&json!(S::CONTRACT_VERSION)),
        "V202_BUNDLE_VERSION",
    )?;
    if sha256_hex(b.layout) != pins["layoutSha256"].as_str().unwrap_or("") {
        return Err(fail("layout provenance drift"));
    }
    if pins["compilerCommit"] != json!(S::COMPILER_COMMIT)
        || pins["compilerBinarySha256"] != json!(COMPILER_SHA256)
    {
        return Err(fail("compiler pin"));
    }
    if pins["networkGenesis"] != json!(BUNDLE_NETWORK_GENESIS) {
        return Err(fail("bundle network genesis pin"));
    }
    let pf = pins["frames"].as_object().ok_or_else(|| fail("frames"))?;
    let mut keys: Vec<&String> = pf.keys().collect();
    keys.sort();
    if keys != ["open", "refunding", "sealed"] {
        return Err(fail("frame set"));
    }
    let mut frames: [Option<Frame>; 3] = [None, None, None];
    for m in ORDER {
        let pin = &pf[m.name()];
        let want_deps: serde_json::Map<String, Value> = deps(m)
            .iter()
            .map(|d| {
                (
                    d.name().to_string(),
                    json!(frames[d.idx()].as_ref().unwrap().template_hash),
                )
            })
            .collect();
        if pin["dependencies"] != Value::Object(want_deps) {
            return Err(fail(&format!("{}: dependency drift", m.name())));
        }
        let (tpl, linked, artifact, args) = b.modules[m.idx()];
        let template = std::str::from_utf8(tpl)?;
        for mk in markers(m) {
            if !template.contains(mk) {
                return Err(fail(&format!(
                    "{}: V2.0.2 source marker missing: {mk}",
                    m.name()
                )));
            }
        }
        if template.contains("0x4b573230") {
            return Err(fail(&format!("{}: old KW20 magic present", m.name())));
        }
        let s = |k: &str| pin[k].as_str().unwrap_or("").to_string();
        if sha256_hex(tpl) != s("templateSourceSha256") {
            return Err(fail(&format!("{}: template drift", m.name())));
        }
        if std::str::from_utf8(linked)? != link_source(m, template, &frames)? {
            return Err(fail(&format!("{}: wrong linked dependency", m.name())));
        }
        if sha256_hex(linked) != s("sourceSha256") {
            return Err(fail(&format!("{}: linked source drift", m.name())));
        }
        if sha256_hex(artifact) != s("artifactSha256") {
            return Err(fail(&format!("{}: artifact drift", m.name())));
        }
        if sha256_hex(args) != s("constructorArgsSha256") {
            return Err(fail(&format!("{}: constructor drift", m.name())));
        }
        let frame =
            parse_compiled_frame(m, &serde_json::from_slice(artifact)?, &s("sourceSha256"))?;
        if frame.template_hash != s("templateHash")
            || Some(frame.tail.len() as u64) != pin["tailBytes"].as_u64()
        {
            return Err(fail(&format!("{}: frame pin", m.name())));
        }
        frames[m.idx()] = Some(frame);
    }
    let frames: [Frame; 3] = frames.map(|f| f.unwrap());
    let computed = S::profile_id(&frames)?;
    if pins["profileId"] != json!(computed) {
        return Err(fail("Profile ID differs from pins"));
    }
    let serialized: serde_json::Map<String, Value> = S::MODULES
        .iter()
        .map(|m| {
            let f = &frames[m.idx()];
            (m.name().to_string(), json!({"tail": hex(&f.tail), "templateHash": f.template_hash, "dispatchTag": f.dispatch_tag, "sourceSha256": f.source_sha256}))
        })
        .collect();
    let metadata: Value = serde_json::from_slice(b.profile_json)?;
    let expected = json!({"id": computed, "contractVersion": pins["contractVersion"], "compilerCommit": S::COMPILER_COMMIT, "frames": serialized});
    if metadata != expected {
        return Err(fail("Profile metadata drift"));
    }
    let report: Value = serde_json::from_slice(b.build_report)?;
    if report["profileId"] != json!(computed)
        || report["contractVersion"] != pins["contractVersion"]
        || report["compilerCommit"] != json!(S::COMPILER_COMMIT)
        || report["compilerBinarySha256"] != json!(COMPILER_SHA256)
        || report["layoutSha256"] != pins["layoutSha256"]
        || report["compileOrder"] != json!(["refunding", "sealed", "open"])
        || report["frames"] != Value::Object(serialized)
    {
        return Err(fail("build report drift"));
    }
    let profile = Profile::new(frames)?;
    let mut all = String::new();
    for f in [b.pins, b.profile_json, b.layout, b.build_report] {
        all.push_str(&sha256_hex(f));
    }
    for (a, l, j, g) in b.modules {
        for f in [a, l, j, g] {
            all.push_str(&sha256_hex(f));
        }
    }
    Ok(LoadedBundle {
        profile,
        pins,
        bundle_digest: sha256_hex(all.as_bytes()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn embedded_bundle_verifies_to_the_approved_profile() {
        let b = load_bundle(&EMBEDDED).unwrap();
        assert_eq!(b.profile.id(), S::APPROVED_PROFILE_ID);
        assert_eq!(b.profile.frame(Module::Open).tail.len(), 3701);
        assert_eq!(b.profile.frame(Module::Sealed).tail.len(), 3830);
        assert_eq!(b.profile.frame(Module::Refunding).tail.len(), 9888);
        assert_eq!(b.profile.frame(Module::Open).dispatch_tag, "1c49a445");
    }
}
