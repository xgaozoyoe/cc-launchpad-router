use crate::{now, App};
use futures_util::TryStreamExt;
use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
use mongodb::{
    bson::{doc, Document},
    options::ReturnDocument,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sha3::Keccak256;
use std::{sync::Arc, time::Duration};
type R<T> = Result<T, String>;
fn fail(_: impl std::fmt::Display) -> String {
    "Stake service unavailable".into()
}
fn text<'a>(d: &'a Document, k: &str) -> R<&'a str> {
    d.get_str(k).map_err(fail)
}
pub fn address(v: &str) -> R<String> {
    if v.len() == 42
        && v.starts_with("0x")
        && v[2..].bytes().all(|b| b.is_ascii_hexdigit())
        && v[2..].bytes().any(|b| b != b'0')
    {
        Ok(v.to_lowercase())
    } else {
        Err("Invalid wallet address".into())
    }
}
fn hash(v: &str) -> R<String> {
    if v.len() == 66 && v.starts_with("0x") && v[2..].bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(v.to_lowercase())
    } else {
        Err("Invalid transaction hash".into())
    }
}
pub fn project_key(id: &str) -> String {
    format!("0x{}", hex::encode(Sha256::digest(id.as_bytes())))
}
fn selector(sig: &str) -> String {
    hex::encode(&Keccak256::digest(sig.as_bytes())[..4])
}
fn word(v: u128) -> String {
    format!("{v:064x}")
}
fn addr_word(a: &str) -> String {
    format!("{:0>64}", &a[2..])
}
fn uint(v: &str) -> R<u128> {
    u128::from_str_radix(v.strip_prefix("0x").unwrap_or(v), 16)
        .map_err(|_| "Invalid chain integer".into())
}
fn data_word(data: &str, i: usize) -> R<&str> {
    let d = data.strip_prefix("0x").unwrap_or(data);
    d.get(i * 64..(i + 1) * 64)
        .ok_or_else(|| "Invalid ABI response".into())
}
fn value_str<'a>(v: &'a Value, k: &str) -> R<&'a str> {
    v.get(k)
        .and_then(Value::as_str)
        .ok_or_else(|| "Invalid chain response".into())
}
pub fn recover(message: &str, signature: &str) -> R<String> {
    let bytes = hex::decode(signature.strip_prefix("0x").unwrap_or(signature)).map_err(fail)?;
    if bytes.len() != 65 {
        return Err("Invalid wallet signature".into());
    }
    let v = match bytes[64] {
        27 | 28 => bytes[64] - 27,
        0 | 1 => bytes[64],
        _ => return Err("Invalid recovery id".into()),
    };
    let sig = Signature::from_slice(&bytes[..64]).map_err(fail)?;
    let prefix = format!("\x19Ethereum Signed Message:\n{}", message.len());
    let digest = Keccak256::new()
        .chain_update(prefix.as_bytes())
        .chain_update(message.as_bytes())
        .finalize();
    let key = VerifyingKey::recover_from_prehash(
        &digest,
        &sig,
        RecoveryId::from_byte(v).ok_or("Invalid recovery id")?,
    )
    .map_err(|_| "Invalid wallet signature".to_string())?;
    let encoded = key.to_encoded_point(false);
    let digest = Keccak256::digest(&encoded.as_bytes()[1..]);
    Ok(format!("0x{}", hex::encode(&digest[12..])))
}
#[derive(Clone, Deserialize)]
pub struct Config {
    pub chain_id: u64,
    pub staking_contract: String,
    pub token_address: String,
    pub deployment_block: u64,
}
fn validate_challenge(c: &Document, id: &str, config: &Config, time: i64) -> R<()> {
    if c.get_i64("chain_id").map_err(fail)? != config.chain_id as i64
        || text(c, "contract")? != config.staking_contract
        || text(c, "token")? != config.token_address
    {
        return Err("Chain configuration changed; sign a new challenge".into());
    }
    if text(c, "id")? != id
        || c.get_bool("used").unwrap_or(true)
        || c.get_i64("expires_at").map_err(fail)? <= time
    {
        return Err("Wallet challenge expired or used".into());
    }
    Ok(())
}
#[derive(Clone)]
pub struct Stake {
    pub config: Config,
    http: reqwest::Client,
    rpc_url: String,
    origin: String,
    operator: Option<(String, String, String)>,
    operator_address: Option<String>,
}
impl Stake {
    pub async fn load() -> R<Option<Self>> {
        let Some(path) = std::env::var("CHAIN_CONFIG_FILE").ok() else {
            return Ok(None);
        };
        let mut config: Config =
            serde_json::from_slice(&std::fs::read(path).map_err(fail)?).map_err(fail)?;
        if config.chain_id != 11155111
            && !(config.chain_id == 31337
                && std::env::var("CHAIN_ALLOW_LOCAL").as_deref() == Ok("1"))
        {
            return Err("Expected Sepolia".into());
        }
        config.staking_contract = address(&config.staking_contract)?;
        config.token_address = address(&config.token_address)?;
        let origin = std::env::var("APP_ORIGIN").map_err(fail)?;
        let parsed = reqwest::Url::parse(&origin).map_err(fail)?;
        if parsed.scheme() != "https"
            && !(config.chain_id == 31337 && parsed.host_str() == Some("localhost"))
        {
            return Err("Invalid signing origin".into());
        }
        let rpc_url = std::env::var("CHAIN_RPC_URL").map_err(fail)?;
        let rpc = reqwest::Url::parse(&rpc_url).map_err(fail)?;
        if rpc.scheme() != "https"
            && !(config.chain_id == 31337
                && matches!(rpc.host_str(), Some("localhost" | "127.0.0.1")))
        {
            return Err("RPC requires HTTPS".into());
        }
        let operator = match (
            std::env::var("STAKE_CAST"),
            std::env::var("STAKE_KEYSTORE"),
            std::env::var("STAKE_PASSWORD_FILE"),
        ) {
            (Ok(a), Ok(b), Ok(c)) => Some((a, b, c)),
            (Err(_), Err(_), Err(_)) => None,
            _ => return Err("Incomplete registration signer configuration".into()),
        };
        let mut s = Self {
            config,
            http: reqwest::Client::builder()
                .user_agent("cc-launchpad-stake/1.0")
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(30))
                .build()
                .map_err(fail)?,
            rpc_url,
            origin,
            operator,
            operator_address: None,
        };
        s.validate().await?;
        if let Some((cast, keystore, password)) = &s.operator {
            let mut command = tokio::process::Command::new(cast);
            command
                .args([
                    "wallet",
                    "address",
                    "--keystore",
                    keystore,
                    "--password-file",
                    password,
                ])
                .kill_on_drop(true);
            let out = tokio::time::timeout(Duration::from_secs(20), command.output())
                .await
                .map_err(|_| "Signer check timed out")?
                .map_err(fail)?;
            if !out.status.success() {
                return Err("Registration signer is unavailable".into());
            }
            let signer = address(String::from_utf8_lossy(&out.stdout).trim())?;
            let role = s
                .call(
                    &s.config.staking_contract,
                    &format!("0x91d14854{}{}", "0".repeat(64), addr_word(&signer)),
                    "latest",
                )
                .await?;
            if uint(&role)? != 1 {
                return Err("Registration signer is not a contract administrator".into());
            }
            s.operator_address = Some(signer);
        }
        Ok(Some(s))
    }
    pub async fn rpc(&self, method: &str, params: Value) -> R<Value> {
        let r: Value = self
            .http
            .post(&self.rpc_url)
            .json(&json!({"jsonrpc":"2.0","id":1,"method":method,"params":params}))
            .send()
            .await
            .map_err(fail)?
            .error_for_status()
            .map_err(fail)?
            .json()
            .await
            .map_err(fail)?;
        if r.get("error").is_some() {
            return Err(format!("Chain RPC rejected {method}"));
        }
        r.get("result")
            .cloned()
            .ok_or_else(|| "Invalid RPC result".into())
    }
    async fn call(&self, to: &str, data: &str, tag: &str) -> R<String> {
        self.rpc("eth_call", json!([{"to":to,"data":data},tag]))
            .await?
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| "Invalid contract response".into())
    }
    async fn validate(&self) -> R<()> {
        let chain = self.rpc("eth_chainId", json!([])).await?;
        if uint(chain.as_str().ok_or("Invalid chain")?)? != self.config.chain_id as u128 {
            return Err("Wrong chain".into());
        }
        let token = self
            .call(&self.config.staking_contract, "0x51ed6a30", "latest")
            .await?;
        if token.len() != 66
            || format!("0x{}", &token[26..]).to_lowercase() != self.config.token_address
        {
            return Err("Wrong staking token".into());
        }
        Ok(())
    }
    pub async fn state(&self, key: &str, tag: &str) -> R<(u128, u128, u64, u64, String)> {
        let d = self
            .call(
                &self.config.staking_contract,
                &format!("0x45b69b1d{}", &key[2..]),
                tag,
            )
            .await?;
        Ok((
            uint(data_word(&d, 0)?)?,
            uint(data_word(&d, 1)?)?,
            uint(data_word(&d, 2)?)?.try_into().map_err(fail)?,
            uint(data_word(&d, 3)?)?.try_into().map_err(fail)?,
            format!("0x{}", &data_word(&d, 4)?[24..]).to_lowercase(),
        ))
    }
    pub async fn handle(
        &self,
        app: &App,
        user: &str,
        id: &str,
        method: &str,
        action: &str,
        body: &[u8],
    ) -> R<Value> {
        if method == "POST" {
            let key = format!("{}:{}", self.config.chain_id, self.config.staking_contract);
            let state = app
                .db
                .collection::<Document>("chain_state")
                .find_one(doc! {"_id":key})
                .await
                .map_err(fail)?;
            if state.as_ref().and_then(|d| d.get_bool("halted").ok()) == Some(true) {
                return Err("Stake service unavailable: finalized checkpoint changed".into());
            }
        }
        let owners = crate::account_ids(app, user).await.map_err(fail)?;
        let projects = app.db.collection::<Document>("projects");
        let project = projects
            .find_one(doc! {"_id":id,"account_id":{"$in":owners.clone()}})
            .await
            .map_err(fail)?
            .ok_or("Project not found")?;
        if project.get_str("status").unwrap_or("draft") != "draft"
            && (project.get_i64("stake_chain_id").ok() != Some(self.config.chain_id as i64)
                || project.get_str("staking_contract").ok()
                    != Some(self.config.staking_contract.as_str()))
        {
            return Err("Project belongs to another staking configuration".into());
        }
        if method == "GET" && action.is_empty() {
            let mut out = json!({"project":project,"chain_id":self.config.chain_id,"token_address":self.config.token_address,"staking_contract":self.config.staking_contract,"test_token":true});
            if let (Ok(wallet), Ok(raw)) = (
                project.get_str("wallet_address"),
                project.get_str("stake_raw"),
            ) {
                let balance = self
                    .call(
                        &self.config.token_address,
                        &format!("0x70a08231{}", addr_word(wallet)),
                        "latest",
                    )
                    .await?;
                out["token_balance_raw"] = json!(uint(&balance)?.to_string());
                out["contract_status"] =
                    json!(self.state(&project_key(id), "latest").await?.0.to_string());
                out["approve_data"] = json!(format!(
                    "0x095ea7b3{}{}",
                    addr_word(&self.config.staking_contract),
                    word(raw.parse().map_err(fail)?)
                ));
                out["stake_data"] = json!(format!(
                    "0x{}{}",
                    selector("stake(bytes32)"),
                    &project_key(id)[2..]
                ));
            }
            return Ok(out);
        }
        let input: Value = serde_json::from_slice(body).map_err(|_| "Invalid stake request")?;
        match (method, action) {
            ("POST", "challenge") => {
                if text(&project, "status")? != "draft" {
                    return Err("Project is already prepared".into());
                }
                if self.operator.is_none() {
                    return Err("Registration operator is not configured".into());
                }
                let wallet = address(value_str(&input, "wallet_address")?)?;
                let amount = value_str(&input, "stake_amount")?
                    .parse::<u128>()
                    .map_err(fail)?;
                if !(1000..=100000).contains(&amount) {
                    return Err("Stake must be 1000–100000 whole test PRL".into());
                }
                let nonce = uuid::Uuid::new_v4().to_string();
                let expiry = now() + 300;
                let message=format!("AI Computing Credit Launchpad\nBind wallet for this project; no transaction is authorized by this signature.\nOrigin: {}\nAccount: {}\nProject: {}\nChain ID: {}\nStaking contract: {}\nToken: {}\nWallet: {}\nStake: {} tPRL\nNonce: {}\nExpires (Unix): {}",self.origin,user,id,self.config.chain_id,self.config.staking_contract,self.config.token_address,wallet,amount,nonce,expiry);
                let result=projects.update_one(doc!{"_id":id,"account_id":{"$in":owners.clone()},"status":"draft"},doc!{"$set":{"wallet_challenge":{"id":&nonce,"message":&message,"address":&wallet,"stake_raw":(amount*10u128.pow(18)).to_string(),"chain_id":self.config.chain_id as i64,"contract":&self.config.staking_contract,"token":&self.config.token_address,"expires_at":expiry,"used":false}}}).await.map_err(fail)?;
                if result.matched_count != 1 {
                    return Err("Project changed; retry".into());
                }
                Ok(json!({"challenge_id":nonce,"message":message,"expires_at":expiry}))
            }
            ("POST", "prepare") => {
                let c = project
                    .get_document("wallet_challenge")
                    .map_err(|_| "Request a wallet challenge first")?;
                let cid = value_str(&input, "challenge_id")?;
                validate_challenge(c, cid, &self.config, now())?;
                if recover(text(c, "message")?, value_str(&input, "signature")?)?
                    != text(c, "address")?
                {
                    return Err("Wallet signature does not match".into());
                }
                let wallet = text(c, "address")?;
                let raw = text(c, "stake_raw")?;
                let result=projects.update_one(doc!{"_id":id,"account_id":{"$in":owners.clone()},"status":"draft","wallet_challenge.id":cid,"wallet_challenge.used":false,"wallet_challenge.expires_at":{"$gt":now()}},doc!{"$set":{"status":"awaiting-registration","stake_chain_id":self.config.chain_id as i64,"staking_contract":&self.config.staking_contract,"stake_token":&self.config.token_address,"wallet_address":wallet,"stake_raw":raw,"onchain_key":project_key(id),"wallet_challenge.used":true,"wallet_verified_at":now()}}).await.map_err(fail)?;
                if result.modified_count != 1 {
                    return Err("Wallet challenge was already consumed".into());
                }
                Ok(json!({"status":"awaiting-registration"}))
            }
            ("POST", "submit") => {
                if !matches!(
                    text(&project, "status")?,
                    "awaiting-stake" | "stake-submitted"
                ) {
                    return Err("Project is not awaiting stake".into());
                }
                let tx_hash = hash(value_str(&input, "tx_hash")?)?;
                let tx = self
                    .rpc("eth_getTransactionByHash", json!([tx_hash]))
                    .await?;
                self.check_tx(&tx, &project)?;
                let result=projects.update_one(doc!{"_id":id,"account_id":{"$in":owners.clone()},"status":{"$in":["awaiting-stake","stake-submitted"]},"$or":[{"stake_tx_hash":{"$exists":false}},{"stake_tx_hash":&tx_hash}]},doc!{"$set":{"status":"stake-submitted","stake_tx_hash":&tx_hash}}).await.map_err(fail)?;
                if result.matched_count != 1 {
                    return Err("A different stake transaction is already recorded".into());
                }
                Ok(
                    json!({"status":"stake-submitted","tx_hash":tx_hash,"message":"Waiting for finalized stake event; no Launch ID before finality"}),
                )
            }
            _ => Err("Stake endpoint not found".into()),
        }
    }
    fn check_tx(&self, tx: &Value, p: &Document) -> R<()> {
        if value_str(tx, "from")?.to_lowercase() != text(p, "wallet_address")?
            || value_str(tx, "to")?.to_lowercase() != self.config.staking_contract
            || value_str(tx, "input")?.to_lowercase()
                != format!(
                    "0x{}{}",
                    selector("stake(bytes32)"),
                    &text(p, "onchain_key")?[2..]
                )
        {
            return Err("Transaction is not this provider's direct stake".into());
        }
        Ok(())
    }
    async fn register(&self, app: &App, p: &Document) -> R<()> {
        let key = text(p, "onchain_key")?;
        let wallet = text(p, "wallet_address")?;
        let amount = text(p, "stake_raw")?.parse::<u128>().map_err(fail)?;
        let mut state = self.state(key, "latest").await?;
        if state.0 == 0 {
            let signer = self
                .operator_address
                .as_ref()
                .ok_or("Registration operator is unavailable")?;
            let latest = self
                .rpc("eth_getTransactionCount", json!([signer, "latest"]))
                .await?;
            let pending = self
                .rpc("eth_getTransactionCount", json!([signer, "pending"]))
                .await?;
            if latest != pending {
                return Err("Operator has a pending transaction; waiting before retry".into());
            }

            let (cast, keystore, password) = self
                .operator
                .as_ref()
                .ok_or("Registration operator is unavailable")?;
            let mut command = tokio::process::Command::new(cast);
            command.args([
                "send",
                "--json",
                "--rpc-url",
                &self.rpc_url,
                &self.config.staking_contract,
                "registerProject(bytes32,address,uint256)",
                key,
                wallet,
                &amount.to_string(),
                "--keystore",
                keystore,
                "--password-file",
                password,
                "--gas-limit",
                "2000000",
            ]);
            command.kill_on_drop(true);
            let out = tokio::time::timeout(Duration::from_secs(120), command.output())
                .await
                .map_err(|_| "Registration transaction timed out")?
                .map_err(fail)?;
            if !out.status.success() {
                return Err(
                    "Registration signer rejected transaction; retry after inspecting chain state"
                        .into(),
                );
            }
            state = self.state(key, "latest").await?;
        }
        if !matches!(state.0, 1..=5) || state.1 != amount || state.4 != wallet {
            return Err("On-chain registration differs from wallet commitment".into());
        }
        app.db.collection::<Document>("projects").update_one(doc!{"_id":text(p,"_id")?,"status":"awaiting-registration"},doc!{"$set":{"status":"awaiting-stake","registered_at":now()},"$unset":{"registration_error":""}}).await.map_err(fail)?;
        Ok(())
    }
    async fn accept_event(&self, app: &App, log: &Value, finalized: u64, chain_time: u64) -> R<()> {
        if log.get("removed").and_then(Value::as_bool) == Some(true) {
            return Err("Removed finalized event".into());
        }
        let topics = log
            .get("topics")
            .and_then(Value::as_array)
            .ok_or("Invalid event topics")?;
        if topics.len() != 3 {
            return Err("Invalid stake event".into());
        }
        let key = topics[1]
            .as_str()
            .ok_or("Invalid project key")?
            .to_lowercase();
        let Some(p) = app
            .db
            .collection::<Document>("projects")
            .find_one(doc! {"onchain_key":&key,"stake_chain_id":self.config.chain_id as i64,"staking_contract":&self.config.staking_contract})
            .await
            .map_err(fail)?
        else {
            return Ok(());
        };
        if p.contains_key("launch_id") {
            return Ok(());
        }
        let hash = hash(value_str(log, "transactionHash")?)?;
        let receipt = self.rpc("eth_getTransactionReceipt", json!([hash])).await?;
        if value_str(&receipt, "status")? != "0x1" {
            return Err("Reverted stake transaction".into());
        }
        let block_number = uint(value_str(&receipt, "blockNumber")?)? as u64;
        if block_number > finalized {
            return Err("Stake is not finalized".into());
        }
        let tx = self.rpc("eth_getTransactionByHash", json!([hash])).await?;
        self.check_tx(&tx, &p)?;
        if let Ok(submitted) = p.get_str("stake_tx_hash") {
            if submitted != hash {
                return Err("Stake hash differs from recorded submission".into());
            }
        }
        let expected_topic = format!(
            "0x{}",
            hex::encode(Keccak256::digest(
                b"ProjectStaked(bytes32,address,uint256,uint64,uint64)"
            ))
        );
        let count = receipt
            .get("logs")
            .and_then(Value::as_array)
            .ok_or("Missing receipt logs")?
            .iter()
            .filter(|l| {
                l.get("address")
                    .and_then(Value::as_str)
                    .map(str::to_lowercase)
                    == Some(self.config.staking_contract.clone())
                    && l.get("topics")
                        .and_then(Value::as_array)
                        .and_then(|t| t.first())
                        .and_then(Value::as_str)
                        == Some(expected_topic.as_str())
            })
            .count();
        if count != 1 {
            return Err("Multiple stake events in one provider transaction".into());
        }
        let block = self
            .rpc(
                "eth_getBlockByNumber",
                json!([format!("0x{block_number:x}"), false]),
            )
            .await?;
        if value_str(&block, "hash")? != value_str(&receipt, "blockHash")?
            || value_str(log, "blockHash")? != value_str(&receipt, "blockHash")?
        {
            return Err("Noncanonical stake block".into());
        }
        let data = value_str(log, "data")?;
        if data.len() != 194 {
            return Err("Invalid stake event data".into());
        }
        let amount = uint(data_word(data, 0)?)?;
        let at = uint(data_word(data, 1)?)? as u64;
        let deadline = uint(data_word(data, 2)?)? as u64;
        let wallet = text(&p, "wallet_address")?;
        if amount != text(&p, "stake_raw")?.parse::<u128>().map_err(fail)?
            || at != uint(value_str(&block, "timestamp")?)? as u64
            || deadline != at.checked_add(86400).ok_or("Invalid deadline")?
            || topics[2].as_str() != Some(format!("0x{}", addr_word(wallet)).as_str())
        {
            return Err("Stake commitment mismatch".into());
        }
        let mut inactive = None;
        for tag in [format!("0x{finalized:x}"), "latest".into()] {
            let s = self.state(&key, &tag).await?;
            if !matches!(s.0, 2..=5)
                || s.1 != amount
                || s.2 != at
                || s.3 != deadline
                || s.4 != wallet
            {
                return Err("Stake commitment changed".into());
            }
            if s.0 == 4 {
                inactive = Some("refunded")
            } else if s.0 == 5 {
                inactive = Some("released")
            }
        }
        let status = if let Some(state) = inactive {
            state
        } else if deadline <= chain_time {
            "expired"
        } else {
            "awaiting-rpc"
        };
        let mut changes = doc! {"status":status,"provider_id":&hash,"stake_tx_hash":&hash,"staked_at":at as i64,"rpc_deadline":deadline as i64,"stake_block":block_number as i64,"stake_block_hash":value_str(&block,"hash")?,"stake_finalized":true};
        if status == "awaiting-rpc" {
            changes.insert("launch_id", format!("launch-{}", uuid::Uuid::new_v4()));
        }
        app.db.collection::<Document>("projects").update_one(doc!{"_id":text(&p,"_id")?,"status":{"$in":["awaiting-stake","stake-submitted"]},"launch_id":{"$exists":false}},doc!{"$set":changes}).await.map_err(fail)?;
        Ok(())
    }
    async fn renew_lease(&self, app: &App, id: &str, owner: &str) -> R<()> {
        let r = app
            .db
            .collection::<Document>("chain_state")
            .update_one(
                doc! {"_id":id,"lease_owner":owner,"lease_until":{"$gt":now()}},
                doc! {"$set":{"lease_until":now()+300}},
            )
            .await
            .map_err(fail)?;
        if r.matched_count != 1 {
            return Err("Monitor lease was lost".into());
        }
        Ok(())
    }
    async fn tick(&self, app: &App, owner: &str) -> R<()> {
        self.validate().await?;
        let states = app.db.collection::<Document>("chain_state");
        let id = format!("{}:{}", self.config.chain_id, self.config.staking_contract);
        let cursor = states
            .find_one(doc! {"_id":&id})
            .await
            .map_err(fail)?
            .ok_or("Missing chain cursor")?;
        if cursor.get_bool("halted").unwrap_or(false) {
            return Err("Finalized chain changed; operator intervention required".into());
        }
        if let (Ok(number), Ok(saved_hash)) =
            (cursor.get_i64("block"), cursor.get_str("block_hash"))
        {
            let b = self
                .rpc(
                    "eth_getBlockByNumber",
                    json!([format!("0x{number:x}"), false]),
                )
                .await?;
            if b.get("hash").and_then(Value::as_str) != Some(saved_hash) {
                states
                    .update_one(
                        doc! {"_id":&id},
                        doc! {"$set":{"halted":true,"error":"Finalized checkpoint changed"}},
                    )
                    .await
                    .map_err(fail)?;
                app.db.collection::<Document>("projects").update_many(doc!{"stake_chain_id":self.config.chain_id as i64,"staking_contract":&self.config.staking_contract,"status":{"$in":["awaiting-rpc","verification-pending","accepted"]}},doc!{"$set":{"status":"chain-halted"}}).await.map_err(fail)?;
                return Err("Finalized checkpoint changed".into());
            }
        }
        self.renew_lease(app, &id, owner).await?;
        if let Some(p)=app.db.collection::<Document>("projects").find_one(doc!{"status":"awaiting-registration","stake_chain_id":self.config.chain_id as i64,"staking_contract":&self.config.staking_contract,"$or":[{"registration_retry_at":{"$exists":false}},{"registration_retry_at":{"$lte":now()}}]}).await.map_err(fail)?{
   if let Err(e)=self.register(app,&p).await{app.db.collection::<Document>("projects").update_one(doc!{"_id":text(&p,"_id")?},doc!{"$set":{"registration_error":e,"registration_retry_at":now()+60}}).await.map_err(fail)?;}
  }
        self.renew_lease(app, &id, owner).await?;
        let finalized = self
            .rpc("eth_getBlockByNumber", json!(["finalized", false]))
            .await?;
        let head = uint(value_str(&finalized, "number")?)? as u64;
        let latest = self
            .rpc("eth_getBlockByNumber", json!(["latest", false]))
            .await?;
        let chain_time = uint(value_str(&latest, "timestamp")?)? as u64;
        if chain_time < uint(value_str(&finalized, "timestamp")?)? as u64 {
            return Err("Inconsistent chain clock".into());
        }

        let from = cursor
            .get_i64("block")
            .map(|n| n as u64 + 1)
            .unwrap_or(self.config.deployment_block);
        if from <= head {
            let to = head.min(from + 99);
            let topic = format!(
                "0x{}",
                hex::encode(Keccak256::digest(
                    b"ProjectStaked(bytes32,address,uint256,uint64,uint64)"
                ))
            );
            let logs=self.rpc("eth_getLogs",json!([{"address":self.config.staking_contract,"topics":[topic],"fromBlock":format!("0x{from:x}"),"toBlock":format!("0x{to:x}")}])).await?;
            for log in logs.as_array().ok_or("Invalid event list")? {
                self.renew_lease(app, &id, owner).await?;
                self.accept_event(app, log, head, chain_time).await?;
            }
            let block = self
                .rpc("eth_getBlockByNumber", json!([format!("0x{to:x}"), false]))
                .await?;
            states.update_one(doc!{"_id":&id,"lease_owner":owner,"lease_until":{"$gt":now()}},doc!{"$set":{"block":to as i64,"block_hash":value_str(&block,"hash")?,"finalized_head":head as i64,"updated_at":now(),"error":""}}).await.map_err(fail)?;
        }
        let submitted = app
            .db
            .collection::<Document>("projects")
            .find(doc! {"status":"stake-submitted","stake_chain_id":self.config.chain_id as i64,"staking_contract":&self.config.staking_contract})
            .limit(100)
            .await
            .map_err(fail)?
            .try_collect::<Vec<_>>()
            .await
            .map_err(fail)?;
        for p in submitted {
            let Ok(h) = p.get_str("stake_tx_hash") else {
                continue;
            };
            let receipt = self.rpc("eth_getTransactionReceipt", json!([h])).await?;
            if receipt.get("status").and_then(Value::as_str) == Some("0x0") {
                app.db.collection::<Document>("projects").update_one(doc!{"_id":text(&p,"_id")?,"status":"stake-submitted","stake_tx_hash":h},doc!{"$set":{"status":"awaiting-stake"},"$unset":{"stake_tx_hash":""},"$addToSet":{"failed_stake_transactions":h}}).await.map_err(fail)?;
            }
        }
        let projects = app
            .db
            .collection::<Document>("projects")
            .find(doc! {"status":{"$in":["awaiting-rpc","expired","verification-pending","accepted"]},"stake_chain_id":self.config.chain_id as i64,"staking_contract":&self.config.staking_contract,"onchain_key":{"$exists":true}})
            .await
            .map_err(fail)?
            .try_collect::<Vec<_>>()
            .await
            .map_err(fail)?;
        for p in projects {
            let s = self.state(text(&p, "onchain_key")?, "latest").await?;
            if s.0 == 2 {
                let expired = s.3 <= chain_time;
                let mut reverted = false;
                if let Ok(hash) = p.get_str("verification_tx_hash") {
                    let receipt = self.rpc("eth_getTransactionReceipt", json!([hash])).await?;
                    reverted = receipt["status"].as_str() == Some("0x0");
                }
                if expired || reverted {
                    app.db
                        .collection::<Document>("projects")
                        .update_one(
                            doc! {"_id":text(&p,"_id")?},
                            doc! {"$set":{"status":if expired {"expired"} else {"awaiting-rpc"}}},
                        )
                        .await
                        .map_err(fail)?;
                }
            }
            if s.0 == 3 {
                let final_state = self.state(text(&p, "onchain_key")?, "finalized").await?;
                app.db.collection::<Document>("projects").update_one(doc!{"_id":text(&p,"_id")?},doc!{"$set":{"status":if final_state.0 == 3 {"accepted"} else {"verification-pending"}}}).await.map_err(fail)?;
            }
            if matches!(s.0, 4 | 5) {
                app.db
                    .collection::<Document>("projects")
                    .update_one(
                        doc! {"_id":text(&p,"_id")?},
                        doc! {"$set":{"status":if s.0==4{"refunded"}else{"released"}}},
                    )
                    .await
                    .map_err(fail)?;
            }
        }
        Ok(())
    }
}
pub async fn run(app: Arc<App>) {
    let Some(stake) = app.stake.clone() else {
        return;
    };
    let states = app.db.collection::<Document>("chain_state");
    let id = format!(
        "{}:{}",
        stake.config.chain_id, stake.config.staking_contract
    );
    let owner = uuid::Uuid::new_v4().to_string();
    let _ = states
        .update_one(
            doc! {"_id":&id},
            doc! {"$setOnInsert":{"lease_until":0_i64,"halted":false}},
        )
        .upsert(true)
        .await;
    loop {
        let lease = states
            .find_one_and_update(
                doc! {"_id":&id,"$or":[{"lease_until":{"$lte":now()}},{"lease_owner":&owner}]},
                doc! {"$set":{"lease_until":now()+300,"lease_owner":&owner}},
            )
            .return_document(ReturnDocument::After)
            .await;
        if matches!(lease, Ok(Some(_))) {
            if let Err(e) = stake.tick(&app, &owner).await {
                let _ = states
                    .update_one(
                        doc! {"_id":&id,"lease_owner":&owner},
                        doc! {"$set":{"error":e,"updated_at":now()}},
                    )
                    .await;
            }
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use k256::ecdsa::SigningKey;
    #[test]
    fn wallet_proof_is_message_bound() {
        let key = SigningKey::from_bytes((&[7_u8; 32]).into()).unwrap();
        let message = "Bind project abc";
        let pref = format!("\x19Ethereum Signed Message:\n{}{}", message.len(), message);
        let digest = Keccak256::new().chain_update(pref.as_bytes());
        let (sig, id) = key.sign_digest_recoverable(digest).unwrap();
        let mut bytes = sig.to_bytes().to_vec();
        bytes.push(id.to_byte() + 27);
        let signature = format!("0x{}", hex::encode(bytes));
        let pubkey = key.verifying_key().to_encoded_point(false);
        let expected = format!(
            "0x{}",
            hex::encode(&Keccak256::digest(&pubkey.as_bytes()[1..])[12..])
        );
        assert_eq!(recover(message, &signature).unwrap(), expected);
        assert_ne!(recover("Another project", &signature).unwrap(), expected);
    }
    #[test]
    fn challenge_expiry_replay_and_contract_binding() {
        let config = Config {
            chain_id: 11155111,
            staking_contract: "escrow".into(),
            token_address: "token".into(),
            deployment_block: 1,
        };
        let c = doc! {"id":"nonce","used":false,"expires_at":100_i64,"chain_id":11155111_i64,"contract":"escrow","token":"token"};
        assert!(validate_challenge(&c, "nonce", &config, 99).is_ok());
        assert!(validate_challenge(&c, "nonce", &config, 100).is_err());
        assert!(validate_challenge(&c, "wrong-nonce", &config, 99).is_err());
        let mut used = c.clone();
        used.insert("used", true);
        assert!(validate_challenge(&used, "nonce", &config, 99).is_err());
        let mut changed = c.clone();
        changed.insert("contract", "other");
        assert!(validate_challenge(&changed, "nonce", &config, 99).is_err());
        let mut changed = c.clone();
        changed.insert("chain_id", 1_i64);
        assert!(validate_challenge(&changed, "nonce", &config, 99).is_err());
    }
    #[test]
    fn identity_and_abi() {
        assert!(address("0x0000000000000000000000000000000000000000").is_err());
        assert!(hash("0x123").is_err());
        assert_eq!(selector("stake(bytes32)"), "c0d64a87");
        assert_eq!(
            project_key("abc"),
            "0xba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
