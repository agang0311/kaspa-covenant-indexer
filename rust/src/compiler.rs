//! SilverScript compiler adapter. No shell, fixed executable, private temporary directory, bounded output/time.
use anyhow::{Result, bail};
use serde_json::{Value, json};
use std::{path::PathBuf, time::Duration};
use tokio::{io::AsyncReadExt, process::Command};

pub async fn compile(body: &Value) -> Result<Value> {
    let source = body["source"].as_str().unwrap_or("");
    if source.trim().is_empty() {
        return Ok(json!({"ok":false,"error":"EMPTY_SOURCE"}));
    }
    if source.len() > 512 * 1024 {
        return Ok(json!({"ok":false,"error":"SOURCE_TOO_LARGE"}));
    }
    let candidates: Vec<PathBuf> = std::env::var_os("SILVERC_PATH")
        .map(PathBuf::from)
        .into_iter()
        .chain([
            PathBuf::from("bin/silverc"),
            PathBuf::from("../bin/silverc"),
            PathBuf::from("/opt/kaspa-covenant-indexer/bin/silverc"),
            PathBuf::from("/opt/kaswin-event-indexer/bin/silverc"),
        ])
        .collect();
    let Some(bin) = candidates.into_iter().find(|p| p.is_file()) else {
        return Ok(json!({"ok":false,"error":"SILVERC_NOT_FOUND"}));
    };
    let dir = tempfile::Builder::new().prefix("kci-silverc-").tempdir()?;
    let src = dir.path().join("contract.sil"); // Client filename is display metadata, never a filesystem path.
    std::fs::write(&src, source)?;
    let mut cmd = Command::new(bin);
    let ast = body["astOnly"].as_bool().unwrap_or(false);
    if ast {
        cmd.arg("--ast-only");
    } else if let Some(args) = body["constructorArgs"].as_array().filter(|a| !a.is_empty()) {
        let mut normalized = vec![];
        for a in args {
            let v = match a {
                Value::Object(o) if o.contains_key("kind") && o.contains_key("value") => a.clone(),
                Value::Bool(_) => json!({"kind":"bool","value":a}),
                Value::Number(n) if n.is_i64() || n.is_u64() => json!({"kind":"int","value":a}),
                Value::String(s) if s.starts_with("0x") => {
                    json!({"kind":"bytes","value":hex::decode(&s[2..])?})
                }
                Value::String(s) if s.parse::<i64>().is_ok() => {
                    json!({"kind":"int","value":s.parse::<i64>()?})
                }
                Value::String(s) => json!({"kind":"text","value":s}),
                _ => bail!("INVALID_CONSTRUCTOR_ARGUMENT"),
            };
            normalized.push(v);
        }
        let ctor = dir.path().join("ctor.json");
        std::fs::write(&ctor, serde_json::to_vec(&normalized)?)?;
        cmd.arg("--constructor-args").arg(ctor);
    }
    cmd.arg("-c")
        .arg(src)
        .kill_on_drop(true)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn()?;
    let mut stdout = child.stdout.take().unwrap().take(8 * 1024 * 1024 + 1);
    let mut stderr = child.stderr.take().unwrap().take(8 * 1024 * 1024 + 1);
    let mut out = vec![];
    let mut err = vec![];
    let work = async {
        tokio::try_join!(
            stdout.read_to_end(&mut out),
            stderr.read_to_end(&mut err),
            child.wait()
        )
    };
    let status = match tokio::time::timeout(Duration::from_secs(10), work).await {
        Ok(r) => r?.2,
        Err(_) => {
            child.kill().await?;
            return Ok(json!({"ok":false,"error":"COMPILATION_TIMEOUT"}));
        }
    };
    if out.len() > 8 * 1024 * 1024 || err.len() > 8 * 1024 * 1024 {
        bail!("COMPILER_OUTPUT_TOO_LARGE");
    }
    if !status.success() {
        return Ok(
            json!({"ok":false,"error":"COMPILATION_ERROR","detail":String::from_utf8_lossy(&err)}),
        );
    }
    let raw: Value = serde_json::from_slice(&out)?;
    if ast {
        let field = |v: &Value| json!({"name":v["name"],"type":v["type_ref"]["base"].as_str().unwrap_or("unknown")});
        let fields: Vec<Value> = raw["fields"]
            .as_array()
            .into_iter()
            .flatten()
            .map(field)
            .collect();
        let params: Vec<Value> = raw["params"]
            .as_array()
            .into_iter()
            .flatten()
            .map(field)
            .collect();
        let functions:Vec<Value>=raw["functions"].as_array().into_iter().flatten().map(|f|json!({"name":f["name"],"params":f["params"].as_array().into_iter().flatten().map(field).collect::<Vec<_>>(),"attributes":f["attributes"].as_array().into_iter().flatten().map(|a|a["name"].clone()).collect::<Vec<_>>()})).collect();
        return Ok(
            json!({"ok":true,"mode":"ast","contractName":raw["name"],"fields":fields,"params":params,"functions":functions,"raw":raw}),
        );
    }
    let mut contracts = vec![];
    if let Some(cs) = raw["contracts"].as_object() {
        for (name, c) in cs {
            let bytes: Vec<u8> = c["compiled"]["template_hash"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_u64().and_then(|x| u8::try_from(x).ok()))
                .collect();
            let entries: Vec<Value> = c["entries"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(k, v)| json!({"key":k,"dispatchTag":v["dispatch_tag"],"params":v["params"]}))
                .collect();
            contracts.push(json!({"name":name,"sourcePath":c["source_path"],"templateHashHex":hex::encode(bytes),"bytecodeLength":c["compiled"]["bytecode"].as_array().map(Vec::len).unwrap_or(0),"stateSpan":c["compiled"]["state_span"],"stateFields":c["runtime_state"]["fields"],"entries":entries,"covDeclToAbi":c["cov_decl_to_abi"]}));
        }
    }
    Ok(
        json!({"ok":true,"mode":"compiled","schemaVersion":raw["schema_version"],"compilerVersion":raw["compiler_version"],"contracts":contracts,"artifact":raw}),
    )
}
