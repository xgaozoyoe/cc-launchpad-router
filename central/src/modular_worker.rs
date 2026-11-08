//! Finalized contract-group indexing and durable modular settlement, sharing the legacy signer loop.
use crate::{
    credits as abi, modular, now,
    protocol_groups::{self, Group},
    App,
};
use futures_util::TryStreamExt;
use mongodb::bson::{doc, Document};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
type R<T> = Result<T, String>;
fn db(_: impl std::fmt::Display) -> String {
    "Modular persistence unavailable".into()
}
async fn rpc(app: &App, method: &str, args: Value) -> R<Value> {
    app.stake
        .as_ref()
        .ok_or("Chain unavailable")?
        .rpc(method, args)
        .await
}
fn num(v: &Value, key: &str) -> R<u128> {
    abi::uint(v[key].as_str().ok_or("Invalid chain integer")?)
}
async fn monitor(app: &App, g: &Group) -> R<bool> {
    let states = app.db.collection::<Document>("protocol_group_state");
    let previous = states.find_one(doc! {"_id":&g.id}).await.map_err(db)?;
    if let Some(row) = &previous {
        if row.get_str("manifest_digest").ok() != Some(g.digest().as_str())
            || row.get_bool("halted").unwrap_or(true)
        {
            return Err("Group halted or manifest changed; operator review required".into());
        }
        let block = row.get_i64("block").map_err(db)?;
        let canonical = rpc(
            app,
            "eth_getBlockByNumber",
            json!([format!("0x{block:x}"), false]),
        )
        .await?;
        if canonical["hash"].as_str() != row.get_str("block_hash").ok() {
            states
                .update_one(
                    doc! {"_id":&g.id},
                    doc! {"$set":{"halted":true,"error":"Finalized group checkpoint changed"}},
                )
                .await
                .map_err(db)?;
            app.db
                .collection::<Document>("projects")
                .update_many(
                    doc! {"contract_group":&g.id},
                    doc! {"$set":{"status":"chain-halted"}},
                )
                .await
                .map_err(db)?;
            return Err("Finalized group checkpoint changed".into());
        }
    } else {
        protocol_groups::audit(app, g).await?;
    }
    let from = previous
        .as_ref()
        .and_then(|p| p.get_i64("block").ok())
        .map(|v| v as u64 + 1)
        .unwrap_or(g.deployment_block);
    let final_block = rpc(app, "eth_getBlockByNumber", json!(["finalized", false])).await?;
    let final_number = num(&final_block, "number")? as u64;
    if from > final_number {
        return Ok(false);
    }
    let to = (from + 249).min(final_number);
    let checkpoint = rpc(
        app,
        "eth_getBlockByNumber",
        json!([format!("0x{to:x}"), false]),
    )
    .await?;
    let mut addresses: Vec<String> = g.contracts.values().cloned().collect();
    if let Some(gate) = &g.migration_gate {
        addresses.push(gate.clone());
    }
    let logs=rpc(app,"eth_getLogs",json!([{"address":addresses,"fromBlock":format!("0x{from:x}"),"toBlock":format!("0x{to:x}")}])).await?;
    let mut events = vec![];
    let mut positions = vec![];
    let mut request_updates = vec![];
    for log in logs.as_array().ok_or("Invalid group logs")? {
        let hash = log["transactionHash"]
            .as_str()
            .ok_or("Missing event hash")?;
        let index = log["logIndex"].as_str().ok_or("Missing event index")?;
        let id = format!("{}:{hash}:{index}", g.id);
        let raw = mongodb::bson::to_document(log).map_err(db)?;
        events.push((id, raw));
        let deposit = format!(
            "0x{}",
            hex::encode(Keccak256::digest(
                b"CreditsDeposited(bytes32,address,uint256)"
            ))
        );
        if log["address"].as_str().map(str::to_lowercase).as_deref()
            == Some(g.contracts["usage_escrow"].as_str())
            && log["topics"][0] == json!(deposit)
        {
            let key = log["topics"][1].as_str().ok_or("Missing project topic")?;
            let buyer = log["topics"][2].as_str().ok_or("Missing buyer topic")?;
            let wallet = format!("0x{}", &abi::w(buyer, 0)?[24..]);
            if let Some(p) = app
                .db
                .collection::<Document>("projects")
                .find_one(doc! {"onchain_key":key})
                .await
                .map_err(db)?
            {
                let pid = p.get_str("_id").map_err(db)?;
                positions.push((wallet, pid.to_string()));
            }
        }
        if log["address"].as_str().map(str::to_lowercase).as_deref()
            == Some(g.contracts["usage_escrow"].as_str())
        {
            let released = format!(
                "0x{}",
                hex::encode(Keccak256::digest(b"ReservationReleased(bytes32)"))
            );
            let settled = format!(
                "0x{}",
                hex::encode(Keccak256::digest(
                    b"ProviderSettled(bytes32,bytes32,bytes32,address,address,uint256)"
                ))
            );
            if log["topics"][0] == json!(released) {
                if let Some(rid) = log["topics"][1].as_str() {
                    request_updates.push((rid.to_string(), "released"));
                }
            }
            if log["topics"][0] == json!(settled) {
                if let Some(rid) = log["topics"][2].as_str() {
                    request_updates.push((rid.to_string(), "settled"));
                }
            }
        }
    }
    let canonical = rpc(
        app,
        "eth_getBlockByNumber",
        json!([format!("0x{to:x}"), false]),
    )
    .await?;
    if canonical["hash"] != checkpoint["hash"] {
        return Err("Checkpoint changed during group indexing".into());
    }
    let mut session = app.db.client().start_session().await.map_err(db)?;
    session.start_transaction().await.map_err(db)?;
    for (id, raw) in events {
        app.db
            .collection::<Document>("modular_events")
            .update_one(
                doc! {"_id":id},
                doc! {"$setOnInsert":{"group":&g.id,"raw":raw}},
            )
            .upsert(true)
            .session(&mut session)
            .await
            .map_err(db)?;
    }
    for (wallet, pid) in positions {
        app.db
            .collection::<Document>("credit_positions")
            .update_one(
                doc! {"_id":format!("{wallet}:{pid}")},
                doc! {"$setOnInsert":{"wallet":wallet,"project_id":pid}},
            )
            .upsert(true)
            .session(&mut session)
            .await
            .map_err(db)?;
    }
    for (rid, status) in request_updates {
        app.db
            .collection::<Document>("modular_requests")
            .update_one(
                doc! {"_id":rid,"group":&g.id},
                doc! {"$set":{"status":status}},
            )
            .session(&mut session)
            .await
            .map_err(db)?;
    }
    states.update_one(doc!{"_id":&g.id},doc!{"$set":{"manifest_digest":g.digest(),"block":to as i64,"block_hash":checkpoint["hash"].as_str(),"halted":false,"checked_at":now(),"error":""}}).upsert(true).session(&mut session).await.map_err(db)?;
    let mut committed = false;
    for _ in 0..3 {
        match session.commit_transaction().await {
            Ok(()) => {
                committed = true;
                break;
            }
            Err(e) if e.contains_label("UnknownTransactionCommitResult") => continue,
            Err(e) => return Err(db(e)),
        }
    }
    if !committed {
        return Err(
            "Group checkpoint commit uncertain; retry read without re-serving requests".into(),
        );
    }
    Ok(to < final_number)
}
async fn prepare(app: &App, c: &abi::Config, action: &Document) -> R<()> {
    let g = protocol_groups::select(app, action.get_str("group").map_err(db)?)?;
    if action.get_str("manifest_digest").ok() != Some(g.digest().as_str()) {
        return Err("Action group configuration changed".into());
    }
    let to = action.get_str("to").map_err(db)?;
    if to != g.contracts["settlement_module"] && to != g.contracts["usage_escrow"] {
        return Err("Invalid modular action destination".into());
    }
    let latest = rpc(app, "eth_getTransactionCount", json!([c.settler, "latest"])).await?;
    let pending = rpc(
        app,
        "eth_getTransactionCount",
        json!([c.settler, "pending"]),
    )
    .await?;
    if latest != pending {
        return Err("Signer has a pending transaction".into());
    }
    let key = std::env::var("CREDIT_SETTLER_KEYSTORE").map_err(db)?;
    let password = std::env::var("CREDIT_SETTLER_PASSWORD_FILE").map_err(db)?;
    let signer = crate::credit_worker::command(vec![
        "wallet".into(),
        "address".into(),
        "--keystore".into(),
        key.clone(),
        "--password-file".into(),
        password.clone(),
    ])
    .await?;
    if signer.to_lowercase() != c.settler {
        return Err("Modular signer mismatch".into());
    }
    let raw = crate::credit_worker::command(vec![
        "mktx".into(),
        to.into(),
        action.get_str("calldata").map_err(db)?.into(),
        "--rpc-url".into(),
        std::env::var("CHAIN_RPC_URL").map_err(db)?,
        "--nonce".into(),
        abi::uint(latest.as_str().ok_or("Invalid nonce")?)?.to_string(),
        "--keystore".into(),
        key,
        "--password-file".into(),
        password,
    ])
    .await?;
    let hash = format!(
        "0x{}",
        hex::encode(Keccak256::digest(
            hex::decode(raw.trim_start_matches("0x")).map_err(db)?
        ))
    );
    app.db
        .collection::<Document>("modular_actions")
        .update_one(
            doc! {"_id":action.get_str("_id").map_err(db)?,"status":"preparing"},
            doc! {"$set":{"status":"prepared","raw":raw,"tx_hash":&hash}},
        )
        .await
        .map_err(db)?;
    app.db
        .collection::<Document>("modular_requests")
        .update_one(
            doc! {"_id":action.get_str("request_id").map_err(db)?},
            doc! {"$set":{"settlement_tx":hash}},
        )
        .await
        .map_err(db)?;
    Ok(())
}
/// True means this signer has an unbroadcast/unmined modular action; legacy preparation must wait.
pub(crate) async fn tick(app: &App, c: &abi::Config) -> R<bool> {
    let groups = protocol_groups::load(c.chain_id)?;
    for g in &groups {
        if !g.monitor_enabled {
            continue;
        }
        for _ in 0..4 {
            match monitor(app, g).await {
                Ok(true) => continue,
                Ok(false) => break,
                Err(e) => {
                    let _ = app
                        .db
                        .collection::<Document>("protocol_group_errors")
                        .update_one(doc! {"_id":&g.id}, doc! {"$set":{"error":e,"at":now()}})
                        .upsert(true)
                        .await;
                    break;
                }
            }
        }
    }
    let actions = app.db.collection::<Document>("modular_actions");
    let rows = actions
        .find(doc! {"signer":&c.settler,"status":{"$in":["preparing","prepared","mined"]}})
        .sort(doc! {"at":1})
        .await
        .map_err(db)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(db)?;
    let legacy_busy = app
        .db
        .collection::<Document>("credit_actions")
        .find_one(doc! {"status":{"$in":["preparing","prepared"]}})
        .await
        .map_err(db)?
        .is_some();
    for a in rows {
        if a.get_str("status").ok() == Some("preparing") {
            if legacy_busy {
                return Ok(false);
            }
            prepare(app, c, &a).await?;
            return Ok(true);
        }
        let hash = a.get_str("tx_hash").map_err(db)?;
        let receipt = rpc(app, "eth_getTransactionReceipt", json!([hash])).await?;
        if receipt.is_null() {
            let _ = rpc(
                app,
                "eth_sendRawTransaction",
                json!([a.get_str("raw").map_err(db)?]),
            )
            .await;
            return Ok(true);
        }
        let canonical = rpc(
            app,
            "eth_getBlockByNumber",
            json!([receipt["blockNumber"], false]),
        )
        .await?;
        if canonical["hash"] != receipt["blockHash"] {
            return Ok(true);
        }
        let reverted = receipt["status"] != json!("0x1");
        let final_block = rpc(app, "eth_getBlockByNumber", json!(["finalized", false])).await?;
        let done = num(&final_block, "number")? >= num(&receipt, "blockNumber")?;
        let status = if reverted {
            "error"
        } else if done {
            "finalized"
        } else {
            "mined"
        };
        let rid = a.get_str("request_id").map_err(db)?;
        actions
            .update_one(
                doc! {"_id":a.get_str("_id").map_err(db)?},
                doc! {"$set":{"status":status}},
            )
            .await
            .map_err(db)?;
        if reverted {
            app.db
                .collection::<Document>("modular_requests")
                .update_one(
                    doc! {"_id":rid},
                    doc! {"$set":{"status":"settlement-error"}},
                )
                .await
                .map_err(db)?;
        } else if done {
            let final_status = if a.get_str("kind").ok() == Some("settle") {
                "settled"
            } else {
                "released"
            };
            app.db
                .collection::<Document>("modular_requests")
                .update_one(doc! {"_id":rid}, doc! {"$set":{"status":final_status}})
                .await
                .map_err(db)?;
        }
    }
    // Recover a crash after action finality and before request status was updated.
    let finals = actions
        .find(doc! {"signer":&c.settler,"status":"finalized"})
        .await
        .map_err(db)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(db)?;
    for a in finals {
        app.db.collection::<Document>("modular_requests").update_one(doc!{"_id":a.get_str("request_id").map_err(db)?,"status":{"$in":["settling","releasing"]}},doc!{"$set":{"status":if a.get_str("kind").ok()==Some("settle"){"settled"}else{"released"}}}).await.map_err(db)?;
    }
    if legacy_busy {
        return Ok(false);
    }
    let requests = app.db.collection::<Document>("modular_requests");
    let Some(r) = requests
        .find_one(doc! {"status":{"$in":["consumed","release-pending"]}})
        .sort(doc! {"at":1})
        .await
        .map_err(db)?
    else {
        return Ok(false);
    };
    let g = protocol_groups::select(app, r.get_str("group").map_err(db)?)?;
    let Some(p)=app.db.collection::<Document>("projects").find_one(doc!{"_id":r.get_str("project_id").map_err(db)?,"contract_group":&g.id,"ledger_epoch":r.get_str("epoch").map_err(db)?}).await.map_err(db)? else{return Err("Request epoch no longer active".into());};
    modular::service(app, &p).await?;
    let rid = r.get_str("_id").map_err(db)?;
    let head = rpc(app, "eth_getBlockByNumber", json!(["latest", false])).await?;
    let expired = num(&head, "timestamp")? >= r.get_i64("expiry").map_err(db)? as u128;
    let settling = r.get_str("status").ok() == Some("consumed") && !expired;
    let (kind, target, data) = if settling {
        (
            "settle",
            g.contracts["settlement_module"].clone(),
            abi::data(
                "settleProvider(bytes32,bytes32,uint256)",
                &[
                    rid.trim_start_matches("0x").into(),
                    rid.trim_start_matches("0x").into(),
                    abi::word(r.get_i64("amount").map_err(db)? as u128),
                ],
            ),
        )
    } else if expired {
        (
            "release",
            g.contracts["usage_escrow"].clone(),
            abi::data(
                "releaseExpired(bytes32)",
                &[rid.trim_start_matches("0x").into()],
            ),
        )
    } else {
        (
            "release",
            g.contracts["settlement_module"].clone(),
            abi::data(
                "releaseFailed(bytes32)",
                &[rid.trim_start_matches("0x").into()],
            ),
        )
    };
    let mut session = app.db.client().start_session().await.map_err(db)?;
    session.start_transaction().await.map_err(db)?;
    let changed = requests
        .update_one(
            doc! {"_id":rid,"status":r.get_str("status").map_err(db)?},
            doc! {"$set":{"status":if settling{"settling"}else{"releasing"}}},
        )
        .session(&mut session)
        .await
        .map_err(db)?;
    if changed.modified_count != 1 {
        let _ = session.abort_transaction().await;
        return Ok(false);
    }
    actions.insert_one(doc!{"_id":format!("modular:{rid}"),"request_id":rid,"group":&g.id,"manifest_digest":g.digest(),"signer":&c.settler,"to":target,"calldata":data,"kind":kind,"status":"preparing","at":now()}).session(&mut session).await.map_err(db)?;
    session.commit_transaction().await.map_err(db)?;
    Ok(true)
}
