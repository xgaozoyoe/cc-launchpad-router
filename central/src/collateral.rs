//! Provider-only collateral views and wallet transaction preparation; no server-initiated withdrawal.
use crate::{credits, error, now, stake, unavailable, ApiResult, App};
use axum::{http::StatusCode, Json};
use futures_util::TryStreamExt;
use mongodb::bson::{doc, Document};
use serde::Deserialize;
use serde_json::{json, Value};
async fn call(app: &App, contract: &str, data: String, tag: &str) -> Result<String, String> {
    app.stake
        .as_ref()
        .ok_or("Chain not configured")?
        .rpc("eth_call", json!([{"to":contract,"data":data},tag]))
        .await?
        .as_str()
        .map(str::to_owned)
        .ok_or("Invalid collateral response".into())
}
fn numbers(raw: &str) -> Result<[u128; 6], String> {
    let mut out = [0; 6];
    for (i, v) in out.iter_mut().enumerate() {
        *v = credits::uint(credits::w(raw, i)?)?;
    }
    Ok(out)
}
fn view(v: [u128; 6]) -> Value {
    json!({"initial":v[0].to_string(),"locked":v[1].to_string(),"free":v[2].to_string(),"returned":v[3].to_string(),"delivered_credits":v[4].to_string(),"committed_credits":v[5].to_string()})
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Withdraw {
    amount_raw: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submission {
    tx_hash: String,
}
pub async fn handle(
    app: &App,
    owners: Vec<String>,
    id: &str,
    action: &str,
    method: &str,
    body: &[u8],
) -> ApiResult {
    if !((method == "GET" && action.is_empty())
        || (method == "POST" && matches!(action, "/withdraw" | "/submit")))
    {
        return Err(error(
            StatusCode::NOT_FOUND,
            "Collateral endpoint not found",
        ));
    }
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(error(StatusCode::NOT_FOUND, "Project not found"));
    }
    let p = app
        .db
        .collection::<Document>("projects")
        .find_one(doc! {"_id":id,"account_id":{"$in":owners}})
        .await
        .map_err(unavailable)?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Project not found"))?;
    let service = app
        .stake
        .as_ref()
        .ok_or_else(|| error(StatusCode::SERVICE_UNAVAILABLE, "Chain unavailable"))?;
    if p.get_bool("stake_finalized").ok() != Some(true) {
        return Ok(Json(
            json!({"progressive":false,"reason":"Balances appear after finalized stake."}),
        ));
    }
    if p.get_i64("stake_chain_id").ok() != Some(service.config.chain_id as i64) {
        return Err(error(StatusCode::CONFLICT, "Project chain mismatch"));
    }
    let contract =
        stake::address(p.get_str("staking_contract").map_err(unavailable)?).map_err(unavailable)?;
    let wallet =
        stake::address(p.get_str("wallet_address").map_err(unavailable)?).map_err(unavailable)?;
    let key = stake::project_key(id);
    let state = call(
        app,
        &contract,
        credits::data("stakeState(bytes32)", &[key[2..].into()]),
        "finalized",
    )
    .await
    .map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, &e))?;
    if format!("0x{}", &credits::w(&state, 4).map_err(unavailable)?[24..]) != wallet {
        return Err(error(StatusCode::CONFLICT, "Provider wallet mismatch"));
    }
    let version = call(
        app,
        &contract,
        credits::data("COLLATERAL_VERSION()", &[]),
        "finalized",
    )
    .await
    .ok()
    .and_then(|v| credits::uint(&v).ok());
    if version != Some(2) {
        if method != "GET" {
            return Err(error(
                StatusCode::CONFLICT,
                "Legacy stake has no partial withdrawal; migration is not enabled",
            ));
        }
        return Ok(Json(
            json!({"progressive":false,"contract":contract,"reason":"This project uses the legacy escrow. Partial release is unavailable; existing stake remains in place. V2 requires a new deployment/project or a separately reviewed migration."}),
        ));
    }
    let f = numbers(
        &call(
            app,
            &contract,
            credits::data("collateralState(bytes32)", &[key[2..].into()]),
            "finalized",
        )
        .await
        .map_err(unavailable)?,
    )
    .map_err(unavailable)?;
    let l = numbers(
        &call(
            app,
            &contract,
            credits::data("collateralState(bytes32)", &[key[2..].into()]),
            "latest",
        )
        .await
        .map_err(unavailable)?,
    )
    .map_err(unavailable)?;
    let chain = app
        .db
        .collection::<Document>("chain_state")
        .find_one(
            doc! {"_id":format!("{}:{}",service.config.chain_id,service.config.staking_contract)},
        )
        .await
        .map_err(unavailable)?;
    let halted = chain
        .as_ref()
        .and_then(|d| d.get_bool("halted").ok())
        .unwrap_or(true);
    let free = if halted { 0 } else { f[2].min(l[2]) };
    if method == "POST" && action == "/withdraw" {
        let input: Withdraw = serde_json::from_slice(body).map_err(|_| {
            error(
                StatusCode::BAD_REQUEST,
                "Provide amount_raw as an integer string",
            )
        })?;
        if input.amount_raw.is_empty() || !input.amount_raw.bytes().all(|b| b.is_ascii_digit()) {
            return Err(error(StatusCode::BAD_REQUEST, "Invalid PRL amount"));
        }
        let n = input
            .amount_raw
            .parse::<u128>()
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Invalid PRL amount"))?;
        if n == 0 || n > free {
            return Err(error(
                StatusCode::CONFLICT,
                "Amount exceeds finalized free collateral, or chain monitoring is paused",
            ));
        }
        return Ok(Json(
            json!({"transaction":{"from":wallet,"to":contract,"data":credits::data("withdrawCollateral(bytes32,uint256)",&[key[2..].into(),credits::word(n)]),"value":"0x0","chainId":format!("0x{:x}",service.config.chain_id)}}),
        ));
    }
    if method == "POST" && action == "/submit" {
        let input: Submission = serde_json::from_slice(body)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide tx_hash"))?;
        let h = input.tx_hash.to_lowercase();
        if h.len() != 66 || !h.starts_with("0x") || !h[2..].bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(error(StatusCode::BAD_REQUEST, "Invalid transaction hash"));
        }
        let tx = service
            .rpc("eth_getTransactionByHash", json!([h]))
            .await
            .map_err(unavailable)?;
        if tx.is_null() {
            return Err(error(StatusCode::CONFLICT,"RPC has not found the withdrawal yet; preserve the hash and refresh instead of withdrawing again"));
        }
        let data = tx["input"].as_str().unwrap_or("").trim_start_matches("0x");
        if tx["from"].as_str().unwrap_or("").to_lowercase() != wallet
            || tx["to"].as_str().unwrap_or("").to_lowercase() != contract
            || !data.starts_with(&credits::sel("withdrawCollateral(bytes32,uint256)"))
            || data.get(8..72) != Some(&key[2..])
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Withdrawal does not match this provider/project/contract",
            ));
        }
        app.db.collection::<Document>("collateral_withdrawals").update_one(doc!{"_id":format!("{contract}:{id}:{h}")},doc!{"$setOnInsert":{"hash":&h,"project_id":id,"contract":&contract,"wallet":&wallet,"at":now()}}).upsert(true).await.map_err(unavailable)?;
        return Ok(Json(json!({"tracked":true,"tx_hash":h})));
    }
    if method != "GET" || !action.is_empty() {
        return Err(error(
            StatusCode::NOT_FOUND,
            "Collateral endpoint not found",
        ));
    }
    let head = service
        .rpc("eth_getBlockByNumber", json!(["finalized", false]))
        .await
        .map_err(unavailable)?;
    let final_block = credits::uint(head["number"].as_str().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Finalized block unavailable",
        )
    })?)
    .map_err(unavailable)?;
    let rows = app
        .db
        .collection::<Document>("collateral_withdrawals")
        .find(doc! {"project_id":id,"contract":&contract,"wallet":&wallet})
        .sort(doc! {"at":-1})
        .limit(10)
        .await
        .map_err(unavailable)?
        .try_collect::<Vec<_>>()
        .await
        .map_err(unavailable)?;
    let mut history = vec![];
    for row in rows {
        let h = row.get_str("hash").map_err(unavailable)?;
        let r = service
            .rpc("eth_getTransactionReceipt", json!([h]))
            .await
            .map_err(unavailable)?;
        let status = if r.is_null() {
            "submitted"
        } else {
            let b = service
                .rpc("eth_getBlockByNumber", json!([r["blockNumber"], false]))
                .await
                .map_err(unavailable)?;
            if b.is_null() || b["hash"] != r["blockHash"] {
                "reconciling"
            } else if r["status"] != "0x1" {
                "failed"
            } else if credits::uint(r["blockNumber"].as_str().unwrap_or("0x0"))
                .map_err(unavailable)?
                <= final_block
            {
                "finalized"
            } else {
                "awaiting-finality"
            }
        };
        history.push(json!({"tx_hash":h,"status":status}));
    }
    Ok(Json(
        json!({"progressive":true,"chain_id":service.config.chain_id,"staking_contract":contract,"wallet":wallet,"finalized":view(f),"latest":view(l),"withdrawable_raw":free.to_string(),"unlock_pending_raw":l[2].saturating_sub(f[2]).to_string(),"withdrawal_pending_raw":l[3].saturating_sub(f[3]).to_string(),"chain_halted":halted,"withdrawals":history}),
    ))
}
