//! Dedicated test settler. Signed raw transactions are persisted before broadcast and safely rebroadcast.
use crate::{
    credits::{self, Config},
    now, App,
};
use futures_util::TryStreamExt;
use mongodb::bson::{doc, Document};
use serde_json::json;
use sha3::{Digest, Keccak256};
use std::{sync::Arc, time::Duration};
type R<T> = Result<T, String>;
fn fail(_: impl std::fmt::Display) -> String {
    "Credit settlement service unavailable".into()
}
pub(crate) async fn command(args: Vec<String>) -> R<String> {
    let cast = std::env::var("STAKE_CAST").map_err(fail)?;
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        tokio::process::Command::new(cast)
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(fail)?
    .map_err(fail)?;
    if !output.status.success() {
        return Err("Credit transaction preparation failed; inspect settler funding/role".into());
    }
    String::from_utf8(output.stdout)
        .map(|s| s.trim().to_string())
        .map_err(fail)
}
async fn prepare(app: &App, c: &Config, action: &Document) -> R<()> {
    let id = action.get_str("_id").map_err(fail)?;
    let stake = app.stake.as_ref().ok_or("Chain unavailable")?;
    let latest = stake
        .rpc("eth_getTransactionCount", json!([c.settler, "latest"]))
        .await?;
    let pending = stake
        .rpc("eth_getTransactionCount", json!([c.settler, "pending"]))
        .await?;
    if latest != pending {
        return Err("Settler has an unconfirmed transaction; waiting".into());
    }
    let signer = command(vec![
        "wallet".into(),
        "address".into(),
        "--keystore".into(),
        std::env::var("CREDIT_SETTLER_KEYSTORE").map_err(fail)?,
        "--password-file".into(),
        std::env::var("CREDIT_SETTLER_PASSWORD_FILE").map_err(fail)?,
    ])
    .await?;
    if signer.to_lowercase() != c.settler {
        return Err("Settler keystore address mismatch".into());
    }
    let data = action.get_str("calldata").map_err(fail)?;
    let raw = command(vec![
        "mktx".into(),
        c.contract.clone(),
        data.into(),
        "--rpc-url".into(),
        std::env::var("CHAIN_RPC_URL").map_err(fail)?,
        "--nonce".into(),
        credits::uint(latest.as_str().ok_or("Invalid nonce")?)?.to_string(),
        "--keystore".into(),
        std::env::var("CREDIT_SETTLER_KEYSTORE").map_err(fail)?,
        "--password-file".into(),
        std::env::var("CREDIT_SETTLER_PASSWORD_FILE").map_err(fail)?,
    ])
    .await?;
    let bytes = hex::decode(raw.trim_start_matches("0x")).map_err(fail)?;
    let hash = format!("0x{}", hex::encode(Keccak256::digest(bytes)));
    app.db
        .collection::<Document>("credit_actions")
        .update_one(
            doc! {"_id":id,"status":"preparing"},
            doc! {"$set":{"status":"prepared","raw":raw,"tx_hash":&hash}},
        )
        .await
        .map_err(fail)?;
    if action.get_str("kind").ok() == Some("batch") {
        app.db
            .collection::<Document>("credit_requests")
            .update_many(doc! {"batch_id":id}, doc! {"$set":{"tx_hash":hash}})
            .await
            .map_err(fail)?;
    }
    Ok(())
}
async fn tick(app: &App, c: &Config) -> R<()> {
    let stake = app.stake.as_ref().ok_or("Chain unavailable")?;
    let binding = credits::call(app, c, credits::data("stakeContract()", &[]), "finalized").await?;
    if format!("0x{}", &credits::w(&binding, 0)?[24..]) != c.staking_contract {
        return Err("Credit contract stake binding mismatch".into());
    }
    let role = hex::encode(Keccak256::digest(b"SETTLER_ROLE"));
    let authorized = credits::call(
        app,
        c,
        credits::data(
            "hasRole(bytes32,address)",
            &[role, credits::addr(&c.settler)],
        ),
        "finalized",
    )
    .await?;
    if credits::uint(&authorized)? != 1 {
        return Err("Credit settler role unavailable".into());
    }
    let checkpoint = app
        .db
        .collection::<Document>("chain_state")
        .find_one(doc! {"_id":format!("{}:{}",stake.config.chain_id,stake.config.staking_contract)})
        .await
        .map_err(fail)?;
    if checkpoint
        .as_ref()
        .and_then(|d| d.get_bool("halted").ok())
        .unwrap_or(true)
    {
        return Err("Chain monitor halted/unavailable".into());
    }
    let actions = app.db.collection::<Document>("credit_actions");
    let rows = actions
        .find(doc! {"contract":&c.contract,"status":{"$in":["preparing","prepared","mined"]}})
        .await
        .map_err(fail)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(fail)?;
    let mut unmined = false;
    for action in rows {
        let id = action.get_str("_id").map_err(fail)?;
        if action.get_str("status").ok() == Some("preparing") {
            prepare(app, c, &action).await?;
            unmined = true;
            continue;
        }
        let h = action.get_str("tx_hash").map_err(fail)?;
        let receipt = stake.rpc("eth_getTransactionReceipt", json!([h])).await?;
        if receipt.is_null() {
            let _ = stake
                .rpc(
                    "eth_sendRawTransaction",
                    json!([action.get_str("raw").map_err(fail)?]),
                )
                .await;
            unmined = true;
            continue;
        }
        if receipt["status"] != json!("0x1") {
            actions.update_one(doc!{"_id":id},doc!{"$set":{"status":"error","error":"Transaction reverted; operator review required"}}).await.map_err(fail)?;
            app.db
                .collection::<Document>("credit_requests")
                .update_many(
                    doc! {"batch_id":id},
                    doc! {"$set":{"status":"settlement-error"}},
                )
                .await
                .map_err(fail)?;
            continue;
        }
        let block = stake
            .rpc(
                "eth_getBlockByNumber",
                json!([receipt["blockNumber"], false]),
            )
            .await?;
        if block["hash"] != receipt["blockHash"] {
            unmined = true;
            continue;
        }
        let finalized = stake
            .rpc("eth_getBlockByNumber", json!(["finalized", false]))
            .await?;
        let done = credits::uint(
            finalized["number"]
                .as_str()
                .ok_or("Invalid finalized block")?,
        )? >= credits::uint(receipt["blockNumber"].as_str().ok_or("Invalid receipt")?)?;
        actions.update_one(doc!{"_id":id},doc!{"$set":{"status":if done{"finalized"}else{"mined"},"block":receipt["blockNumber"].as_str()}}).await.map_err(fail)?;
        if done && action.get_str("kind").ok() == Some("batch") {
            let raw = credits::call(
                app,
                c,
                credits::data(
                    "batchDigest(bytes32)",
                    &[id.trim_start_matches("0x").into()],
                ),
                "finalized",
            )
            .await?;
            if raw.trim_start_matches("0x").chars().all(|ch| ch == '0') {
                return Err("Finalized batch evidence missing".into());
            }
            app.db
                .collection::<Document>("credit_requests")
                .update_many(
                    doc! {"batch_id":id,"status":"batching"},
                    doc! {"$set":{"status":"settled"}},
                )
                .await
                .map_err(fail)?;
        }
    }
    // Recover the benign crash window between finalized action and request status update.
    let finals = actions
        .find(doc! {"contract":&c.contract,"kind":"batch","status":"finalized"})
        .await
        .map_err(fail)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(fail)?;
    for a in finals {
        app.db
            .collection::<Document>("credit_requests")
            .update_many(
                doc! {"batch_id":a.get_str("_id").map_err(fail)?,"status":"batching"},
                doc! {"$set":{"status":"settled"}},
            )
            .await
            .map_err(fail)?;
    }
    if unmined {
        return Ok(());
    }
    let requests = app.db.collection::<Document>("credit_requests");
    let rows = requests
        .find(doc! {"contract":&c.contract,"status":"consumed"})
        .sort(doc! {"at":1})
        .limit(16)
        .await
        .map_err(fail)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(fail)?;
    if !rows.is_empty() {
        let id = format!(
            "0x{}",
            hex::encode(Keccak256::digest(uuid::Uuid::new_v4().as_bytes()))
        );
        let array = |v: Vec<String>| format!("[{}]", v.join(","));
        let ps = array(
            rows.iter()
                .map(|r| crate::stake::project_key(r.get_str("project_id").unwrap_or("")))
                .collect(),
        );
        let ws = array(
            rows.iter()
                .map(|r| r.get_str("wallet").unwrap_or("").into())
                .collect(),
        );
        let ns = array(
            rows.iter()
                .map(|r| r.get_i64("amount").unwrap_or(0).to_string())
                .collect(),
        );
        let us = array(
            rows.iter()
                .map(|r| format!("0x{}", r.get_str("_id").unwrap_or("")))
                .collect(),
        );
        let calldata = command(vec![
            "calldata".into(),
            "settleBatch(bytes32,bytes32[],address[],uint256[],bytes32[])".into(),
            id.clone(),
            ps,
            ws,
            ns,
            us,
        ])
        .await?;
        let ids: Vec<String> = rows
            .iter()
            .map(|r| r.get_str("_id").unwrap_or("").into())
            .collect();
        let mut session = app.db.client().start_session().await.map_err(fail)?;
        session.start_transaction().await.map_err(fail)?;
        let claimed = requests
            .update_many(
                doc! {"_id":{"$in":&ids},"status":"consumed"},
                doc! {"$set":{"status":"batching","batch_id":&id}},
            )
            .session(&mut session)
            .await
            .map_err(fail)?;
        if claimed.modified_count != ids.len() as u64 {
            let _ = session.abort_transaction().await;
            return Err("Settlement claim changed; retrying".into());
        }
        actions.insert_one(doc!{"_id":&id,"contract":&c.contract,"kind":"batch","status":"preparing","calldata":calldata,"at":now()}).session(&mut session).await.map_err(fail)?;
        session.commit_transaction().await.map_err(fail)?;
        return Ok(());
    }
    let positions = app
        .db
        .collection::<Document>("credit_positions")
        .find(doc! {})
        .await
        .map_err(fail)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(fail)?;
    for pos in positions {
        let pid = pos.get_str("project_id").map_err(fail)?;
        if app
            .db
            .collection::<Document>("projects")
            .find_one(doc! {"_id":pid,"contract_group":{"$exists":true}})
            .await
            .map_err(fail)?
            .is_some()
        {
            continue;
        }
        let wal = pos.get_str("wallet").map_err(fail)?;
        let lease = format!("paid:{wal}:{pid}");
        let run = uuid::Uuid::new_v4().to_string();
        if crate::trial::take_lease(app, &lease, &run).await.is_err() {
            continue;
        }
        let queued: R<bool>=async {
        let a = credits::account(app, c, pid, wal, "finalized").await?;
        let latest = credits::account(app, c, pid, wal, "latest").await?;
        if !a.pending || !latest.pending || latest.approved || a.nonce != latest.nonce {
            return Ok(false);
        }
        let (r, u) = credits::pending(app, pid, wal).await.map_err(fail)?;
        if r > 0 || u != a.burned || u != latest.burned {
            return Ok(false);
        }
        let id = format!("withdraw:{pid}:{wal}:{}", a.nonce);
        if actions
            .find_one(doc! {"_id":&id})
            .await
            .map_err(fail)?
            .is_some()
        {
            return Ok(false);
        }
        let key = crate::stake::project_key(pid);
        let calldata = credits::data(
            "approveWithdrawal(bytes32,address,uint256,uint256)",
            &[
                key[2..].into(),
                credits::addr(wal),
                credits::word(a.nonce),
                credits::word(a.withdrawal.min(latest.locked)),
            ],
        );
        actions.insert_one(doc!{"_id":id,"contract":&c.contract,"kind":"withdraw","status":"preparing","calldata":calldata,"at":now()}).await.map_err(fail)?;
        Ok(true)
        }.await;
        crate::trial::release(app, &[lease], &run).await;
        if queued? {
            break;
        }
    }
    Ok(())
}
pub async fn run(app: Arc<App>) {
    let owner = uuid::Uuid::new_v4().to_string();
    loop {
        if let Ok(c) = credits::config(&app) {
            let id = format!("settler:{}:{}", c.chain_id, c.settler);
            let lease = app
                .db
                .collection::<Document>("service_leases")
                .update_one(
                    doc! {"_id":&id,"$or":[{"owner":&owner},{"until":{"$lte":now()}}]},
                    doc! {"$set":{"owner":&owner,"until":now()+300}},
                )
                .upsert(true)
                .await;
            if lease.is_ok() {
                let result = tokio::time::timeout(Duration::from_secs(90), async {
                    let busy=match crate::modular_worker::tick(&app,&c).await {
                        Ok(v)=>v,Err(e)=>{
                            let _=app.db.collection::<Document>("protocol_group_errors").update_one(doc!{"_id":"worker"},doc!{"$set":{"error":e,"at":now()}}).upsert(true).await;
                            app.db.collection::<Document>("modular_actions").find_one(doc!{"signer":&c.settler,"status":{"$in":["preparing","prepared"]}}).await.map_err(fail)?.is_some()
                        }
                    };
                    if busy {Ok(())}else{tick(&app,&c).await}
                })
                    .await
                    .unwrap_or_else(|_| {
                        Err("Settlement cycle timed out; durable actions will resume".into())
                    });
                let _=app.db.collection::<Document>("service_leases").update_one(doc!{"_id":id,"owner":&owner},doc!{"$set":{"until":now()+300,"checked_at":now(),"error":result.err().unwrap_or_default()}}).await;
            }
        }
        tokio::time::sleep(Duration::from_secs(10)).await;
    }
}
