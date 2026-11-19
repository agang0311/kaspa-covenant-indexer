#[cfg(target_os = "linux")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[cfg(target_os = "linux")]
#[unsafe(export_name = "_rjem_malloc_conf")]
pub static MALLOC_CONF: &[u8] = b"dirty_decay_ms:1000,muzzy_decay_ms:1000\0";

mod address;
mod api;
#[cfg(test)]
mod checks;
mod compiler;
mod contracts;
mod core;
mod dual_node;
mod engine;
mod log;
mod networks;
mod node;
mod node_pool;
mod normalize;
mod replay;
mod rpc;
mod runtime;
mod store;
mod tracer;
mod util;
mod window_scan;

use anyhow::{Result, bail};
use clap::Parser;
use serde_json::json;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(
    version,
    about = "Kaspa covenant indexer: native Rust, read-only on-chain, Kaswin V2.0.2"
)]
struct Cli {
    #[arg(default_value="run",value_parser=["run","replay","check-cases","status","contracts","track"])]
    command: String,
    /// replay DB, or track GENESIS_TXID START_CHAIN_HASH
    args: Vec<String>,
    #[arg(long, default_value = "kaswin-events.sqlite")]
    db: String,
    #[arg(
        long,
        alias = "network",
        env = "KASWIN_NETWORKS",
        default_value = "tn10"
    )]
    networks: String,
    #[arg(long,env="KASWIN_NODE_URL",conflicts_with_all=["urls","pool"])]
    url: Option<String>,
    #[arg(long,env="KASWIN_NODE_URLS",conflicts_with_all=["url","pool"])]
    urls: Option<String>,
    #[arg(long)]
    pool: bool,
    #[arg(long)]
    pool_urls: Option<String>,
    #[arg(long)]
    contracts: Option<String>,
    #[arg(long, env = "ADMIN_TOKEN", hide_env_values = true)]
    admin_token: Option<String>,
    #[arg(long, default_value = "127.0.0.1")]
    api_host: String,
    #[arg(long, default_value_t = 8789)]
    api_port: u16,
    #[arg(long, default_value_t = 3000)]
    checkpoint_ms: u64,
    #[arg(long, default_value_t = 60000)]
    reconcile_ms: u64,
    #[arg(long, default_value_t = 600)]
    finality_blue_score: u64,
    #[arg(long, env = "KASWIN_REST_URL")]
    rest_url: Option<String>,
    #[arg(long)]
    relay: Option<String>,
    #[arg(long)]
    relay_file: Option<String>,
}
#[tokio::main(worker_threads = 2)]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cli = Cli::parse();
    if cli.command == "check-cases" {
        let path = cli
            .args
            .first()
            .ok_or_else(|| anyhow::anyhow!("CASE_FILE_REQUIRED"))?;
        println!("{}", replay::check_cases(path)?);
        return Ok(());
    }
    if cli.command == "replay" {
        let path = cli.args.first().unwrap_or(&cli.db);
        let r = replay::replay(path, true)?;
        println!("{}", serde_json::to_string_pretty(&r)?);
        if r["mismatchOrError"] != 0 {
            bail!("REPLAY_MISMATCH");
        }
        return Ok(());
    }
    let nets = networks::parse_networks(Some(&cli.networks))?;
    if (cli.url.is_some() || cli.urls.is_some()) && nets.len() != 1 {
        bail!("EXPLICIT_NODES_ARE_SINGLE_NETWORK");
    }
    if cli.checkpoint_ms < 100 || cli.reconcile_ms < 100 || cli.api_port == 0 {
        bail!("INVALID_INTERVAL_OR_PORT");
    }
    if cli.command == "contracts" {
        let mut out = json!({});
        for n in &nets {
            let c = contracts::load_contracts(
                cli.contracts.as_deref(),
                &contracts::LoadCtx {
                    network: n.key,
                    registry_addresses: None,
                },
            )?;
            out[n.key] = json!({"networkId":n.network_id,"genesis":n.genesis,"contracts":c.describe(),"watch":c.watch_addresses()});
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    if cli.command == "status" {
        let mut out = json!({});
        for n in &nets {
            let p = runtime::db_path(&cli.db, n, nets.len() > 1);
            // Status must not create or migrate a database.
            let db = rusqlite::Connection::open_with_flags(
                &p,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )?;
            let rounds: i64 = db.query_row("SELECT COUNT(*) FROM rounds", [], |r| r.get(0))?;
            let transitions: i64 =
                db.query_row("SELECT COUNT(*) FROM transitions", [], |r| r.get(0))?;
            out[n.key] = json!({"db":p,"rounds":rounds,"transitions":transitions});
        }
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    if cli.command == "track"
        && (nets.len() != 1 || cli.args.len() != 2 || !cli.args.iter().all(|s| util::is_hash(s)))
    {
        bail!("usage: kci track GENESIS_TXID START_CHAIN_HASH --network tn10 --db PATH");
    }
    let mut locks = vec![];
    let mut views = vec![];
    let mut event_streams = vec![];
    for n in &nets {
        let db = runtime::db_path(&cli.db, n, nets.len() > 1);
        if let Some(parent) = std::path::Path::new(&db)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(format!("{db}.kci-lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .map_err(|_| anyhow::anyhow!("DATABASE_ALREADY_IN_USE {db}"))?;
        locks.push(lock);
        let urls = cli.url.clone().map(|u| vec![u]).or_else(|| {
            cli.urls
                .as_ref()
                .map(|s| s.split(',').map(|s| s.trim().to_owned()).collect())
        });
        let extras = cli
            .pool_urls
            .as_deref()
            .unwrap_or("")
            .split(',')
            .filter(|u| u.contains(&format!("/{}/", n.resolver_path)))
            .map(str::to_owned)
            .collect();
        let rest = match cli.rest_url.as_deref() {
            Some("none" | "off" | "false") => None,
            Some(s) => Some(s.into()),
            None => Some(n.rest.into()),
        };
        let config = engine::EngineConfig {
            rest_url: rest,
            finality_blue_score: i128::from(cli.finality_blue_score),
            relay_enabled: cli.relay.is_some() || cli.relay_file.is_some(),
            ..Default::default()
        };
        let (view, events) =
            runtime::open_view(n, &db, cli.contracts.as_deref(), urls, extras, config).await?;
        views.push(view);
        event_streams.push(events);
    }
    let mut tasks = vec![];
    for (v, events) in views.iter().zip(event_streams) {
        tasks.extend(
            runtime::start_view(v.clone(), events, cli.checkpoint_ms, cli.reconcile_ms).await?,
        );
    }
    if cli.command == "track" {
        let e = &views[0].engine;
        let (id, start) = (cli.args[0].clone(), cli.args[1].clone());
        let (tx, rx) = tokio::sync::oneshot::channel();
        e.run(format!("track:{id}"), move |e| async move {
            let result = e
                .discover_genesis(&id, &json!({"starts":[start]}), true)
                .await;
            let _ = tx.send(result);
            Ok(())
        });
        let result = rx.await??;
        log::log("TRACK", json!({"result":result}));
        for task in tasks {
            task.abort();
        }
        e.node.close().await;
        return Ok(());
    }
    if cli.relay.is_some() || cli.relay_file.is_some() {
        for v in &views {
            let e = v.engine.clone();
            let url = cli.relay.clone();
            let file = cli.relay_file.clone();
            let token = std::env::var("KASWIN_RELAY_TOKEN").ok();
            tasks.push(tokio::spawn(async move {
                let http = reqwest::Client::new();
                let mut interval = tokio::time::interval(Duration::from_secs(2));
                loop {
                    interval.tick().await;
                    let result: Result<()> = async {
                        let items = e.store.db().pending(100)?;
                        for (id, mut p) in items {
                            p["idempotencyKey"] = json!(format!(
                                "{}:{}:{}",
                                util::js_string(p.get("type")),
                                util::js_string(p.get("roundId")),
                                p.get("seq")
                                    .map(|x| util::js_string(Some(x)))
                                    .unwrap_or_default()
                            ));
                            let body = serde_json::to_string(&p)?;
                            if let Some(path) = &file {
                                use std::io::Write;
                                let mut f = std::fs::OpenOptions::new()
                                    .create(true)
                                    .append(true)
                                    .open(path)?;
                                writeln!(f, "{body}")?;
                            }
                            if let Some(u) = &url {
                                let mut req = http
                                    .post(u)
                                    .header("content-type", "application/json")
                                    .body(body)
                                    .timeout(Duration::from_secs(8));
                                if let Some(t) = &token {
                                    req = req.bearer_auth(t);
                                }
                                let r = req.send().await?;
                                if !r.status().is_success() && r.status().as_u16() != 409 {
                                    bail!("RELAY_HTTP_{}", r.status());
                                }
                            }
                            e.store.db().delivered(id, util::now_ms())?;
                        }
                        Ok(())
                    }
                    .await;
                    if let Err(err) = result {
                        log::log(
                            "RELAY_RETRY",
                            json!({"network":e.net.key,"message":err.to_string()}),
                        );
                    }
                }
            }));
        }
    }
    let token = match cli
        .admin_token
        .or_else(|| std::env::var("KASPA_INDEXER_ADMIN_TOKEN").ok())
    {
        Some(t) => t,
        None => views[0]
            .engine
            .store
            .db()
            .get_meta("adminToken")?
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| hex::encode(rand::random::<[u8; 32]>())),
    };
    if token.is_empty() {
        bail!("ADMIN_TOKEN_EMPTY");
    }
    views[0]
        .engine
        .store
        .db()
        .set_meta("adminToken", &json!(token))?;
    let api = Arc::new(api::Api {
        views: views.clone(),
        token,
        contracts_path: cli.contracts,
        started: Instant::now(),
        mutations: tokio::sync::Mutex::new(()),
    });
    let listener = tokio::net::TcpListener::bind((cli.api_host.as_str(), cli.api_port)).await?;
    log::log(
        "API_LISTENING",
        json!({"address":listener.local_addr()?.to_string(),"runtime":"rust"}),
    );
    let server = axum::serve(listener, api::router(api)).with_graceful_shutdown(async {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM");
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=term.recv()=>{}}
    });
    server.await?;
    for t in tasks {
        t.abort();
    }
    for v in views {
        let _ = tokio::time::timeout(Duration::from_secs(20), v.engine.idle()).await;
        v.engine.node.close().await;
    }
    drop(locks);
    Ok(())
}
