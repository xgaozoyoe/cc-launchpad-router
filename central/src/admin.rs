//! Wallet-authorized review. Challenges bind the account, operation and exact payload.
use crate::{error, now, stake, unavailable, ApiResult, App};
use axum::{http::StatusCode, Json};
use futures_util::TryStreamExt;
use mongodb::bson::{doc, Document};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use sha3::Keccak256;
fn denied(message: &str) -> (StatusCode, Json<Value>) {
    error(StatusCode::FORBIDDEN, message)
}
fn text<'a>(v: &'a Value, key: &str) -> Result<&'a str, (StatusCode, Json<Value>)> {
    v[key]
        .as_str()
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Missing field"))
}
fn admin_address() -> Result<String, (StatusCode, Json<Value>)> {
    stake::address(&std::env::var("ADMIN_WALLET_ADDRESS").unwrap_or_default()).map_err(|_| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Admin wallet is not configured",
        )
    })
}
fn digest(v: &str) -> String {
    hex::encode(Sha256::digest(v.as_bytes()))
}
fn calldata(id: &str) -> String {
    format!(
        "0x{}{}",
        hex::encode(&Keccak256::digest(b"verifyProject(bytes32)")[..4]),
        &stake::project_key(id)[2..]
    )
}
async fn verifier(app: &App, wallet: &str) -> Result<(), (StatusCode, Json<Value>)> {
    let s = app
        .stake
        .as_ref()
        .ok_or_else(|| denied("Chain is not configured"))?;
    let data = format!(
        "0x{}{}{:0>64}",
        hex::encode(&Keccak256::digest(b"hasRole(bytes32,address)")[..4]),
        hex::encode(Keccak256::digest(b"VERIFIER_ROLE")),
        &wallet[2..]
    );
    let result = s
        .rpc(
            "eth_call",
            json!([{"to":s.config.staking_contract,"data":data},"latest"]),
        )
        .await
        .map_err(|_| denied("Cannot check verifier role"))?;
    if result
        .as_str()
        .and_then(|x| u128::from_str_radix(x.trim_start_matches("0x"), 16).ok())
        != Some(1)
    {
        return Err(denied("Admin wallet does not hold VERIFIER_ROLE"));
    }
    Ok(())
}
async fn ready(app: &App, p: &Document) -> Result<(), (StatusCode, Json<Value>)> {
    crate::deployment::eligible(app, p)
        .await
        .map_err(|e| denied(&e))?;
    if p.get_str("status").ok() != Some("awaiting-rpc") {
        return Err(denied("Project is not awaiting acceptance"));
    }
    let chain = app
        .stake
        .as_ref()
        .ok_or_else(|| denied("Chain is not configured"))?;
    let state = chain
        .state(p.get_str("onchain_key").map_err(unavailable)?, "latest")
        .await
        .map_err(|_| denied("Cannot read stake state"))?;
    if state.0 != 2 {
        return Err(denied("Stake is no longer awaiting verification"));
    }
    let report = app
        .db
        .collection::<Document>("rpc_checks")
        .find_one(doc! {"_id":p.get_str("_id").unwrap_or("")})
        .await
        .map_err(unavailable)?;
    let valid = report
        .as_ref()
        .and_then(|r| r.get_document("report").ok())
        .is_some_and(|r| {
            r.get_str("status").ok() == Some("passed")
                && r.get_str("provider_id").ok() == p.get_str("provider_id").ok()
                && r.get_i64("checked_at")
                    .map(|at| at <= now() && at > now() - 3600)
                    .unwrap_or(false)
        });
    if !valid {
        return Err(denied(
            "Run a passing Test RPC within the last hour before approval",
        ));
    }
    Ok(())
}
pub async fn handle(app: &App, user: &str, action: &str, method: &str, body: &[u8]) -> ApiResult {
    if method != "POST" {
        return Err(error(StatusCode::METHOD_NOT_ALLOWED, "Use POST"));
    }
    let wallet = admin_address()?;
    let input: Value =
        serde_json::from_slice(body).map_err(|_| error(StatusCode::BAD_REQUEST, "Invalid JSON"))?;
    let s = app
        .stake
        .as_ref()
        .ok_or_else(|| denied("Chain is not configured"))?;
    if action == "challenge" {
        let intent = text(&input, "intent")?;
        if ![
            "login",
            "capacity",
            "prepare",
            "submit",
            "retest",
            "migration-freeze",
            "migration-resume",
            "migration-activate",
            "migration-plan",
            "migration-prepare",
            "migration-governance",
        ]
        .contains(&intent)
        {
            return Err(denied("Unknown admin operation"));
        }
        let payload = input.get("payload").cloned().unwrap_or(json!({}));
        if intent.starts_with("migration-") {
            crate::migration::validate_project(app, &payload)
                .await
                .map_err(|e| denied(&e))?;
        }
        let payload_json = serde_json::to_string(&payload).map_err(unavailable)?;
        let id = uuid::Uuid::new_v4().to_string();
        let expiry = now() + 300;
        let origin = std::env::var("APP_ORIGIN").map_err(unavailable)?;
        let message=format!("AI Computing Credit Launchpad — Admin authorization\nOrigin: {origin}\nAccount: {user}\nAdmin wallet: {wallet}\nChain: {}\nContract: {}\nOperation: {intent}\nPayload: {payload_json}\nNonce: {id}\nExpires (Unix): {expiry}\nThis signature authorizes only this operation. It does not submit a blockchain transaction.",s.config.chain_id,s.config.staking_contract);
        app.db.collection::<Document>("admin_challenges").insert_one(doc!{"_id":&id,"user":user,"wallet":&wallet,"intent":intent,"chain_id":s.config.chain_id as i64,"contract":&s.config.staking_contract,"origin":&origin,"payload":payload_json,"message":&message,"expires_at":expiry,"used":false}).await.map_err(unavailable)?;
        return Ok(Json(
            json!({"challenge_id":id,"message":message,"admin_address":wallet,"chain_id":s.config.chain_id,"contract":s.config.staking_contract}),
        ));
    }
    if action == "query" || action == "protocol" || action == "migration-status" {
        let token = text(&input, "token")?;
        let valid = app
            .db
            .collection::<Document>("admin_sessions")
            .find_one(
                doc! {"_id":digest(token),"user":user,"wallet":&wallet,"expires_at":{"$gt":now()}},
            )
            .await
            .map_err(unavailable)?;
        if valid.is_none() {
            return Err(denied("Connect and sign with the admin wallet"));
        }
        if action == "migration-status" {
            return crate::migration::status(app, &wallet, &input).await;
        }
        if action == "protocol" {
            return Ok(Json(crate::protocol_groups::report(app).await));
        }
        let mut rows=app.db.collection::<Document>("projects").find(doc!{}).projection(doc!{"_id":1,"name":1,"model_id":1,"status":1,"stake_finalized":1,"provider_id":1,"wallet_address":1,"launch_id":1,"rpc_deadline":1,"onchain_key":1,"capacity_approval":1,"verification_tx_hash":1,"contract_group":1,"ledger_epoch":1,"migration_phase":1,"migration_target_group":1}).sort(doc!{"created_at":-1}).limit(100).await.map_err(unavailable)?.try_collect::<Vec<_>>().await.map_err(unavailable)?;
        crate::project_progress(app, &mut rows)
            .await
            .map_err(unavailable)?;
        let groups = crate::protocol_groups::load(s.config.chain_id).unwrap_or_default();
        for group in &groups {
            if let Ok(key) = crate::migration::target_key(app, group).await {
                for p in &mut rows {
                    if p.get_str("_id")
                        .ok()
                        .is_some_and(|id| crate::migration::project_matches(id, &key))
                    {
                        p.insert("migration_available_group", group.id.clone());
                    }
                }
            }
        }
        for p in &mut rows {
            let key = stake::project_key(p.get_str("_id").unwrap_or(""));
            match s.state(&key, "finalized").await {
                Ok((state, amount, _, _, _)) => {
                    p.insert("migration_stake_state", state as i32);
                    p.insert(
                        "migration_prl_principal_wei",
                        if state == 2 || state == 3 {
                            amount.to_string()
                        } else {
                            "0".into()
                        },
                    );
                }
                Err(_) => {
                    p.insert(
                        "migration_inventory_error",
                        "Unable to read finalized legacy stake",
                    );
                }
            }
            if let Ok(offer) = crate::credits::offer(app, p, "finalized").await {
                p.insert("migration_credit_issued", offer.issued.to_string());
                p.insert("migration_credit_commitment", offer.supply.to_string());
            }
        }
        for p in &mut rows {
            let positions = app
                .db
                .collection::<Document>("credit_positions")
                .find(doc! {"project_id":p.get_str("_id").unwrap_or("")})
                .projection(doc! {"wallet":1})
                .limit(32)
                .await
                .map_err(unavailable)?
                .try_collect::<Vec<_>>()
                .await
                .map_err(unavailable)?;
            p.insert(
                "migration_holders",
                positions
                    .iter()
                    .filter_map(|r| r.get_str("wallet").ok().map(str::to_owned))
                    .collect::<Vec<_>>(),
            );
        }
        for p in &mut rows {
            p.insert(
                "rpc_retest_available",
                crate::rpc_credentials::load(p).is_ok(),
            );
        }
        return Ok(Json(
            json!({"projects":rows,"admin_address":wallet,"chain_id":s.config.chain_id,"contract":s.config.staking_contract,"credit_contract":crate::credits::config(app).ok().map(|c|c.contract),"sales_enabled":false,"candidate_groups":crate::protocol_groups::load(s.config.chain_id).unwrap_or_default().into_iter().map(|g|g.id).collect::<Vec<_>>()}),
        ));
    }
    let id = text(&input, "challenge_id")?;
    let c = app
        .db
        .collection::<Document>("admin_challenges")
        .find_one(
            doc! {"_id":id,"user":user,"wallet":&wallet,"expires_at":{"$gt":now()},"used":false},
        )
        .await
        .map_err(unavailable)?
        .ok_or_else(|| denied("Authorization expired or already used"))?;
    if c.get_i64("chain_id").ok() != Some(s.config.chain_id as i64)
        || c.get_str("contract").ok() != Some(s.config.staking_contract.as_str())
        || c.get_str("origin").ok() != std::env::var("APP_ORIGIN").ok().as_deref()
    {
        return Err(denied("Configuration changed; sign a new authorization"));
    }
    let intent = if action == "authenticate" {
        "login"
    } else {
        action
    };
    if c.get_str("intent").ok() != Some(intent) {
        return Err(denied("Authorization operation mismatch"));
    }
    let recovered = stake::recover(
        c.get_str("message").map_err(unavailable)?,
        text(&input, "signature")?,
    )
    .map_err(|_| denied("Invalid admin signature"))?;
    if recovered != wallet {
        return Err(denied(
            "Signature was not sent by the configured admin wallet",
        ));
    }
    let consumed = app
        .db
        .collection::<Document>("admin_challenges")
        .update_one(doc! {"_id":id,"used":false}, doc! {"$set":{"used":true}})
        .await
        .map_err(unavailable)?;
    if consumed.modified_count != 1 {
        return Err(denied("Authorization already used"));
    }
    if action == "authenticate" {
        let token = format!("{}{}", uuid::Uuid::new_v4(), uuid::Uuid::new_v4());
        app.db
            .collection::<Document>("admin_sessions")
            .insert_one(
                doc! {"_id":digest(&token),"user":user,"wallet":&wallet,"expires_at":now()+900},
            )
            .await
            .map_err(unavailable)?;
        return Ok(Json(json!({"token":token,"admin_address":wallet})));
    }
    let payload: Value =
        serde_json::from_str(c.get_str("payload").map_err(unavailable)?).map_err(unavailable)?;
    let project = text(&payload, "project_id")?;
    let p = app
        .db
        .collection::<Document>("projects")
        .find_one(doc! {"_id":project})
        .await
        .map_err(unavailable)?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Project not found"))?;
    if action.starts_with("migration-") {
        app.db.collection::<Document>("admin_audit").insert_one(doc!{"project_id":project,"wallet":&wallet,"account":user,"operation":action,"challenge_id":id,"at":now(),"stage":"requested"}).await.map_err(unavailable)?;
    }
    if action == "migration-governance" {
        return crate::migration::governance(app, &wallet, &p, &payload).await;
    }
    if ["migration-plan", "migration-prepare"].contains(&action) {
        return crate::migration::plan_admin(app, &wallet, action, &p, &payload).await;
    }
    if ["migration-freeze", "migration-resume", "migration-activate"].contains(&action) {
        return crate::migration::admin(app, &wallet, action, &p, &payload).await;
    }
    if action == "retest" {
        let key = crate::rpc_credentials::load(&p).map_err(|e| error(StatusCode::CONFLICT, &e))?;
        app.db.collection::<Document>("admin_audit").insert_one(doc!{"project_id":project,"operation":"rpc-retest","wallet":&wallet,"account":user,"at":now(),"challenge_id":id}).await.map_err(unavailable)?;
        return crate::rpc_check::run(app, &p, key).await;
    }
    if action == "capacity" {
        ready(app, &p).await?;
        let reason = text(&payload, "reason")?.trim();
        if reason.len() < 8 || reason.len() > 1000 || payload["manual_demo"] != true {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Confirm manual demo approval and provide a reason (8–1000 characters)",
            ));
        }
        let approval = doc! {"wallet":&wallet,"account":user,"at":now(),"reason":reason,"type":"manual-demo-not-benchmarked","provider_id":p.get_str("provider_id").unwrap_or("")};
        app.db.collection::<Document>("admin_audit").insert_one(doc!{"project_id":project,"operation":"capacity","approval":&approval,"challenge_id":id}).await.map_err(unavailable)?;
        app.db
            .collection::<Document>("projects")
            .update_one(
                doc! {"_id":project,"status":"awaiting-rpc"},
                doc! {"$set":{"capacity_approval":approval}},
            )
            .await
            .map_err(unavailable)?;
        return Ok(Json(
            json!({"approved":true,"benchmarked":false,"sales_enabled":false}),
        ));
    }
    if action == "prepare" {
        ready(app, &p).await?;
        verifier(app, &wallet).await?;
        if !p.get_document("capacity_approval").ok().is_some_and(|a| {
            a.get_str("provider_id").ok() == p.get_str("provider_id").ok()
                && a.get_str("wallet").ok() == Some(wallet.as_str())
        }) {
            return Err(denied("Manual capacity approval is required"));
        }
        return Ok(Json(
            json!({"transaction":{"from":wallet,"to":s.config.staking_contract,"data":calldata(project),"value":"0x0","chainId":format!("0x{:x}",s.config.chain_id)},"project_key":stake::project_key(project),"deadline":p.get_i64("rpc_deadline").ok()}),
        ));
    }
    if action == "submit" {
        let h = text(&payload, "tx_hash")?;
        if h.len() != 66 || !h.starts_with("0x") || !h[2..].bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(denied("Invalid transaction hash"));
        }
        verifier(app, &wallet).await?;
        if p.get_document("capacity_approval").is_err() {
            return Err(denied("Capacity approval is required"));
        }
        let tx = s
            .rpc("eth_getTransactionByHash", json!([h]))
            .await
            .map_err(|_| denied("Transaction unavailable; retry tracking the same hash"))?;
        let eq = |key: &str, expected: &str| {
            tx[key]
                .as_str()
                .is_some_and(|v| v.eq_ignore_ascii_case(expected))
        };
        if !eq("from", &wallet)
            || !eq("to", &s.config.staking_contract)
            || !eq("input", &calldata(project))
            || tx["value"].as_str() != Some("0x0")
            || !eq("chainId", &format!("0x{:x}", s.config.chain_id))
        {
            return Err(denied(
                "Transaction signer, chain, contract or project does not match",
            ));
        }
        app.db.collection::<Document>("admin_audit").insert_one(doc!{"project_id":project,"operation":"verify","wallet":&wallet,"account":user,"at":now(),"tx_hash":h,"challenge_id":id}).await.map_err(unavailable)?;
        app.db
            .collection::<Document>("projects")
            .update_one(
                doc! {"_id":project,"status":{"$in":["awaiting-rpc","verification-pending"]}},
                doc! {"$set":{"verification_tx_hash":h,"status":"verification-pending"}},
            )
            .await
            .map_err(unavailable)?;
        return Ok(Json(
            json!({"tx_hash":h,"status":"verification-pending","sales_enabled":false}),
        ));
    }
    Err(error(StatusCode::NOT_FOUND, "Unknown admin operation"))
}
