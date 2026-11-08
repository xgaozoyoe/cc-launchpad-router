//! Staged contract-group validation. Never replaces legacy configuration or moves funds.
use crate::{credits, stake, App};
use serde::Deserialize;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use std::collections::{BTreeMap, BTreeSet};

const COMPONENTS: [&str; 9] = [
    "factory",
    "governance",
    "registry",
    "credit_token",
    "usage_escrow",
    "payment_escrow",
    "stake_vault",
    "sale_router",
    "settlement_module",
];
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub id: String,
    pub version: String,
    pub chain_id: u64,
    pub deployment_block: u64,
    pub contracts: BTreeMap<String, String>,
    pub runtime_hashes: BTreeMap<String, String>,
    pub prl_token: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u8,
    candidates: Vec<Group>,
}
fn hash(value: &str) -> bool {
    value.len() == 66
        && value.starts_with("0x")
        && value[2..].bytes().all(|v| v.is_ascii_hexdigit())
}
impl Group {
    fn validate(mut self, chain: u64) -> Result<Self, String> {
        if self.version != "modular-v1"
            || self.chain_id != chain
            || self.deployment_block == 0
            || self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return Err("Unsupported candidate version, chain or identity".into());
        }
        self.prl_token = stake::address(&self.prl_token)?;
        if self.contracts.len() != COMPONENTS.len() || self.runtime_hashes.len() != COMPONENTS.len()
        {
            return Err("Every protocol component and reviewed runtime hash is required".into());
        }
        let mut unique = BTreeSet::new();
        unique.insert(self.prl_token.clone());
        for name in COMPONENTS {
            let address = stake::address(self.contracts.get(name).ok_or("Missing component")?)?;
            if !unique.insert(address.clone()) {
                return Err("Protocol components must have distinct addresses".into());
            }
            self.contracts.insert(name.into(), address);
            let digest = self
                .runtime_hashes
                .get_mut(name)
                .ok_or("Missing reviewed runtime hash")?;
            if !hash(digest) {
                return Err("Invalid reviewed runtime hash".into());
            }
            *digest = digest.to_lowercase();
        }
        Ok(self)
    }
}
fn parse(bytes: &[u8], chain: u64) -> Result<Vec<Group>, String> {
    if bytes.len() > 65536 {
        return Err("Protocol manifest too large".into());
    }
    let manifest: Manifest =
        serde_json::from_slice(bytes).map_err(|_| "Invalid protocol manifest")?;
    if manifest.schema_version != 1 || manifest.candidates.len() > 2 {
        return Err("Unsupported protocol manifest".into());
    }
    let mut ids = BTreeSet::new();
    manifest
        .candidates
        .into_iter()
        .map(|g| {
            if !ids.insert(g.id.clone()) {
                return Err("Duplicate protocol group identity".into());
            }
            g.validate(chain)
        })
        .collect()
}
fn load(chain: u64) -> Result<Vec<Group>, String> {
    match std::env::var("PROTOCOL_GROUPS_FILE") {
        Err(std::env::VarError::NotPresent) => Ok(vec![]),
        Err(_) => Err("Protocol manifest unavailable".into()),
        Ok(path) => parse(
            &std::fs::read(path).map_err(|_| "Protocol manifest unavailable")?,
            chain,
        ),
    }
}
fn field<'a>(value: &'a Value, name: &str) -> Result<&'a str, String> {
    value[name]
        .as_str()
        .ok_or_else(|| "Invalid chain response".into())
}
fn expected_address(raw: &str) -> Result<String, String> {
    if raw.len() != 66 || !raw.starts_with("0x") || raw[2..26].bytes().any(|c| c != b'0') {
        return Err("Invalid address response".into());
    }
    stake::address(&format!("0x{}", &raw[26..]))
}
async fn audit(app: &App, g: &Group) -> Result<Value, String> {
    let rpc = app.stake.as_ref().ok_or("Chain unavailable")?;
    if g.prl_token != rpc.config.token_address {
        return Err("Candidate uses a different PRL token".into());
    }
    let chain = rpc.rpc("eth_chainId", json!([])).await?;
    if credits::uint(chain.as_str().ok_or("Invalid chain ID")?)? != g.chain_id as u128 {
        return Err("Candidate RPC chain mismatch".into());
    }
    let block = rpc
        .rpc("eth_getBlockByNumber", json!(["finalized", false]))
        .await?;
    let number = field(&block, "number")?.to_string();
    let block_hash = field(&block, "hash")?.to_string();
    if credits::uint(&number)? < g.deployment_block as u128 {
        return Err("Candidate deployment is not finalized".into());
    }
    for name in COMPONENTS {
        let code = rpc
            .rpc("eth_getCode", json!([g.contracts[name], number]))
            .await?;
        let raw = code
            .as_str()
            .and_then(|c| c.strip_prefix("0x"))
            .ok_or("Invalid bytecode response")?;
        let bytes = hex::decode(raw).map_err(|_| "Invalid bytecode")?;
        if bytes.is_empty() {
            return Err(format!("No finalized code for {name}"));
        }
        let digest = format!("0x{}", hex::encode(Keccak256::digest(bytes)));
        if digest != g.runtime_hashes[name] {
            return Err(format!("Reviewed runtime hash mismatch: {name}"));
        }
    }
    let mut checks: Vec<(String, String, String)> = vec![];
    for (component, getter) in [
        ("registry", "registry()"),
        ("credit_token", "creditToken()"),
        ("usage_escrow", "usageEscrow()"),
        ("payment_escrow", "paymentEscrow()"),
        ("stake_vault", "stakeVault()"),
        ("sale_router", "saleRouter()"),
        ("settlement_module", "settlementModule()"),
    ] {
        checks.push((
            "factory".into(),
            getter.into(),
            g.contracts[component].clone(),
        ));
    }
    for (source, getter, target) in [
        ("registry", "stakeVault()", "stake_vault"),
        ("credit_token", "registry()", "registry"),
        ("credit_token", "paymentEscrow()", "payment_escrow"),
        ("credit_token", "usageEscrow()", "usage_escrow"),
        ("usage_escrow", "registry()", "registry"),
        ("usage_escrow", "token()", "credit_token"),
        ("usage_escrow", "paymentEscrow()", "payment_escrow"),
        ("payment_escrow", "registry()", "registry"),
        ("payment_escrow", "token()", "credit_token"),
        ("payment_escrow", "usageEscrow()", "usage_escrow"),
        ("stake_vault", "registry()", "registry"),
        ("stake_vault", "usageEscrow()", "usage_escrow"),
        ("sale_router", "paymentEscrow()", "payment_escrow"),
        ("settlement_module", "usageEscrow()", "usage_escrow"),
        ("registry", "governor()", "governance"),
        ("usage_escrow", "governor()", "governance"),
        ("payment_escrow", "governor()", "governance"),
        ("settlement_module", "governor()", "governance"),
    ] {
        checks.push((source.into(), getter.into(), g.contracts[target].clone()));
    }
    checks.push((
        "stake_vault".into(),
        "stakeToken()".into(),
        g.prl_token.clone(),
    ));
    for (source, getter, expected) in checks {
        let raw = rpc
            .rpc(
                "eth_call",
                json!([{"to":g.contracts[&source],"data":credits::data(&getter,&[])},number]),
            )
            .await?;
        if expected_address(raw.as_str().ok_or("Invalid binding response")?)? != expected {
            return Err(format!("Peer binding mismatch: {source}.{getter}"));
        }
    }
    for component in [
        "registry",
        "usage_escrow",
        "payment_escrow",
        "settlement_module",
    ] {
        let raw=rpc.rpc("eth_call",json!([{"to":g.contracts[component],"data":credits::data("hasRole(bytes32,address)",&[credits::word(0),credits::addr(&g.contracts["governance"])])},number])).await?;
        if credits::uint(raw.as_str().ok_or("Invalid admin role response")?)? != 1 {
            return Err(format!("Governance role mismatch: {component}"));
        }
    }
    for (component, role, member) in [
        ("payment_escrow", "SALE_ROLE", "sale_router"),
        ("usage_escrow", "SETTLEMENT_ROLE", "settlement_module"),
    ] {
        let role = hex::encode(Keccak256::digest(role.as_bytes()));
        let raw=rpc.rpc("eth_call",json!([{"to":g.contracts[component],"data":credits::data("hasRole(bytes32,address)",&[role,credits::addr(&g.contracts[member])])},number])).await?;
        if credits::uint(raw.as_str().ok_or("Invalid module role response")?)? != 1 {
            return Err(format!("Active module role mismatch: {component}"));
        }
    }
    let delay=rpc.rpc("eth_call",json!([{"to":g.contracts["governance"],"data":credits::data("getMinDelay()",&[])},number])).await?;
    if credits::uint(delay.as_str().ok_or("Invalid governance delay")?)? != 172800 {
        return Err("Expected fixed 48-hour governance delay".into());
    }
    let canonical = rpc
        .rpc("eth_getBlockByNumber", json!([number, false]))
        .await?;
    if field(&canonical, "hash")? != block_hash {
        return Err("Candidate checkpoint changed during audit".into());
    }
    Ok(
        json!({"finalized_block":number,"finalized_block_hash":block_hash,"code_and_bindings":"passed"}),
    )
}
/// This endpoint is deliberately read-only and cannot activate a candidate.
pub async fn report(app: &App) -> Value {
    let Some(s) = app.stake.as_ref() else {
        return json!({"phase":"not-configured","can_activate":false,"blockers":["Chain unavailable"]});
    };
    let candidates = match load(s.config.chain_id) {
        Ok(c) => c,
        Err(e) => {
            return json!({"phase":"blocked","can_activate":false,"legacy_staking_contract":s.config.staking_contract,"blockers":[e]})
        }
    };
    let mut rows = vec![];
    for g in candidates {
        let validation = match tokio::time::timeout(
            std::time::Duration::from_secs(20),
            audit(app, &g),
        )
        .await
        {
            Ok(Ok(value)) => value,
            Ok(Err(message)) => json!({"code_and_bindings":"failed","error":message}),
            Err(_) => {
                json!({"code_and_bindings":"failed","error":"Candidate audit timed out; no activation occurred"})
            }
        };
        rows.push(json!({"id":g.id,"version":g.version,"chain_id":g.chain_id,"contracts":g.contracts,"validation":validation}));
    }
    json!({"phase":if rows.is_empty(){"preparing"}else{"staged"},"can_activate":false,"legacy_staking_contract":s.config.staking_contract,"candidates":rows,
      "blockers":["Migration importer and atomic handover are not implemented in this contract version","Modular Rust/UI purchase, reservation and settlement integration is incomplete","Per-project contract-group monitors and migration reconciliation are required","Holder consent, fresh frozen snapshot and fork/Sepolia rehearsal are required"],
      "note":"Legacy contracts remain active. A successful bytecode/binding audit does not authorize activation or move funds."})
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Value {
        let contracts: BTreeMap<String, String> = COMPONENTS
            .iter()
            .enumerate()
            .map(|(i, n)| (n.to_string(), format!("0x{:040x}", i + 1)))
            .collect();
        let hashes: BTreeMap<String, String> = COMPONENTS
            .iter()
            .map(|n| (n.to_string(), format!("0x{}", "ab".repeat(32))))
            .collect();
        json!({"schema_version":1,"candidates":[{"id":"candidate-1","version":"modular-v1","chain_id":11155111,"deployment_block":100,"contracts":contracts,"runtime_hashes":hashes,"prl_token":format!("0x{:040x}",99)}]})
    }
    fn valid(v: &Value) -> bool {
        parse(&serde_json::to_vec(v).unwrap(), 11155111).is_ok()
    }
    #[test]
    fn valid_candidate_is_only_a_candidate() {
        assert!(valid(&fixture()));
        assert!(valid(&json!({"schema_version":1,"candidates":[]})));
    }
    #[test]
    fn wrong_chain_or_version_rejected() {
        let mut v = fixture();
        v["candidates"][0]["chain_id"] = json!(1);
        assert!(!valid(&v));
        let mut v = fixture();
        v["candidates"][0]["version"] = json!("legacy");
        assert!(!valid(&v));
    }
    #[test]
    fn missing_or_duplicate_addresses_rejected() {
        let mut v = fixture();
        v["candidates"][0]["contracts"]["factory"] =
            v["candidates"][0]["contracts"]["governance"].clone();
        assert!(!valid(&v));
        let mut v = fixture();
        v["candidates"][0]["contracts"]
            .as_object_mut()
            .unwrap()
            .remove("registry");
        assert!(!valid(&v));
    }
    #[test]
    fn unreviewed_code_and_unknown_settings_rejected() {
        let mut v = fixture();
        v["candidates"][0]["runtime_hashes"]["factory"] = json!("0x00");
        assert!(!valid(&v));
        let mut v = fixture();
        v["candidates"][0]["activate"] = json!(true);
        assert!(!valid(&v));
    }
    #[test]
    fn duplicate_group_ids_rejected() {
        let mut v = fixture();
        let g = v["candidates"][0].clone();
        v["candidates"].as_array_mut().unwrap().push(g);
        assert!(!valid(&v));
    }
    #[test]
    fn short_and_noncanonical_abi_addresses_rejected() {
        assert!(expected_address("0x00").is_err());
        assert!(expected_address(&format!("0x{}{}", "1".repeat(24), "2".repeat(40))).is_err());
        assert_eq!(
            expected_address(&format!("0x{}{}", "0".repeat(24), "a".repeat(40))).unwrap(),
            format!("0x{}", "a".repeat(40))
        );
    }
}
