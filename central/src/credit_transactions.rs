//! Informational wallet transaction history. Never credits a balance from a submitted hash.
use crate::{
    credits::{self, Config},
    error, now, stake, unavailable, ApiResult, App,
};
use axum::{http::StatusCode, Json};
use futures_util::TryStreamExt;
use mongodb::bson::{doc, Document};
use serde::Deserialize;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
type R<T> = Result<T, String>;
fn topic(s: &str) -> String {
    format!("0x{}", hex::encode(Keccak256::digest(s.as_bytes())))
}
fn events() -> Vec<(String, &'static str)> {
    vec![
        (
            topic("CreditsPurchased(bytes32,address,uint256,uint256)"),
            "purchase",
        ),
        (
            topic("CreditsDeposited(bytes32,address,uint256)"),
            "deposit",
        ),
        (
            topic("CreditsWithdrawn(bytes32,address,uint256,uint256)"),
            "complete-withdrawal",
        ),
        (
            topic("WithdrawalRequested(bytes32,address,uint256,uint256)"),
            "withdraw",
        ),
    ]
}
fn hash(s: &str) -> bool {
    s.len() == 66 && s.starts_with("0x") && s[2..].bytes().all(|b| b.is_ascii_hexdigit())
}
fn decode(tx: &Value, c: &Config, pid: &str, wal: &str) -> R<(String, Option<String>)> {
    if tx["to"].as_str().unwrap_or("").to_lowercase() != c.contract
        || tx["from"].as_str().unwrap_or("").to_lowercase() != wal
    {
        return Err("Transaction does not belong to this buyer and credit contract".into());
    }
    let input = tx["input"].as_str().unwrap_or("").trim_start_matches("0x");
    let key = stake::project_key(pid);
    if input.get(8..72) != Some(&key[2..]) {
        return Err("Transaction targets another project".into());
    }
    for (sig, kind, quantity) in [
        ("buyAndLock(bytes32,uint256)", "purchase", true),
        ("depositCredits(bytes32,uint256)", "deposit", true),
        ("requestWithdrawal(bytes32,uint256)", "withdraw", true),
        ("completeWithdrawal(bytes32)", "complete-withdrawal", false),
        ("cancelWithdrawal(bytes32)", "cancel-withdrawal", false),
    ] {
        if input.starts_with(&credits::sel(sig)) {
            let qty = if quantity {
                Some(
                    credits::uint(input.get(72..136).ok_or("Invalid transaction arguments")?)?
                        .to_string(),
                )
            } else {
                None
            };
            return Ok((kind.into(), qty));
        }
    }
    Err("Unsupported credit transaction".into())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submission {
    tx_hash: String,
}
pub async fn submit(app: &App, user: &str, pid: &str, body: &[u8]) -> ApiResult {
    let c = credits::config(app).map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, &e))?;
    credits::project(app, pid).await?;
    let wal = credits::wallet(app, user).await?;
    let input: Submission = serde_json::from_slice(body)
        .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide tx_hash"))?;
    let h = input.tx_hash.to_lowercase();
    if !hash(&h) {
        return Err(error(StatusCode::BAD_REQUEST, "Invalid transaction hash"));
    }
    let tx = app
        .stake
        .as_ref()
        .unwrap()
        .rpc("eth_getTransactionByHash", json!([h]))
        .await
        .map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, &e))?;
    let (kind, quantity) = if tx.is_null() {
        ("unknown".into(), None)
    } else {
        decode(&tx, &c, pid, &wal).map_err(|e| error(StatusCode::BAD_REQUEST, &e))?
    };
    let collection = app.db.collection::<Document>("credit_wallet_transactions");
    let id = format!("{}:{pid}:{wal}:{h}", c.contract);
    if collection.find_one(doc!{"_id":&id}).await.map_err(unavailable)?.is_none() && collection.count_documents(doc!{"wallet":&wal,"contract":&c.contract,"status":{"$in":["not-seen","submitted","awaiting-finality","checking"]}}).await.map_err(unavailable)?>=30{return Err(error(StatusCode::TOO_MANY_REQUESTS,"Too many outstanding transactions; wait for confirmation before submitting more"));}
    collection.update_one(doc!{"_id":id,"wallet":&wal,"project_id":pid},doc!{"$setOnInsert":{"hash":&h,"wallet":&wal,"project_id":pid,"contract":&c.contract,"at":now(),"status":"checking","source":"wallet"},"$set":{"kind":kind,"quantity":quantity}}).upsert(true).await.map_err(unavailable)?;
    Ok(Json(
        json!({"tracked":true,"tx_hash":h,"note":"Tracking only; balances use finalized contract state."}),
    ))
}
async fn discover(
    app: &App,
    c: &Config,
    pid: &str,
    wal: &str,
    head: u64,
    finalized: u64,
) -> R<bool> {
    let cursor_id = format!("{}:{pid}:{wal}", c.contract);
    let cursors = app.db.collection::<Document>("credit_transaction_cursors");
    let row = cursors
        .find_one(doc! {"_id":&cursor_id})
        .await
        .map_err(|_| "Transaction cursor unavailable")?;
    let next = row
        .and_then(|r| r.get_i64("next").ok())
        .map(|v| v.max(0) as u64)
        .unwrap_or(c.deployment_block);
    let ev = events();
    let topics: Vec<String> = ev.iter().map(|e| e.0.clone()).collect();
    let key = stake::project_key(pid);
    let mut ranges = vec![];
    let mut at = next;
    for _ in 0..2 {
        if at > head {
            break;
        }
        let end = (at + 99).min(head);
        ranges.push((at, end));
        at = end + 1;
    }
    // Always include recent activity even while older history is still catching up.
    let recent = head.saturating_sub(199).max(c.deployment_block);
    for start in [recent, recent + 100] {
        if start <= head && !ranges.iter().any(|(a, b)| *a <= start && *b >= start) {
            ranges.push((start, (start + 99).min(head)));
        }
    }
    for (start, end) in ranges {
        let logs=app.stake.as_ref().ok_or("Chain unavailable")?.rpc("eth_getLogs",json!([{"address":c.contract,"fromBlock":format!("0x{start:x}"),"toBlock":format!("0x{end:x}"),"topics":[topics,key,format!("0x{}",credits::addr(wal))]}])).await?;
        for log in logs
            .as_array()
            .ok_or("Invalid transaction event response")?
        {
            if log["removed"] == true {
                continue;
            }
            if log["address"].as_str().unwrap_or("").to_lowercase() != c.contract
                || log["topics"][1] != key
                || log["topics"][2] != format!("0x{}", credits::addr(wal))
            {
                return Err("Transaction event identity mismatch".into());
            }
            let h = log["transactionHash"]
                .as_str()
                .filter(|h| hash(h))
                .ok_or("Invalid event hash")?;
            let kind = ev
                .iter()
                .find(|e| Some(e.0.as_str()) == log["topics"][0].as_str())
                .ok_or("Invalid event type")?
                .1;
            let qty = credits::uint(credits::w(
                log["data"].as_str().ok_or("Invalid event data")?,
                0,
            )?)?
            .to_string();
            let block = credits::uint(log["blockNumber"].as_str().ok_or("Invalid event block")?)?;
            app.db.collection::<Document>("credit_wallet_transactions").update_one(doc!{"_id":format!("{}:{pid}:{wal}:{h}",c.contract),"wallet":wal,"project_id":pid},doc!{"$setOnInsert":{"hash":h,"wallet":wal,"project_id":pid,"contract":&c.contract,"at":now(),"status":"checking"},"$set":{"kind":kind,"quantity":qty,"source":"event","block":block as i64}}).upsert(true).await.map_err(|_|"Could not save transaction history")?;
        }
    }
    let safe_next = at.min(finalized + 1).max(next);
    cursors
        .update_one(
            doc! {"_id":cursor_id},
            doc! {"$set":{"next":safe_next as i64}},
        )
        .upsert(true)
        .await
        .map_err(|_| "Could not save transaction cursor")?;
    Ok(at > head)
}
fn final_status(success: bool, canonical: bool, block: u64, finalized: u64) -> &'static str {
    if !canonical {
        "reorged"
    } else if !success {
        "failed"
    } else if block <= finalized {
        "finalized"
    } else {
        "awaiting-finality"
    }
}
pub async fn list(app: &App, c: &Config, pid: &str, wal: &str) -> R<Value> {
    let rpc = app.stake.as_ref().ok_or("Chain unavailable")?;
    let head = rpc
        .rpc("eth_getBlockByNumber", json!(["latest", false]))
        .await?;
    let fin = rpc
        .rpc("eth_getBlockByNumber", json!(["finalized", false]))
        .await?;
    let latest = credits::uint(head["number"].as_str().ok_or("Invalid latest block")?)? as u64;
    let finalized = credits::uint(fin["number"].as_str().ok_or("Invalid finalized block")?)? as u64;
    let lease = format!("credit-tx:{}:{pid}:{wal}", c.contract);
    let run = uuid::Uuid::new_v4().to_string();
    let mut scanned = false;
    if crate::trial::take_lease(app, &lease, &run).await.is_ok() {
        let scan = discover(app, c, pid, wal, latest, finalized).await;
        crate::trial::release(app, &[lease], &run).await;
        scanned = scan?;
    }
    let collection = app.db.collection::<Document>("credit_wallet_transactions");
    let rows = collection
        .find(doc! {"wallet":wal,"project_id":pid,"contract":&c.contract})
        .sort(doc! {"at":-1,"block":-1})
        .limit(25)
        .await
        .map_err(|_| "Transaction history unavailable")?
        .try_collect::<Vec<_>>()
        .await
        .map_err(|_| "Transaction history unavailable")?;
    let mut result = vec![];
    for row in rows {
        let h = row.get_str("hash").map_err(|_| "Invalid saved hash")?;
        let receipt = rpc.rpc("eth_getTransactionReceipt", json!([h])).await?;
        let mut kind = row.get_str("kind").unwrap_or("unknown").to_string();
        let mut qty = row.get_str("quantity").ok().map(str::to_owned);
        let (status, block, confirmations, is_final) = if receipt.is_null() {
            let tx = rpc.rpc("eth_getTransactionByHash", json!([h])).await?;
            if tx.is_null() {
                ("not-seen", None, 0, false)
            } else {
                match decode(&tx, c, pid, wal) {
                    Ok((k, q)) => {
                        kind = k;
                        qty = q;
                        ("submitted", None, 0, false)
                    }
                    Err(_) => ("invalid", None, 0, false),
                }
            }
        } else {
            let n = credits::uint(
                receipt["blockNumber"]
                    .as_str()
                    .ok_or("Invalid receipt block")?,
            )? as u64;
            let b = rpc
                .rpc(
                    "eth_getBlockByNumber",
                    json!([receipt["blockNumber"], false]),
                )
                .await?;
            let canonical = b["hash"] == receipt["blockHash"] && !b.is_null();
            // Caller-supplied hashes are never trusted just because they have a receipt.
            let valid = if row.get_str("source").ok() == Some("event") {
                true
            } else {
                let tx = rpc.rpc("eth_getTransactionByHash", json!([h])).await?;
                match decode(&tx, c, pid, wal) {
                    Ok((k, q)) => {
                        kind = k;
                        qty = q;
                        true
                    }
                    Err(_) => false,
                }
            };
            if !valid {
                ("invalid", Some(n), 0, false)
            } else {
                (
                    final_status(receipt["status"] == "0x1", canonical, n, finalized),
                    Some(n),
                    if canonical {
                        latest.saturating_sub(n) + 1
                    } else {
                        0
                    },
                    canonical && n <= finalized,
                )
            }
        };
        collection
            .update_one(
                doc! {"_id":row.get_str("_id").unwrap_or("")},
                doc! {"$set":{"status":status,"kind":&kind,"quantity":&qty,"checked_at":now()}},
            )
            .await
            .map_err(|_| "Could not update transaction status")?;
        result.push(json!({"tx_hash":h,"kind":kind,"quantity":qty,"status":status,"block":block,"confirmations":confirmations,"finalized":is_final,"blocks_to_finalized_head":block.map(|n|n.saturating_sub(finalized)),"observed_at":row.get_i64("at").ok()}));
    }
    Ok(
        json!({"available":true,"latest_block":latest,"finalized_block":finalized,"history_scan_caught_up":scanned,"transactions":result,"checked_at":now()}),
    )
}
pub async fn cached(app: &App, c: &Config, pid: &str, wal: &str) -> R<Value> {
    let rows = app
        .db
        .collection::<Document>("credit_wallet_transactions")
        .find(doc! {"wallet":wal,"project_id":pid,"contract":&c.contract})
        .sort(doc! {"at":-1})
        .limit(25)
        .await
        .map_err(|_| "History unavailable")?
        .try_collect::<Vec<_>>()
        .await
        .map_err(|_| "History unavailable")?;
    let rows:Vec<Value>=rows.iter().map(|r|json!({"tx_hash":r.get_str("hash").ok(),"kind":r.get_str("kind").unwrap_or("unknown"),"quantity":r.get_str("quantity").ok(),"status":"status-unavailable","previous_status":r.get_str("status").ok(),"block":r.get_i64("block").ok(),"confirmations":null,"finalized":false,"blocks_to_finalized_head":null})).collect();
    Ok(
        json!({"available":false,"transactions":rows,"error":"Live chain status is temporarily unavailable. Saved hashes are shown below; check their explorer links and do not submit a duplicate purchase."}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_successful_canonical_receipts_can_finalize() {
        assert_eq!(final_status(true, true, 100, 99), "awaiting-finality");
        assert_eq!(final_status(true, true, 100, 100), "finalized");
        assert_eq!(final_status(false, true, 100, 110), "failed");
        assert_eq!(final_status(true, false, 100, 110), "reorged");
    }
}
