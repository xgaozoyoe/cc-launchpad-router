//! Wallet-authorized maintenance and finalized per-project ledger cutover. Never broadcasts migration.
use crate::{
    credits as abi, error, modular, now, protocol_groups, stake, unavailable, ApiResult, App,
};
use axum::{http::StatusCode, Json};
use mongodb::bson::{doc, Document};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
fn conflict(e: impl AsRef<str>) -> (StatusCode, Json<Value>) {
    error(StatusCode::CONFLICT, e.as_ref())
}
async fn call(
    app: &App,
    to: &str,
    signature: &str,
    args: &[String],
    tag: &str,
) -> Result<String, String> {
    app.stake
        .as_ref()
        .ok_or("Chain unavailable")?
        .rpc(
            "eth_call",
            json!([{"to":to,"data":abi::data(signature,args)},tag]),
        )
        .await?
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "Invalid chain response".into())
}
pub async fn admin(
    app: &App,
    wallet: &str,
    operation: &str,
    p: &Document,
    payload: &Value,
) -> ApiResult {
    let id = p.get_str("_id").map_err(unavailable)?;
    let group_id = payload["group_id"]
        .as_str()
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Provide group_id"))?;
    let g = protocol_groups::select(app, group_id).map_err(conflict)?;
    let gate = g
        .migration_gate
        .as_deref()
        .ok_or_else(|| conflict("Migration gate not configured"))?;
    protocol_groups::audit(app, &g).await.map_err(conflict)?;
    let key = stake::project_key(id);
    let actual = call(app, gate, "projectId()", &[], "finalized")
        .await
        .map_err(conflict)?;
    if actual.to_lowercase() != key {
        return Err(conflict("Migration gate belongs to another project"));
    }
    let source = call(app, gate, "legacyCredits()", &[], "finalized")
        .await
        .map_err(conflict)?;
    let c = abi::config(app).map_err(conflict)?;
    if format!("0x{}", &abi::w(&source, 0).map_err(conflict)?[24..]) != c.contract {
        return Err(conflict("Legacy credit source mismatch"));
    }
    if operation == "migration-freeze" {
        if p.get_str("migration_phase").ok() == Some("frozen")
            && p.get_str("migration_target_group").ok() != Some(group_id)
        {
            return Err(conflict(
                "Revoke the old gate roles and resume before changing the migration target",
            ));
        }
        if p.get_str("contract_group").is_ok() {
            return Err(conflict("Project already migrated"));
        }
        let run = uuid::Uuid::new_v4().to_string();
        let lease = format!("project:{id}");
        crate::trial::take_lease(app, &lease, &run).await?;
        let result=async {
   let pending=app.db.collection::<Document>("credit_requests").find_one(doc!{"project_id":id,"status":{"$nin":["failed","settled"]}}).await.map_err(unavailable)?;
   if pending.is_some(){return Err(conflict("Resolve all requests and finalize burns before freezing"));}
   let changed=app.db.collection::<Document>("projects").update_one(doc!{"_id":id,"contract_group":{"$exists":false},"status":"accepted"},doc!{"$set":{"migration_phase":"frozen","migration_target_group":&g.id,"migration_frozen_at":now()}}).await.map_err(unavailable)?;
   if changed.matched_count!=1{return Err(conflict("Project must be an accepted legacy launch"));}Ok(Json(json!({"phase":"frozen","project_id":id,"group_id":g.id})))
  }.await;
        crate::trial::release(app, &[lease], &run).await;
        return result;
    }
    if p.get_str("migration_target_group").ok() != Some(group_id)
        || p.get_str("migration_phase").ok() != Some("frozen")
    {
        return Err(conflict("Freeze this project for the selected group first"));
    }
    if operation == "migration-resume" {
        let role = hex::encode(Keccak256::digest(b"SETTLER_ROLE"));
        let offer = hex::encode(Keccak256::digest(b"OFFER_MANAGER_ROLE"));
        for tag in ["finalized", "latest"] {
            let state = call(
                app,
                &c.staking_contract,
                "stakeState(bytes32)",
                &[key[2..].into()],
                tag,
            )
            .await
            .map_err(conflict)?;
            if abi::uint(abi::w(&state, 0).map_err(conflict)?).map_err(conflict)? != 3 {
                return Err(conflict(
                    "Legacy stake is no longer verified; it cannot be resumed",
                ));
            }
            if abi::uint(
                &call(app, gate, "committed()", &[], tag)
                    .await
                    .map_err(conflict)?,
            )
            .map_err(conflict)?
                != 0
            {
                return Err(conflict(
                    "Committed migration cannot resume the legacy ledger",
                ));
            }
            for (address, r) in [
                (&c.contract, &role),
                (&c.contract, &offer),
                (&c.staking_contract, &role),
            ] {
                if abi::uint(
                    &call(
                        app,
                        address,
                        "hasRole(bytes32,address)",
                        &[r.clone(), abi::addr(gate)],
                        tag,
                    )
                    .await
                    .map_err(conflict)?,
                )
                .map_err(conflict)?
                    != 0
                {
                    return Err(conflict("Revoke and finalize every legacy gate role before resuming; pending commits must fail"));
                }
            }
        }
        app.db.collection::<Document>("projects").update_one(doc!{"_id":id,"contract_group":{"$exists":false},"migration_phase":"frozen"},doc!{"$unset":{"migration_phase":"","migration_target_group":"","migration_frozen_at":""}}).await.map_err(unavailable)?;
        return Ok(Json(json!({"phase":"legacy-resumed"})));
    }
    if operation != "migration-activate" {
        return Err(error(StatusCode::NOT_FOUND, "Unknown migration action"));
    }
    let hash = payload["tx_hash"]
        .as_str()
        .filter(|s| {
            s.len() == 66 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit())
        })
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Provide migration tx_hash"))?;
    let rpc = app
        .stake
        .as_ref()
        .ok_or_else(|| conflict("Chain unavailable"))?;
    let receipt = rpc
        .rpc("eth_getTransactionReceipt", json!([hash]))
        .await
        .map_err(conflict)?;
    let head = rpc
        .rpc("eth_getBlockByNumber", json!(["finalized", false]))
        .await
        .map_err(conflict)?;
    if receipt["status"] != json!("0x1")
        || receipt["to"].as_str().map(str::to_lowercase).as_deref() != Some(gate)
    {
        return Err(conflict(
            "Migration receipt does not show a successful gate call",
        ));
    }
    let block = abi::uint(receipt["blockNumber"].as_str().unwrap_or("")).map_err(conflict)?;
    if block > abi::uint(head["number"].as_str().unwrap_or("")).map_err(conflict)? {
        return Err(conflict("Wait for migration finality"));
    }
    let canonical = rpc
        .rpc(
            "eth_getBlockByNumber",
            json!([receipt["blockNumber"], false]),
        )
        .await
        .map_err(conflict)?;
    if receipt["blockHash"] != canonical["hash"] {
        return Err(conflict("Migration receipt is not canonical"));
    }
    let topic = format!(
        "0x{}",
        hex::encode(Keccak256::digest(
            b"MigrationCommitted(bytes32,bytes32,uint256,uint256,uint256,uint256)"
        ))
    );
    let logs = receipt["logs"]
        .as_array()
        .ok_or_else(|| conflict("Missing migration event"))?;
    let matches: Vec<&Value> = logs
        .iter()
        .filter(|log| {
            log["address"].as_str().map(str::to_lowercase).as_deref() == Some(gate)
                && log["topics"][0] == json!(topic)
                && log["topics"][2] == json!(key)
        })
        .collect();
    if matches.len() != 1 {
        return Err(conflict("Expected exactly one project migration event"));
    }
    let plan = matches[0]["topics"][1]
        .as_str()
        .ok_or_else(|| conflict("Missing plan hash"))?;
    if call(app, gate, "committedPlan()", &[], "finalized")
        .await
        .map_err(conflict)?
        != plan
    {
        return Err(conflict("Committed plan mismatch"));
    }
    let state=app.db.collection::<Document>("protocol_group_state").find_one(doc!{"_id":&g.id,"manifest_digest":g.digest(),"halted":false,"block":{"$gte":block as i64}}).await.map_err(unavailable)?;
    if state.is_none() {
        return Err(conflict(
            "Wait for the new group monitor to index the migration",
        ));
    }
    let raw = modular::call(
        app,
        &g,
        "registry",
        "project(bytes32)",
        &[key[2..].into()],
        "finalized",
    )
    .await
    .map_err(conflict)?;
    let who = format!("0x{}", &abi::w(&raw, 0).map_err(conflict)?[24..]);
    let model = hex::encode(Keccak256::digest(
        p.get_str("model_id").unwrap_or("").as_bytes(),
    ));
    if who != p.get_str("wallet_address").unwrap_or("")
        || abi::w(&raw, 4).map_err(conflict)? != model
        || abi::uint(abi::w(&raw, 7).map_err(conflict)?).map_err(conflict)? != 3
        || abi::uint(abi::w(&raw, 1).map_err(conflict)?)
            .map_err(conflict)?
            .to_string()
            != p.get_str("stake_raw").unwrap_or("")
        || abi::uint(abi::w(&raw, 5).map_err(conflict)?).map_err(conflict)?
            != p.get_i64("staked_at").unwrap_or(0) as u128
        || abi::uint(abi::w(&raw, 6).map_err(conflict)?).map_err(conflict)?
            != p.get_i64("rpc_deadline").unwrap_or(0) as u128
    {
        return Err(conflict(
            "Imported terms, provider or original deadline differ",
        ));
    }
    let mut session = app.db.client().start_session().await.map_err(unavailable)?;
    session.start_transaction().await.map_err(unavailable)?;
    let changed=app.db.collection::<Document>("projects").update_one(doc!{"_id":id,"contract_group":{"$exists":false},"migration_phase":"frozen","migration_target_group":&g.id},doc!{"$set":{"contract_group":&g.id,"ledger_epoch":plan,"migration_finalized":true,"migration_phase":"active","migration_tx_hash":hash,"active_stake_vault":&g.contracts["stake_vault"],"status":"accepted"}}).session(&mut session).await.map_err(unavailable)?;
    if changed.modified_count != 1 {
        return Err(conflict("Project migration state changed; refresh"));
    }
    app.db.collection::<Document>("migration_audit").insert_one(doc!{"project_id":id,"group_id":&g.id,"plan_hash":plan,"tx_hash":hash,"block":block as i64,"wallet":wallet,"operation":"activate","at":now()}).session(&mut session).await.map_err(unavailable)?;
    let mut committed = false;
    for _ in 0..3 {
        match session.commit_transaction().await {
            Ok(()) => {
                committed = true;
                break;
            }
            Err(e) if e.contains_label("UnknownTransactionCommitResult") => continue,
            Err(e) => return Err(unavailable(e)),
        }
    }
    if !committed {
        return Err(conflict(
            "Cutover result uncertain; refresh project state instead of repeating migration",
        ));
    }
    Ok(Json(
        json!({"phase":"active","group_id":g.id,"ledger_epoch":plan,"project_id":id}),
    ))
}

fn plan_call(holders: &[String], checkpoint: u128, hash: &str) -> String {
    let mut words = vec![
        abi::word(96),
        abi::word(checkpoint),
        hash.trim_start_matches("0x").into(),
        abi::word(holders.len() as u128),
    ];
    words.extend(holders.iter().map(|w| abi::addr(w)));
    abi::data("preview(address[],uint256,bytes32)", &words)
}
async fn current_plan(
    app: &App,
    p: &Document,
) -> Result<(protocol_groups::Group, Document), (StatusCode, Json<Value>)> {
    let group = p.get_str("migration_target_group").map_err(unavailable)?;
    let g = protocol_groups::select(app, group).map_err(conflict)?;
    let plan=app.db.collection::<Document>("migration_plans").find_one(doc!{"project_id":p.get_str("_id").map_err(unavailable)?,"group_id":group,"active":true,"expires_at":{"$gt":now()}}).await.map_err(unavailable)?.ok_or_else(||conflict("No current migration consent plan; ask the admin to prepare a fresh one"))?;
    let raw=app.stake.as_ref().unwrap().rpc("eth_call",json!([{"to":g.migration_gate,"data":plan.get_str("preview_calldata").map_err(unavailable)?},"latest"])).await.map_err(conflict)?;
    if format!(
        "0x{}",
        abi::w(raw.as_str().unwrap_or(""), 0).map_err(conflict)?
    ) != plan.get_str("_id").map_err(unavailable)?
    {
        return Err(conflict(
            "Source balances or checkpoint changed; create a new consent plan",
        ));
    }
    Ok((g, plan))
}
pub(crate) async fn plan_admin(
    app: &App,
    wallet: &str,
    operation: &str,
    p: &Document,
    payload: &Value,
) -> ApiResult {
    let id = p.get_str("_id").map_err(unavailable)?;
    if p.get_str("migration_phase").ok() != Some("frozen") || p.get_str("contract_group").is_ok() {
        return Err(conflict(
            "Freeze the legacy project before preparing consent",
        ));
    }
    if operation == "migration-prepare" {
        let (g, plan) = current_plan(app, p).await?;
        let gate = g
            .migration_gate
            .as_deref()
            .ok_or_else(|| conflict("Missing gate"))?;
        let holders: Vec<String> = plan
            .get_array("holders")
            .map_err(unavailable)?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| conflict("Invalid stored holder"))
            })
            .collect::<Result<_, _>>()?;
        let mut signatures = vec![];
        for holder in &holders {
            let consent=app.db.collection::<Document>("migration_consents").find_one(doc!{"_id":format!("{}:{}:{holder}",plan.get_str("_id").map_err(unavailable)?,plan.get_i64("chain_expiry").map_err(unavailable)?)}).await.map_err(unavailable)?.ok_or_else(||conflict("Not every holder has signed the current migration plan"))?;
            signatures.push(
                consent
                    .get_str("signature")
                    .map_err(unavailable)?
                    .to_string(),
            );
        }
        let holder_text = format!("[{}]", holders.join(","));
        let signature_text = format!("[{}]", signatures.join(","));
        let data = crate::credit_worker::command(vec![
            "calldata".into(),
            "commit(address[],uint256,bytes32,uint64,bytes[])".into(),
            holder_text,
            plan.get_i64("checkpoint").map_err(unavailable)?.to_string(),
            plan.get_str("checkpoint_hash").map_err(unavailable)?.into(),
            plan.get_i64("chain_expiry")
                .map_err(unavailable)?
                .to_string(),
            signature_text,
        ])
        .await
        .map_err(conflict)?;
        let transaction = json!({"from":wallet,"to":gate,"chainId":format!("0x{:x}",g.chain_id),"value":"0x0","data":data});
        app.stake.as_ref().unwrap().rpc("eth_call",json!([transaction,"latest"])).await.map_err(|_|conflict("Migration simulation failed: check funding, exact PRL allowance, legacy gate roles and current consents"))?;
        return Ok(Json(
            json!({"transaction":transaction,"gate":gate,"plan_hash":plan.get_str("_id").ok(),"simulation":"passed","warning":"This atomic transaction irreversibly retires V1 credits and transfers PRL into the new Vault; wait for finality before activating the ledger"}),
        ));
    }
    let group_id = payload["group_id"]
        .as_str()
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Provide group_id"))?;
    if p.get_str("migration_target_group").ok() != Some(group_id) {
        return Err(conflict("Frozen target group mismatch"));
    }
    let g = protocol_groups::select(app, group_id).map_err(conflict)?;
    protocol_groups::audit(app, &g).await.map_err(conflict)?;
    let gate = g
        .migration_gate
        .as_deref()
        .ok_or_else(|| conflict("Missing gate"))?;
    let input = payload["holders"].as_str().ok_or_else(|| {
        error(
            StatusCode::BAD_REQUEST,
            "Provide holder addresses separated by commas or whitespace",
        )
    })?;
    let mut holders: Vec<String> = input
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(stake::address)
        .collect::<Result<_, _>>()
        .map_err(conflict)?;
    holders.sort();
    if holders.is_empty() || holders.len() > 32 || holders.windows(2).any(|v| v[0] == v[1]) {
        return Err(conflict("Provide every distinct legacy holder, at most 32"));
    }
    let rpc = app.stake.as_ref().unwrap();
    let head = rpc
        .rpc("eth_getBlockByNumber", json!(["finalized", false]))
        .await
        .map_err(conflict)?;
    let checkpoint = abi::uint(head["number"].as_str().unwrap_or(""))
        .map_err(conflict)?
        .checked_sub(1)
        .ok_or_else(|| conflict("No finalized checkpoint"))?;
    let block = rpc
        .rpc(
            "eth_getBlockByNumber",
            json!([format!("0x{checkpoint:x}"), false]),
        )
        .await
        .map_err(conflict)?;
    let hash = block["hash"]
        .as_str()
        .ok_or_else(|| conflict("Missing checkpoint hash"))?;
    let data = plan_call(&holders, checkpoint, hash);
    let latest = rpc
        .rpc("eth_call", json!([{"to":gate,"data":data},"latest"]))
        .await
        .map_err(conflict)?;
    let finalized = rpc
        .rpc("eth_call", json!([{"to":gate,"data":data},"finalized"]))
        .await
        .map_err(conflict)?;
    if latest != finalized {
        return Err(conflict(
            "Wait for all source changes to finalize before creating a consent plan",
        ));
    }
    let raw = latest
        .as_str()
        .ok_or_else(|| conflict("Invalid plan preview"))?;
    let plan_hash = format!("0x{}", abi::w(raw, 0).map_err(conflict)?);
    let chain_head = rpc
        .rpc("eth_getBlockByNumber", json!(["latest", false]))
        .await
        .map_err(conflict)?;
    let expiry = abi::uint(chain_head["timestamp"].as_str().unwrap_or("")).map_err(conflict)? + 600;
    let mut session = app.db.client().start_session().await.map_err(unavailable)?;
    session.start_transaction().await.map_err(unavailable)?;
    app.db
        .collection::<Document>("migration_plans")
        .update_many(
            doc! {"project_id":id,"active":true},
            doc! {"$set":{"active":false}},
        )
        .session(&mut session)
        .await
        .map_err(unavailable)?;
    app.db.collection::<Document>("migration_plans").update_one(doc!{"_id":&plan_hash},doc!{"$set":{"project_id":id,"group_id":&g.id,"holders":holders,"checkpoint":checkpoint as i64,"checkpoint_hash":hash,"preview_calldata":data,"chain_expiry":expiry as i64,"expires_at":now()+600,"active":true,"at":now(),"actor":wallet}}).upsert(true).session(&mut session).await.map_err(unavailable)?;
    session.commit_transaction().await.map_err(unavailable)?;
    Ok(Json(
        json!({"plan_hash":plan_hash,"remaining_credits":abi::uint(abi::w(raw,1).map_err(conflict)?).map_err(conflict)?.to_string(),"historical_delivery":abi::uint(abi::w(raw,2).map_err(conflict)?).map_err(conflict)?.to_string(),"replacement_reserve_wei":abi::uint(abi::w(raw,3).map_err(conflict)?).map_err(conflict)?.to_string(),"expires_at":now()+600}),
    ))
}
fn recover_digest(hash: &str, signature: &str) -> Result<String, String> {
    use k256::ecdsa::{RecoveryId, Signature, VerifyingKey};
    let raw = hex::decode(signature.trim_start_matches("0x")).map_err(|_| "Invalid signature")?;
    if raw.len() != 65 {
        return Err("Invalid signature".into());
    }
    let recovery = match raw[64] {
        27 | 28 => raw[64] - 27,
        0 | 1 => raw[64],
        _ => return Err("Invalid recovery ID".into()),
    };
    let sig = Signature::from_slice(&raw[..64]).map_err(|_| "Invalid signature")?;
    if sig.normalize_s().is_some() {
        return Err("Noncanonical signature".into());
    }
    let digest = hex::decode(hash.trim_start_matches("0x")).map_err(|_| "Invalid digest")?;
    let key = VerifyingKey::recover_from_prehash(
        &digest,
        &sig,
        RecoveryId::from_byte(recovery).ok_or("Invalid recovery ID")?,
    )
    .map_err(|_| "Invalid signer")?;
    Ok(format!(
        "0x{}",
        hex::encode(&Keccak256::digest(&key.to_encoded_point(false).as_bytes()[1..])[12..])
    ))
}
pub(crate) async fn consent(
    app: &App,
    user: &str,
    p: &Document,
    action: &str,
    method: &str,
    body: &[u8],
) -> ApiResult {
    if p.get_str("migration_phase").ok() != Some("frozen") {
        return Err(conflict("Project is not awaiting migration consent"));
    }
    let wallet = abi::wallet(app, user).await?;
    let (g, plan) = current_plan(app, p).await?;
    if !plan
        .get_array("holders")
        .map_err(unavailable)?
        .iter()
        .any(|h| h.as_str() == Some(&wallet))
    {
        return Err(error(
            StatusCode::NOT_FOUND,
            "This wallet is not a holder in the current plan",
        ));
    }
    let hash = plan.get_str("_id").map_err(unavailable)?;
    let gate = g
        .migration_gate
        .as_deref()
        .ok_or_else(|| conflict("Missing gate"))?;
    if action == "consent" && method == "POST" {
        let input: Value = serde_json::from_slice(body)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide signature and plan_hash"))?;
        if input["plan_hash"].as_str() != Some(hash) {
            return Err(conflict("Migration plan changed"));
        }
        let signature = input["signature"]
            .as_str()
            .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Provide signature"))?;
        let digest = call(
            app,
            gate,
            "consentDigest(bytes32,uint64)",
            &[
                hash[2..].into(),
                abi::word(plan.get_i64("chain_expiry").map_err(unavailable)? as u128),
            ],
            "latest",
        )
        .await
        .map_err(conflict)?;
        if recover_digest(&digest, signature).map_err(conflict)? != wallet {
            return Err(error(
                StatusCode::FORBIDDEN,
                "Consent was not signed by this holder wallet",
            ));
        }
        app.db.collection::<Document>("migration_consents").update_one(doc!{"_id":format!("{hash}:{}:{wallet}",plan.get_i64("chain_expiry").map_err(unavailable)?)},doc!{"$setOnInsert":{"plan_hash":hash,"wallet":&wallet,"signature":signature,"at":now()}}).upsert(true).await.map_err(unavailable)?;
        return Ok(Json(json!({"signed":true,"plan_hash":hash})));
    }
    if action != "consent-plan" || method != "GET" {
        return Err(error(
            StatusCode::METHOD_NOT_ALLOWED,
            "Use GET consent-plan or POST consent",
        ));
    }
    let c = abi::config(app).map_err(conflict)?;
    let a = abi::account(
        app,
        &c,
        p.get_str("_id").map_err(unavailable)?,
        &wallet,
        "finalized",
    )
    .await
    .map_err(conflict)?;
    let signed=app.db.collection::<Document>("migration_consents").find_one(doc!{"_id":format!("{hash}:{}:{wallet}",plan.get_i64("chain_expiry").map_err(unavailable)?)}).await.map_err(unavailable)?.is_some();
    Ok(Json(
        json!({"plan_hash":hash,"gate":gate,"group_id":g.id,"chain_id":g.chain_id,"chain_expiry":plan.get_i64("chain_expiry").ok(),"expires_at":plan.get_i64("expires_at").ok(),"wallet":wallet,"remaining_credits":a.locked.to_string(),"historical_consumed":a.burned.to_string(),"signed":signed,"checkpoint_hash":plan.get_str("checkpoint_hash").ok(),"new_usage_escrow":g.contracts["usage_escrow"],"new_stake_vault":g.contracts["stake_vault"]}),
    ))
}

/// Fixed governance operation only: bind the configured gate once. Never exposes generic execute.
pub(crate) async fn governance(
    app: &App,
    wallet: &str,
    p: &Document,
    payload: &Value,
) -> ApiResult {
    let group_id = payload["group_id"]
        .as_str()
        .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Provide group_id"))?;
    let g = protocol_groups::select(app, group_id).map_err(conflict)?;
    let gate = g
        .migration_gate
        .as_deref()
        .ok_or_else(|| conflict("Migration gate not configured in manifest"))?;
    let mut core = g.clone();
    core.migration_gate = None;
    core.migration_runtime_hash = None;
    protocol_groups::audit(app, &core).await.map_err(conflict)?;
    let rpc = app
        .stake
        .as_ref()
        .ok_or_else(|| conflict("Chain unavailable"))?;
    let code = rpc
        .rpc("eth_getCode", json!([gate, "finalized"]))
        .await
        .map_err(conflict)?;
    let raw = hex::decode(
        code.as_str()
            .and_then(|s| s.strip_prefix("0x"))
            .unwrap_or(""),
    )
    .map_err(|_| conflict("Invalid gate bytecode"))?;
    if raw.is_empty()
        || Some(format!("0x{}", hex::encode(Keccak256::digest(raw)))) != g.migration_runtime_hash
    {
        return Err(conflict(
            "Wait for reviewed migration-gate deployment finality",
        ));
    }
    let key = stake::project_key(p.get_str("_id").map_err(unavailable)?);
    if call(app, gate, "projectId()", &[], "finalized")
        .await
        .map_err(conflict)?
        != key
    {
        return Err(conflict("Gate belongs to another project"));
    }
    for (signature, want) in [
        ("protocol()", g.contracts["factory"].as_str()),
        ("legacyStake()", rpc.config.staking_contract.as_str()),
    ] {
        let value = call(app, gate, signature, &[], "finalized")
            .await
            .map_err(conflict)?;
        if format!("0x{}", &abi::w(&value, 0).map_err(conflict)?[24..]) != want {
            return Err(conflict("Gate source/target mismatch"));
        }
    }
    let bound = call(
        app,
        &g.contracts["factory"],
        "migrationGate()",
        &[],
        "latest",
    )
    .await
    .map_err(conflict)?;
    if abi::w(&bound, 0)
        .map_err(conflict)?
        .bytes()
        .any(|b| b != b'0')
    {
        return Err(conflict(
            "The one-time migration gate is already configured",
        ));
    }
    let configure = abi::data("configureMigration(address)", &[abi::addr(gate)]);
    let salt = format!(
        "0x{}",
        hex::encode(Keccak256::digest(
            format!(
                "bind-migration:{}:{}:{gate}",
                g.chain_id, g.contracts["factory"]
            )
            .as_bytes()
        ))
    );
    let zero = format!("0x{}", "0".repeat(64));
    let cast = |sig: &str, args: Vec<String>| {
        let mut out = vec!["calldata".into(), sig.into()];
        out.extend(args);
        out
    };
    let operation_data = crate::credit_worker::command(cast(
        "hashOperation(address,uint256,bytes,bytes32,bytes32)",
        vec![
            g.contracts["factory"].clone(),
            "0".into(),
            configure.clone(),
            zero.clone(),
            salt.clone(),
        ],
    ))
    .await
    .map_err(conflict)?;
    let op = rpc
        .rpc(
            "eth_call",
            json!([{"to":g.contracts["governance"],"data":operation_data},"latest"]),
        )
        .await
        .map_err(conflict)?;
    let op_id = op
        .as_str()
        .ok_or_else(|| conflict("Invalid operation hash"))?;
    let ready_at = call(
        app,
        &g.contracts["governance"],
        "getTimestamp(bytes32)",
        &[op_id[2..].into()],
        "latest",
    )
    .await
    .map_err(conflict)?;
    let timestamp = abi::uint(&ready_at).map_err(conflict)?;
    let step = payload["step"].as_str().unwrap_or("");
    let (role, signature, args) = if step == "schedule" {
        if timestamp != 0 {
            return Err(conflict(
                "This governance operation is already scheduled; wait for readiness",
            ));
        }
        (
            "PROPOSER_ROLE",
            "schedule(address,uint256,bytes,bytes32,bytes32,uint256)",
            vec![
                g.contracts["factory"].clone(),
                "0".into(),
                configure,
                zero,
                salt,
                "172800".into(),
            ],
        )
    } else if step == "execute" {
        let ready = call(
            app,
            &g.contracts["governance"],
            "isOperationReady(bytes32)",
            &[op_id[2..].into()],
            "latest",
        )
        .await
        .map_err(conflict)?;
        if abi::uint(&ready).map_err(conflict)? != 1 {
            return Err(conflict(format!(
                "Governance operation not ready; chain unlock timestamp: {timestamp}"
            )));
        }
        (
            "EXECUTOR_ROLE",
            "execute(address,uint256,bytes,bytes32,bytes32)",
            vec![
                g.contracts["factory"].clone(),
                "0".into(),
                configure,
                zero,
                salt,
            ],
        )
    } else {
        return Err(error(StatusCode::BAD_REQUEST, "Use schedule or execute"));
    };
    let role = hex::encode(Keccak256::digest(role.as_bytes()));
    let allowed = call(
        app,
        &g.contracts["governance"],
        "hasRole(bytes32,address)",
        &[role, abi::addr(wallet)],
        "finalized",
    )
    .await
    .map_err(conflict)?;
    if abi::uint(&allowed).map_err(conflict)? != 1 {
        return Err(error(
            StatusCode::FORBIDDEN,
            "Connected admin wallet does not have this governance role",
        ));
    }
    let data = crate::credit_worker::command(cast(signature, args))
        .await
        .map_err(conflict)?;
    Ok(Json(
        json!({"transaction":{"from":wallet,"to":g.contracts["governance"],"chainId":format!("0x{:x}",g.chain_id),"value":"0x0","data":data},"governance":g.contracts["governance"],"operation_id":op_id,"ready_at":timestamp,"step":step,"delay_seconds":172800}),
    ))
}
