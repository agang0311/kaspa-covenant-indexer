//! Network catalog: one entry per monitored Kaspa network. Pinned facts (genesis / prefix / registry); endpoints are
//! operator-configurable hints, never a consensus claim. A network context never falls back to another network.
use anyhow::{Result, anyhow, bail};

#[derive(Debug)]
pub struct Network {
    pub key: &'static str,
    pub label: &'static str,
    pub network_id: &'static str,
    pub genesis: &'static str,
    pub address_prefix: &'static str,
    pub rest: &'static str,
    pub registry: &'static str,
    pub seed: &'static [&'static str],
    pub resolver_path: &'static str,
}

pub static TN10: Network = Network {
    key: "tn10",
    label: "Testnet 10",
    network_id: "testnet-10",
    genesis: "f896a3034873be1739fc4359236899fd3d65d2bc94f9780df0d0da3eb1cc4370",
    address_prefix: "kaspatest",
    rest: "https://api-tn10.kaspa.org",
    registry: "kaspatest:qztrjpfpuf6g9jw76e6ay909z43hndpv5maqkker6ur4enz7llarsh82ezggl",
    seed: &[
        "wss://muon-10.kaspa.blue/kaspa/testnet-10/wrpc/json",
        "wss://vector-10.kaspa.green/kaspa/testnet-10/wrpc/json",
        "wss://quark-10.kaspa.red/kaspa/testnet-10/wrpc/json",
        "wss://electron-10.kaspa.stream/kaspa/testnet-10/wrpc/json",
        "wss://neutrino-10.kaspa.stream/kaspa/testnet-10/wrpc/json",
        "wss://electron-10.kaspa.blue/kaspa/testnet-10/wrpc/json",
        "wss://proton-10.kaspa.stream/kaspa/testnet-10/wrpc/json",
    ],
    resolver_path: "testnet-10",
};

pub static MAINNET: Network = Network {
    key: "mainnet",
    label: "Kaspa mainnet",
    network_id: "mainnet",
    genesis: "58c2d4199e21f910d1571d114969cecef48f09f934d42ccb6a281a15868f2999",
    address_prefix: "kaspa",
    rest: "https://api.kaspa.org",
    registry: "kaspa:qzccgw790r0mnfe9uvy7nwd7a8990029ycg8e0manehhgruqm2r9v528nz0qr",
    seed: &[
        "wss://kate.kaspa.red/kaspa/mainnet/wrpc/json",
        "wss://john.kaspa.red/kaspa/mainnet/wrpc/json",
        "wss://eva.kaspa.green/kaspa/mainnet/wrpc/json",
        "wss://ivy.kaspa.green/kaspa/mainnet/wrpc/json",
        "wss://dina.kaspa.green/kaspa/mainnet/wrpc/json",
    ],
    resolver_path: "mainnet",
};

pub static ALL: [&Network; 2] = [&TN10, &MAINNET];
pub const DEFAULT_NETWORK: &str = "tn10";

/// Resolve a network key ('tn10' | 'mainnet') or a full networkId ('testnet-10' | 'mainnet'). Fails closed.
pub fn network_config(key: &str) -> Result<&'static Network> {
    if key.is_empty() {
        bail!("NETWORK_REQUIRED");
    }
    ALL.iter()
        .copied()
        .find(|n| n.key == key)
        .or_else(|| ALL.iter().copied().find(|n| n.network_id == key))
        .ok_or_else(|| anyhow!("UNKNOWN_NETWORK {key}"))
}

pub fn network_of_genesis(genesis: &str) -> Option<&'static Network> {
    ALL.iter().copied().find(|n| n.genesis == genesis)
}

/// Parse a comma-separated --networks value. Rejects unknown, empty and duplicate entries.
pub fn parse_networks(raw: Option<&str>) -> Result<Vec<&'static Network>> {
    let raw = match raw {
        None | Some("") => DEFAULT_NETWORK,
        Some(r) => r,
    };
    let list: Vec<&str> = raw
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    if list.is_empty() {
        bail!("NETWORKS_EMPTY");
    }
    let nets = list
        .iter()
        .map(|k| network_config(k))
        .collect::<Result<Vec<_>>>()?;
    let mut keys: Vec<&str> = nets.iter().map(|n| n.key).collect();
    keys.sort();
    keys.dedup();
    if keys.len() != nets.len() {
        bail!("NETWORKS_DUPLICATE");
    }
    Ok(nets)
}
