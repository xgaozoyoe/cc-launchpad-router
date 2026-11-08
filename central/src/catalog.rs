//! Authenticated directory of public launch metadata. Drafts and private account/config fields stay private.
use crate::{unavailable, ApiResult, App};
use axum::Json;
use futures_util::TryStreamExt;
use mongodb::bson::{doc, Document};
use serde_json::json;
pub fn models() -> serde_json::Value {
    json!([{"id":"qwen3-8b-awq","model":"Qwen/Qwen3-8B-AWQ","precision":"AWQ","revision":"93eb32ea9ef98cc266b85563ad0dd29e5f2940b9","context":4096}])
}
pub async fn list(app: &App) -> ApiResult {
    let models = models();
    let Some(stake) = app.stake.as_ref() else {
        return Ok(Json(
            json!({"configured":false,"launches":[],"models":models,"sales_enabled":false}),
        ));
    };
    let mut rows=app.db.collection::<Document>("projects").find(doc!{"stake_finalized":true,"launch_id":{"$type":"string","$ne":""},"stake_chain_id":stake.config.chain_id as i64,"staking_contract":&stake.config.staking_contract,"status":{"$in":["awaiting-rpc","verification-pending","accepted","chain-halted"]}})
 .projection(doc!{"_id":1,"name":1,"model_id":1,"status":1,"launch_id":1,"provider_id":1,"stake_raw":1,"stake_finalized":1,"capacity_approval":1,"onchain_key":1,"wallet_address":1,"stake_tx_hash":1,"rpc_deadline":1,"staking_contract":1,"stake_chain_id":1})
 .sort(doc!{"staked_at":-1,"_id":1}).limit(101).await.map_err(unavailable)?.try_collect::<Vec<_>>().await.map_err(unavailable)?;
    let has_more = rows.len() > 100;
    rows.truncate(100);
    crate::project_progress(app, &mut rows)
        .await
        .map_err(unavailable)?;
    // Keep only the existence of a manual capacity decision, never its actor/account/reason.
    for row in &mut rows {
        let approved = row.remove("capacity_approval").is_some();
        row.insert("capacity_approved", approved);
        let trial = row.get_str("status").ok() == Some("accepted")
            && crate::rpc_credentials::trial_enabled(row);
        row.insert("trial_available", trial);
        row.insert("sales_enabled", false);
        if row.get_str("status").ok() == Some("accepted")
            && crate::rpc_credentials::paid_key(row).is_ok()
            && crate::credits::ready(app, row).await.is_ok()
        {
            if let Ok(o) = crate::credits::offer(app, row, "finalized").await {
                row.insert("credit_offer",doc!{"price_wei_per_credit":o.price.to_string(),"supply":o.supply.to_string(),"issued":o.issued.to_string(),"provider":o.provider,"unit":"output_token"});
                row.insert("sales_enabled", o.active && o.issued < o.supply);
            } else if let Ok(o) = crate::credits::offer(app, row, "latest").await {
                row.insert("credit_offer",doc!{"price_wei_per_credit":o.price.to_string(),"supply":o.supply.to_string(),"issued":o.issued.to_string(),"provider":o.provider,"unit":"output_token"});
                row.insert("credit_offer_pending", true);
            }
        }
    }
    for row in &mut rows {
        for field in [
            "onchain_key",
            "stake_tx_hash",
            "wallet_address",
            "rpc_deadline",
            "staking_contract",
            "stake_chain_id",
        ] {
            row.remove(field);
        }
    }
    let sales = rows
        .iter()
        .any(|r| r.get_bool("sales_enabled").unwrap_or(false));
    Ok(Json(
        json!({"configured":true,"chain_id":stake.config.chain_id,"network":if stake.config.chain_id==11155111{"Ethereum Sepolia"}else{"Configured test network"},"staking_contract":stake.config.staking_contract,"token_address":stake.config.token_address,"token_label":"tPRL (test token)","launches":rows,"models":models,"has_more":has_more,"sales_enabled":sales}),
    ))
}
