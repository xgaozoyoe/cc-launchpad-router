use crate::{error, unavailable, ApiResult, App};
use axum::{
    extract::{OriginalUri, State},
    http::StatusCode,
    Json,
};
use mongodb::bson::{doc, Document};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
#[derive(Deserialize)]
struct Config {
    helper: String,
    python: String,
    server_addr: String,
    server_port: u16,
}
fn config() -> Result<Config, String> {
    let path = std::env::var("PROVIDER_BUNDLE_CONFIG")
        .map_err(|_| "Provider bundle service unavailable")?;
    serde_json::from_slice(&std::fs::read(path).map_err(|_| "Bundle configuration unavailable")?)
        .map_err(|_| "Bundle configuration invalid".into())
}
fn value<'a>(p: &'a Document, k: &str) -> Result<&'a str, String> {
    p.get_str(k).map_err(|_| format!("Project missing {k}"))
}
pub(crate) async fn eligible(app: &App, p: &Document) -> Result<(), String> {
    if p.get_str("contract_group").is_ok() {
        return crate::modular::service(app, p).await;
    }

    if !matches!(
        p.get_str("status").unwrap_or(""),
        "awaiting-rpc" | "verification-pending" | "accepted"
    ) || p.get_bool("stake_finalized").ok() != Some(true)
    {
        return Err("Project needs an active finalized stake awaiting RPC".into());
    }
    let stake = app.stake.as_ref().ok_or("Stake service unavailable")?;
    let row = app
        .db
        .collection::<Document>("chain_state")
        .find_one(doc! {"_id":format!("{}:{}",stake.config.chain_id,stake.config.staking_contract)})
        .await
        .map_err(|_| "Chain state unavailable")?;
    if row
        .as_ref()
        .and_then(|p| p.get_bool("halted").ok())
        .unwrap_or(true)
    {
        return Err("Chain monitor unavailable or halted".into());
    }
    let state = stake.state(value(p, "onchain_key")?, "latest").await?;
    let block = stake
        .rpc("eth_getBlockByNumber", json!(["latest", false]))
        .await?;
    let at = u64::from_str_radix(
        block["timestamp"]
            .as_str()
            .ok_or("Chain time unavailable")?
            .trim_start_matches("0x"),
        16,
    )
    .map_err(|_| "Chain time invalid")?;
    let deadline = p.get_i64("rpc_deadline").map_err(|_| "Missing deadline")?;
    if deadline <= 0
        || !matches!(state.0, 2 | 3)
        || (state.0 == 2 && (at >= deadline as u64 || p.get_str("status").ok() == Some("accepted")))
        || state.3 != deadline as u64
        || state.4 != value(p, "wallet_address")?
        || state.1.to_string() != value(p, "stake_raw")?
    {
        return Err("Stake expired, refunded or no longer matches project".into());
    }
    if value(p, "provider_id")? != value(p, "stake_tx_hash")? {
        return Err("Provider identity mismatch".into());
    }
    Ok(())
}
pub async fn handle(
    app: &App,
    owners: Vec<String>,
    id: &str,
    action: &str,
    method: &str,
) -> ApiResult {
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
    let cfg = config().map_err(|_| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Provider bundle service unavailable",
        )
    })?;
    if action.is_empty() && method == "GET" {
        let record = app
            .db
            .collection::<Document>("provider_deployments")
            .find_one(doc! {"_id":id})
            .await
            .map_err(unavailable)?;
        let eligibility = eligible(app, &p).await;
        return Ok(Json(
            json!({"configured":true,"bundle_available":eligibility.is_ok(),"reason":eligibility.err(),"server_addr":cfg.server_addr,"server_port":cfg.server_port,"platform":"linux-amd64","frp_version":"0.71.1","issued":record.is_some(),"last_login_at":record.as_ref().and_then(|d|d.get_i64("last_login_at").ok()),"last_proxy_at":record.as_ref().and_then(|d|d.get_i64("last_proxy_at").ok()),"rpc_acceptance":"not_integrated"}),
        ));
    }
    if action != "/bundle" || method != "POST" {
        return Err(error(
            StatusCode::NOT_FOUND,
            "Deployment endpoint not found",
        ));
    }
    eligible(app, &p)
        .await
        .map_err(|e| error(StatusCode::CONFLICT, &e))?;
    let mut child = tokio::process::Command::new(&cfg.python)
        .arg(&cfg.helper)
        .env(
            "PROVIDER_BUNDLE_CONFIG",
            std::env::var("PROVIDER_BUNDLE_CONFIG").unwrap(),
        )
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| {
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Bundle builder unavailable",
            )
        })?;
    let input=serde_json::to_vec(&json!({"project_id":id,"launch_id":p.get_str("launch_id").unwrap_or(""),"provider_id":p.get_str("provider_id").unwrap_or(""),"deadline":p.get_i64("rpc_deadline").unwrap_or(0)})).unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(&input)
        .await
        .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, "Bundle builder failed"))?;
    let out = tokio::time::timeout(std::time::Duration::from_secs(45), child.wait_with_output())
        .await
        .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, "Bundle builder timed out"))?
        .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, "Bundle builder failed"))?;
    if !out.status.success() {
        return Err(error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Bundle builder failed",
        ));
    }
    let result: Value = serde_json::from_slice(&out.stdout)
        .map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, "Invalid bundle result"))?;
    let hash = result["credential_sha256"]
        .as_str()
        .filter(|s| s.len() == 64)
        .ok_or_else(|| error(StatusCode::SERVICE_UNAVAILABLE, "Invalid credential record"))?;
    eligible(app, &p)
        .await
        .map_err(|e| error(StatusCode::CONFLICT, &e))?;
    let collection = app.db.collection::<Document>("provider_deployments");
    collection.update_one(doc!{"_id":id},doc!{"$setOnInsert":{"provider_id":p.get_str("provider_id").unwrap(),"credential_sha256":hash,"created_at":crate::now()}}).upsert(true).await.map_err(unavailable)?;
    let saved = collection
        .find_one(doc! {"_id":id})
        .await
        .map_err(unavailable)?
        .unwrap();
    if saved.get_str("credential_sha256").ok() != Some(hash) {
        return Err(error(
            StatusCode::CONFLICT,
            "Credential record mismatch; contact operator",
        ));
    }
    Ok(Json(
        json!({"filename":result["filename"],"bundle_base64":result["bundle_base64"]}),
    ))
}
pub fn authorized(op: &str, c: &Value, provider: &str, hash: &str) -> bool {
    let u = if op == "Login" { c } else { &c["user"] };
    if u["user"].as_str() != Some(provider) {
        return false;
    }
    let Some(token) = u["metas"]["provider_token"].as_str() else {
        return false;
    };
    use hmac::{Hmac, Mac};
    let Ok(expected) = hex::decode(hash) else {
        return false;
    };
    // Constant-time digest comparison through a keyed MAC verification.
    let digest = Sha256::digest(token.as_bytes());
    let mut mac = Hmac::<Sha256>::new_from_slice(&digest).unwrap();
    mac.update(b"provider-auth");
    let tag = mac.finalize().into_bytes();
    let mut check = Hmac::<Sha256>::new_from_slice(&expected).unwrap();
    check.update(b"provider-auth");
    if check.verify_slice(&tag).is_err() {
        return false;
    }
    if op == "Login" {
        return true;
    }
    if op != "NewProxy" {
        return false;
    }
    let route = format!(
        "p-{}.launchpad.internal",
        &hex::encode(Sha256::digest(provider.as_bytes()))[..40]
    );
    c["proxy_type"] == "http"
        && c["proxy_name"] == format!("{provider}.inference")
        && c["custom_domains"] == json!([route])
        && [
            "subdomain",
            "host_header_rewrite",
            "http_user",
            "http_pwd",
            "group",
            "group_key",
        ]
        .iter()
        .all(|k| c[*k].as_str().unwrap_or("").is_empty())
        && c["locations"]
            .as_array()
            .map(|a| a.is_empty())
            .unwrap_or(true)
}
pub async fn hook(
    State(app): State<Arc<App>>,
    OriginalUri(uri): OriginalUri,
    Json(body): Json<Value>,
) -> Json<Value> {
    let op = uri
        .query()
        .unwrap_or("")
        .split('&')
        .find_map(|s| s.strip_prefix("op="))
        .unwrap_or("");
    let c = &body["content"];
    let u = if op == "Login" { c } else { &c["user"] };
    let provider = u["user"].as_str().unwrap_or("");
    let check = async {
        let d = app
            .db
            .collection::<Document>("provider_deployments")
            .find_one(doc! {"provider_id":provider})
            .await
            .ok()??;
        if !authorized(op, c, provider, d.get_str("credential_sha256").ok()?) {
            return None;
        }
        let id = d.get_str("_id").ok()?;
        let p = app
            .db
            .collection::<Document>("projects")
            .find_one(doc! {"_id":id,"provider_id":provider})
            .await
            .ok()??;
        eligible(&app, &p).await.ok()?;
        let field = if op == "Login" {
            "last_login_at"
        } else {
            "last_proxy_at"
        };
        let mut set = Document::new();
        set.insert(field, crate::now());
        app.db
            .collection::<Document>("provider_deployments")
            .update_one(doc! {"_id":id}, doc! {"$set":set})
            .await
            .ok()?;
        Some(())
    }
    .await;
    Json(
        json!({"reject":check.is_none(),"reject_reason":if check.is_some(){""}else{"Project identity, route or stake not authorized"},"unchange":true}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_and_route_are_bound() {
        let id = "0xabc";
        let token = "private-project-key";
        let h = hex::encode(Sha256::digest(token.as_bytes()));
        let login = json!({"user":id,"metas":{"provider_token":token}});
        assert!(authorized("Login", &login, id, &h));
        assert!(!authorized("Login", &login, "other", &h));
        assert!(!authorized("Login", &login, id, &"00".repeat(32)));
        let route = format!(
            "p-{}.launchpad.internal",
            &hex::encode(Sha256::digest(id.as_bytes()))[..40]
        );
        let mut c = json!({"user":login,"proxy_type":"http","proxy_name":"0xabc.inference","custom_domains":[route]});
        assert!(authorized("NewProxy", &c, id, &h));
        c["custom_domains"] = json!(["another-route"]);
        assert!(!authorized("NewProxy", &c, id, &h));
        assert!(!authorized("Unknown", &c, id, &h));
    }
}
