//! Sepolia purchase/escrow accounting. Chain balances are authoritative; MongoDB holds durable usage reservations.
use crate::{error, now, stake, unavailable, ApiResult, App};
use axum::{http::StatusCode, Json};
use futures_util::TryStreamExt;
use mongodb::bson::{doc, DateTime, Document};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sha3::Keccak256;
#[derive(Clone, Deserialize)]
pub struct Config {
    pub chain_id: u64,
    pub contract: String,
    pub staking_contract: String,
    pub settler: String,
    pub deployment_block: u64,
}
pub fn config(app: &App) -> Result<Config, String> {
    let path =
        std::env::var("CREDIT_CONFIG_FILE").map_err(|_| "Credit contracts are not configured")?;
    let mut c: Config = serde_json::from_slice(
        &std::fs::read(path).map_err(|_| "Credit configuration unavailable")?,
    )
    .map_err(|_| "Credit configuration invalid")?;
    c.contract = stake::address(&c.contract)?;
    c.settler = stake::address(&c.settler)?;
    let s = app.stake.as_ref().ok_or("Stake service unavailable")?;
    if c.chain_id != s.config.chain_id || c.staking_contract != s.config.staking_contract {
        return Err("Credit/stake chain mismatch".into());
    }
    Ok(c)
}
pub fn sel(s: &str) -> String {
    hex::encode(&Keccak256::digest(s.as_bytes())[..4])
}
pub fn word(n: u128) -> String {
    format!("{n:064x}")
}
pub fn addr(a: &str) -> String {
    format!("{:0>64}", &a[2..])
}
pub fn uint(s: &str) -> Result<u128, String> {
    u128::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|_| "Invalid chain integer".into())
}
pub fn w(s: &str, i: usize) -> Result<&str, String> {
    s.trim_start_matches("0x")
        .get(i * 64..(i + 1) * 64)
        .ok_or("Invalid chain response".into())
}
pub fn data(signature: &str, args: &[String]) -> String {
    format!("0x{}{}", sel(signature), args.join(""))
}
pub async fn call(app: &App, c: &Config, data: String, tag: &str) -> Result<String, String> {
    app.stake
        .as_ref()
        .ok_or("Chain unavailable")?
        .rpc("eth_call", json!([{"to":c.contract,"data":data},tag]))
        .await?
        .as_str()
        .map(str::to_owned)
        .ok_or("Invalid chain response".into())
}
#[derive(Clone)]
pub struct Offer {
    pub provider: String,
    pub supply: u128,
    pub issued: u128,
    pub price: u128,
    pub active: bool,
}
pub async fn offer(app: &App, p: &Document, tag: &str) -> Result<Offer, String> {
    let c = config(app)?;
    let key = stake::project_key(p.get_str("_id").map_err(|_| "Missing project")?);
    let raw = call(
        app,
        &c,
        data("offerState(bytes32)", &[key[2..].into()]),
        tag,
    )
    .await?;
    if raw.trim_start_matches("0x").is_empty() {
        return Err("Credit contract awaiting Ethereum finality".into());
    }
    let model = format!(
        "{}",
        hex::encode(Keccak256::digest(
            p.get_str("model_id").unwrap_or("").as_bytes()
        ))
    );
    let o = Offer {
        provider: format!("0x{}", &w(&raw, 0)?[24..]),
        supply: uint(w(&raw, 1)?)?,
        issued: uint(w(&raw, 2)?)?,
        price: uint(w(&raw, 3)?)?,
        active: uint(w(&raw, 4)?)? == 1,
    };
    if o.supply == 0 || w(&raw, 5)? != model {
        return Err("No matching credit offering".into());
    }
    Ok(o)
}
#[derive(Clone)]
pub struct Account {
    pub locked: u128,
    pub deposited: u128,
    pub burned: u128,
    pub withdrawn: u128,
    pub withdrawal: u128,
    pub nonce: u128,
    pub approved: bool,
    pub pending: bool,
}
pub async fn account(
    app: &App,
    c: &Config,
    pid: &str,
    wallet: &str,
    tag: &str,
) -> Result<Account, String> {
    let key = stake::project_key(pid);
    let raw = call(
        app,
        c,
        data(
            "accountState(bytes32,address)",
            &[key[2..].into(), addr(wallet)],
        ),
        tag,
    )
    .await?;
    Ok(Account {
        locked: uint(w(&raw, 0)?)?,
        deposited: uint(w(&raw, 1)?)?,
        burned: uint(w(&raw, 2)?)?,
        withdrawn: uint(w(&raw, 3)?)?,
        withdrawal: uint(w(&raw, 4)?)?,
        nonce: uint(w(&raw, 5)?)?,
        approved: uint(w(&raw, 6)?)? == 1,
        pending: uint(w(&raw, 7)?)? == 1,
    })
}
pub async fn project(app: &App, id: &str) -> Result<Document, (StatusCode, Json<Value>)> {
    app.db
        .collection::<Document>("projects")
        .find_one(doc! {"_id":id,"stake_finalized":true,"launch_id":{"$exists":true}})
        .await
        .map_err(unavailable)?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Launch not found"))
}
pub async fn ready(app: &App, p: &Document) -> Result<(), (StatusCode, Json<Value>)> {
    if p.get_str("status").ok() != Some("accepted") {
        return Err(error(
            StatusCode::CONFLICT,
            "Wait for finalized launch verification",
        ));
    }
    crate::deployment::eligible(app, p)
        .await
        .map_err(|e| error(StatusCode::CONFLICT, &e))?;
    let s = app
        .stake
        .as_ref()
        .ok_or_else(|| error(StatusCode::SERVICE_UNAVAILABLE, "Chain unavailable"))?;
    if s.state(p.get_str("onchain_key").map_err(unavailable)?, "finalized")
        .await
        .map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, &e))?
        .0
        != 3
    {
        return Err(error(StatusCode::CONFLICT, "Verification is not finalized"));
    }
    Ok(())
}
pub async fn wallet(app: &App, user: &str) -> Result<String, (StatusCode, Json<Value>)> {
    app.db
        .collection::<Document>("buyer_wallets")
        .find_one(doc! {"_id":user})
        .await
        .map_err(unavailable)?
        .and_then(|d| d.get_str("wallet").ok().map(str::to_owned))
        .ok_or_else(|| {
            error(
                StatusCode::CONFLICT,
                "Connect and sign your buyer wallet first",
            )
        })
}
pub async fn pending(
    app: &App,
    pid: &str,
    wallet: &str,
) -> Result<(u128, u128), mongodb::error::Error> {
    let rows=app.db.collection::<Document>("credit_requests").find(doc!{"project_id":pid,"wallet":wallet,"contract":config(app).map(|c|c.contract).unwrap_or_default(),"status":{"$ne":"failed"}}).await?.try_collect::<Vec<_>>().await?;
    let (mut reserved, mut consumed) = (0, 0);
    for r in rows {
        let n = r.get_i64("amount").unwrap_or(0).max(0) as u128;
        if matches!(r.get_str("status").unwrap_or(""), "reserved" | "inflight") {
            reserved += n
        } else {
            consumed += n
        }
    }
    Ok((reserved, consumed))
}
fn available(finalized: &Account, latest: &Account, reserved: u128, consumed: u128) -> u128 {
    if finalized.pending || latest.pending || latest.burned > consumed {
        return 0;
    }
    finalized
        .deposited
        .saturating_sub(finalized.withdrawn.max(latest.withdrawn))
        .saturating_sub(reserved.saturating_add(consumed))
        .min(latest.locked)
}
fn as_state(a: &Account) -> Value {
    json!({"locked":a.locked.to_string(),"deposited":a.deposited.to_string(),"burned":a.burned.to_string(),"withdrawn":a.withdrawn.to_string(),"withdrawal":a.withdrawal.to_string(),"withdrawal_nonce":a.nonce.to_string(),"withdrawal_approved":a.approved,"withdrawal_pending":a.pending})
}
pub async fn state(app: &App, user: &str, id: &str) -> ApiResult {
    let c = config(app).map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, &e))?;
    let p = project(app, id).await?;
    let o = offer(app, &p, "finalized")
        .await
        .map_err(|e| error(StatusCode::CONFLICT, &e))?;
    let wal = wallet(app, user).await?;
    let a = account(app, &c, id, &wal, "finalized")
        .await
        .map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, &e))?;
    let latest = account(app, &c, id, &wal, "latest")
        .await
        .map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, &e))?;
    if latest.deposited > 0 {
        app.db
            .collection::<Document>("credit_positions")
            .update_one(
                doc! {"_id":format!("{wal}:{id}")},
                doc! {"$setOnInsert":{"wallet":&wal,"project_id":id}},
            )
            .upsert(true)
            .await
            .map_err(unavailable)?;
    }
    let (r, u) = pending(app, id, &wal).await.map_err(unavailable)?;
    let free = available(&a, &latest, r, u);
    let frozen = a.pending || latest.pending || latest.burned > u;
    let requests = app
        .db
        .collection::<Document>("credit_requests")
        .find(doc! {"project_id":id,"wallet":&wal})
        .projection(doc! {"_id":1,"status":1,"amount":1,"at":1,"usage":1,"batch_id":1,"tx_hash":1})
        .sort(doc! {"at":-1})
        .limit(20)
        .await
        .map_err(unavailable)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(unavailable)?;
    Ok(Json(
        json!({"configured":true,"deployment_block":c.deployment_block,"chain_id":c.chain_id,"contract":c.contract,"wallet":wal,"project_id":id,"name":p.get_str("name").ok(),"model_id":p.get_str("model_id").ok(),"provider":o.provider,"price_wei_per_credit":o.price.to_string(),"supply":o.supply.to_string(),"issued":o.issued.to_string(),"sales_open":o.active&&p.get_str("status").ok()==Some("accepted"),"finalized":as_state(&a),"latest":as_state(&latest),"available":if frozen{0}else{free}.to_string(),"reserved":r.to_string(),"consumed_pending":u.saturating_sub(a.burned).to_string(),"incoming_pending":latest.deposited.saturating_sub(a.deposited).to_string(),"withdrawal_frozen":frozen,"requests":requests,"prl_policy":"Existing PRL remains locked; no proportional release in this test."}),
    ))
}
pub fn tx(c: &Config, wal: &str, data: String, value: u128) -> Value {
    json!({"from":wal,"to":c.contract,"data":data,"value":format!("0x{value:x}"),"chainId":format!("0x{:x}",c.chain_id)})
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Proof {
    challenge_id: String,
    signature: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WalletInput {
    wallet_address: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Quantity {
    quantity: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inference {
    prompt: String,
    max_output_tokens: u64,
    request_id: String,
}
pub async fn handle(app: &App, user: &str, path: &str, method: &str, body: &[u8]) -> ApiResult {
    let c = config(app).map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, &e))?;
    if path == "claim-revenue" && method == "POST" {
        let wal = wallet(app, user).await?;
        return Ok(Json(
            json!({"transaction":tx(&c,&wal,data("claimRevenue()",&[]),0)}),
        ));
    }
    if path == "me" && method == "GET" {
        let wal = wallet(app, user).await.ok();
        let rows = if let Some(w) = &wal {
            app.db
                .collection::<Document>("credit_positions")
                .find(doc! {"wallet":w})
                .projection(doc! {"project_id":1})
                .await
                .map_err(unavailable)?
                .try_collect::<Vec<_>>()
                .await
                .map_err(unavailable)?
        } else {
            vec![]
        };
        let revenue = if let Some(w) = &wal {
            call(app, &c, data("revenue(address)", &[addr(w)]), "finalized")
                .await
                .ok()
                .and_then(|n| uint(&n).ok())
                .unwrap_or(0)
        } else {
            0
        };
        return Ok(Json(
            json!({"configured":true,"chain_id":c.chain_id,"contract":c.contract,"wallet":wal,"positions":rows,"revenue_wei":revenue.to_string()}),
        ));
    }
    if path == "wallet/challenge" && method == "POST" {
        let input: WalletInput = serde_json::from_slice(body)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide wallet_address"))?;
        let wal = stake::address(&input.wallet_address)
            .map_err(|e| error(StatusCode::BAD_REQUEST, &e))?;
        let id = uuid::Uuid::new_v4().to_string();
        let expires = now() + 300;
        let origin = std::env::var("APP_ORIGIN").map_err(unavailable)?;
        let message=format!("AI Computing Credit Launchpad\nBind buyer wallet; no transaction or token spending is authorized by this signature.\nOrigin: {origin}\nAccount: {user}\nWallet: {wal}\nChain: {}\nCredit contract: {}\nNonce: {id}\nExpires (Unix): {expires}",c.chain_id,c.contract);
        app.db.collection::<Document>("buyer_challenges").insert_one(doc!{"_id":&id,"user":user,"wallet":&wal,"contract":&c.contract,"chain_id":c.chain_id as i64,"message":&message,"expires":expires,"used":false}).await.map_err(unavailable)?;
        return Ok(Json(
            json!({"challenge_id":id,"message":message,"chain_id":c.chain_id}),
        ));
    }
    if path == "wallet/bind" && method == "POST" {
        let input: Proof = serde_json::from_slice(body)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide signed challenge"))?;
        let ch=app.db.collection::<Document>("buyer_challenges").find_one(doc!{"_id":&input.challenge_id,"user":user,"used":false,"expires":{"$gt":now()},"contract":&c.contract,"chain_id":c.chain_id as i64}).await.map_err(unavailable)?.ok_or_else(||error(StatusCode::FORBIDDEN,"Wallet challenge expired or used"))?;
        let wal = ch.get_str("wallet").map_err(unavailable)?;
        if stake::recover(
            ch.get_str("message").map_err(unavailable)?,
            &input.signature,
        )
        .ok()
        .as_deref()
            != Some(wal)
        {
            return Err(error(
                StatusCode::FORBIDDEN,
                "Invalid buyer wallet signature",
            ));
        }
        let used = app
            .db
            .collection::<Document>("buyer_challenges")
            .update_one(
                doc! {"_id":&input.challenge_id,"used":false},
                doc! {"$set":{"used":true}},
            )
            .await
            .map_err(unavailable)?;
        if used.modified_count != 1 {
            return Err(error(StatusCode::CONFLICT, "Challenge already used"));
        }
        if let Ok(existing) = wallet(app, user).await {
            if existing != wal {
                return Err(error(
                    StatusCode::CONFLICT,
                    "This account is already bound to another buyer wallet",
                ));
            }
        }
        app.db
            .collection::<Document>("buyer_wallets")
            .update_one(
                doc! {"_id":user},
                doc! {"$setOnInsert":{"wallet":wal,"at":now()}},
            )
            .upsert(true)
            .await
            .map_err(|_| {
                error(
                    StatusCode::CONFLICT,
                    "Wallet is already bound to another account",
                )
            })?;
        if wallet(app, user).await? != wal {
            return Err(error(
                StatusCode::CONFLICT,
                "This account is already bound to another buyer wallet",
            ));
        }
        return Ok(Json(json!({"wallet":wal})));
    }
    let (id, action) = path.split_once('/').unwrap_or((path, ""));
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(error(StatusCode::NOT_FOUND, "Credit endpoint not found"));
    }
    if method == "GET" && action.is_empty() {
        return state(app, user, id).await;
    }
    let wal = wallet(app, user).await?;
    let p = project(app, id).await?;
    let key = stake::project_key(id);
    if method == "GET" && action.starts_with("request/") {
        let rid = &action[8..];
        let r = app
            .db
            .collection::<Document>("credit_requests")
            .find_one(doc! {"_id":rid,"project_id":id,"wallet":&wal})
            .await
            .map_err(unavailable)?
            .ok_or_else(|| error(StatusCode::NOT_FOUND, "Request not found"))?;
        let cached = app
            .db
            .collection::<Document>("credit_results")
            .find_one(doc! {"_id":rid,"expires_at":{"$gt":DateTime::from_millis(now()*1000)}})
            .await
            .map_err(unavailable)?;
        return Ok(Json(
            json!({"request":r,"result":cached.and_then(|d|d.get_document("result").ok().cloned())}),
        ));
    }
    if method != "POST" {
        return Err(error(StatusCode::METHOD_NOT_ALLOWED, "Use GET or POST"));
    }
    if action == "purchase" {
        ready(app, &p).await?;
        crate::rpc_credentials::paid_key(&p).map_err(|e| error(StatusCode::CONFLICT, &e))?;
        let q: Quantity = serde_json::from_slice(body)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide quantity"))?;
        let o = offer(app, &p, "latest")
            .await
            .map_err(|e| error(StatusCode::CONFLICT, &e))?;
        if !o.active || q.quantity == 0 || q.quantity as u128 > o.supply - o.issued {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Quantity exceeds available offering",
            ));
        }
        app.db
            .collection::<Document>("credit_positions")
            .update_one(
                doc! {"_id":format!("{wal}:{id}")},
                doc! {"$setOnInsert":{"wallet":&wal,"project_id":id}},
            )
            .upsert(true)
            .await
            .map_err(unavailable)?;
        return Ok(Json(
            json!({"transaction":tx(&c,&wal,data("buyAndLock(bytes32,uint256)",&[key[2..].into(),word(q.quantity as u128)]),q.quantity as u128*o.price)}),
        ));
    }
    if ["withdraw", "deposit"].contains(&action) {
        let q: Quantity = serde_json::from_slice(body)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide quantity"))?;
        if q.quantity == 0 {
            return Err(error(StatusCode::BAD_REQUEST, "Quantity must be positive"));
        }
        let state = state(app, user, id).await?.0;
        let free = state["available"]
            .as_str()
            .unwrap_or("0")
            .parse::<u128>()
            .unwrap_or(0);
        if action == "withdraw" && (q.quantity as u128 > free || state["withdrawal_frozen"] == true)
        {
            return Err(error(
                StatusCode::CONFLICT,
                "Withdrawal exceeds reconciled available credits",
            ));
        }
        let signature = if action == "withdraw" {
            "requestWithdrawal(bytes32,uint256)"
        } else {
            "depositCredits(bytes32,uint256)"
        };
        return Ok(Json(
            json!({"transaction":tx(&c,&wal,data(signature,&[key[2..].into(),word(q.quantity as u128)]),0)}),
        ));
    }
    if action == "complete-withdrawal" || action == "cancel-withdrawal" {
        let signature = if action == "complete-withdrawal" {
            "completeWithdrawal(bytes32)"
        } else {
            "cancelWithdrawal(bytes32)"
        };
        return Ok(Json(
            json!({"transaction":tx(&c,&wal,data(signature,&[key[2..].into()]),0)}),
        ));
    }
    if action == "infer" {
        ready(app, &p).await?;
        let api_key =
            crate::rpc_credentials::paid_key(&p).map_err(|e| error(StatusCode::CONFLICT, &e))?;
        let input: Inference = serde_json::from_slice(body).map_err(|_| {
            error(
                StatusCode::BAD_REQUEST,
                "Provide prompt, max_output_tokens and request_id",
            )
        })?;
        if input.prompt.trim().is_empty()
            || input.prompt.chars().count() > 500
            || !(16..=256).contains(&input.max_output_tokens)
            || uuid::Uuid::parse_str(&input.request_id).is_err()
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Use at most 500 prompt characters and 16–256 output tokens",
            ));
        }
        let rid = hex::encode(Sha256::digest(
            format!("{wal}:{id}:{}", input.request_id).as_bytes(),
        ));
        let requests = app.db.collection::<Document>("credit_requests");
        if requests
            .find_one(doc! {"_id":&rid})
            .await
            .map_err(unavailable)?
            .is_some()
        {
            let cached = app
                .db
                .collection::<Document>("credit_results")
                .find_one(doc! {"_id":&rid,"expires_at":{"$gt":DateTime::from_millis(now()*1000)}})
                .await
                .map_err(unavailable)?;
            if let Some(cache) = cached {
                return Ok(Json(
                    json!({"request_id":rid,"result":cache.get_document("result").ok(),"replayed":true}),
                ));
            }
            return Err(error(
                StatusCode::CONFLICT,
                "Request already exists; inspect its status instead of generating again",
            ));
        }
        let lease_id = format!("paid:{wal}:{id}");
        crate::trial::take_lease(app, &lease_id, &rid).await?;
        let leases = vec![lease_id, format!("project:{id}")];
        if let Err(e) = crate::trial::take_lease(app, &leases[1], &rid).await {
            crate::trial::release(app, &leases, &rid).await;
            return Err(e);
        }
        let attempt = tokio::time::timeout(std::time::Duration::from_secs(80), async {
            let a = account(app, &c, id, &wal, "finalized").await.map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, &e))?;
            let latest = account(app, &c, id, &wal, "latest").await.map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, &e))?;
            let (r, u) = pending(app, id, &wal).await.map_err(unavailable)?;
            if available(&a, &latest, r, u) < input.max_output_tokens as u128 {
                return Err(error(StatusCode::CONFLICT, "Insufficient finalized available credits or a withdrawal is pending"));
            }
            requests.insert_one(doc! {"_id":&rid,"project_id":id,"wallet":&wal,"contract":&c.contract,"at":now(),"status":"inflight","amount":input.max_output_tokens as i64}).await.map_err(unavailable)?;
            let output = tokio::time::timeout(std::time::Duration::from_secs(45), crate::trial::generate(&p, &api_key, &input.prompt, input.max_output_tokens)).await.unwrap_or_else(|_|Err("Provider timed out; no paid credits consumed".into()));
            match output {
                Err(e) => {
                    requests.update_one(doc! {"_id":&rid,"status":"inflight"}, doc! {"$set":{"status":"failed","amount":0}}).await.map_err(unavailable)?;
                    Err(error(StatusCode::BAD_GATEWAY, &e))
                }
                Ok(result) => {
                    let amount = result["usage"]["completion_tokens"].as_u64().ok_or_else(||error(StatusCode::BAD_GATEWAY,"Invalid output usage"))?;
                    let mut session = app.db.client().start_session().await.map_err(unavailable)?;
                    session.start_transaction().await.map_err(unavailable)?;
                    app.db.collection::<Document>("credit_results").insert_one(doc! {"_id":&rid,"result":mongodb::bson::to_document(&result).map_err(unavailable)?,"expires_at":DateTime::from_millis((now()+900)*1000)}).session(&mut session).await.map_err(unavailable)?;
                    requests.update_one(doc! {"_id":&rid,"status":"inflight"},doc! {"$set":{"status":"consumed","amount":amount as i64,"usage":mongodb::bson::to_document(&result["usage"]).map_err(unavailable)?}}).session(&mut session).await.map_err(unavailable)?;
                    // An unknown commit result may already have committed. Retry the same commit, never inference.
                    let mut committed = false;
                    for _ in 0..3 {
                        match session.commit_transaction().await {
                            Ok(()) => {committed = true; break;}
                            Err(e) if e.contains_label("UnknownTransactionCommitResult") => continue,
                            Err(e) => return Err(unavailable(e)),
                        }
                    }
                    if !committed {return Err(error(StatusCode::SERVICE_UNAVAILABLE,"Settlement record commit is uncertain; recover this request before generating again"));}
                    Ok(Json(json!({"request_id":rid,"result":result,"consumed":amount,"settlement":"queued"})))
                }
            }
        }).await.unwrap_or_else(|_|Err(error(StatusCode::SERVICE_UNAVAILABLE,"Request interrupted; inspect saved request status before retrying. Any in-flight reservation remains held for reconciliation.")));
        crate::trial::release(app, &leases, &rid).await;
        return attempt;
    }
    Err(error(StatusCode::NOT_FOUND, "Credit endpoint not found"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn a(deposited: u128, withdrawn: u128, locked: u128) -> Account {
        Account {
            deposited,
            withdrawn,
            locked,
            burned: 0,
            withdrawal: 0,
            nonce: 0,
            approved: false,
            pending: false,
        }
    }
    #[test]
    fn pending_redeposit_cannot_reuse_credits_withdrawn_since_finality() {
        let f = a(1000, 0, 1000);
        let l = a(1500, 997, 500);
        assert_eq!(available(&f, &l, 0, 3), 0);
        assert_eq!(available(&a(1500, 997, 500), &l, 0, 3), 500);
    }
    #[test]
    fn consumption_is_deducted_once_across_burn_finality() {
        let mut f = a(1000, 0, 1000);
        let mut l = a(1000, 0, 997);
        l.burned = 3;
        assert_eq!(available(&f, &l, 128, 3), 869);
        f.burned = 3;
        f.locked = 997;
        assert_eq!(available(&f, &l, 0, 3), 997);
        assert_eq!(available(&f, &l, 0, 0), 0);
        l.pending = true;
        assert_eq!(available(&f, &l, 0, 3), 0);
    }
}
