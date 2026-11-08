//! Bounded real inference previews. No browser URL, key, model, or generation parameters are trusted.
use crate::{error, now, unavailable, ApiResult, App};
use axum::{http::StatusCode, Json};
use mongodb::bson::{doc, DateTime, Document};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
const OUTPUT: u64 = 128;
const DAILY: i64 = 100;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    prompt: String,
    request_id: String,
}
fn limits() -> Value {
    json!({"per_minute":2,"per_day":DAILY,"project_per_hour":30,"project_per_day":200,"project_concurrency":1,"max_prompt_chars":500,"max_output_tokens":OUTPUT,"timeout_seconds":45})
}
fn reject(status: StatusCode, message: &str, retry: i64) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({"error":message,"retry_after_seconds":retry.max(1)})),
    )
}
fn subject(user: &str) -> String {
    hex::encode(Sha256::digest(user.as_bytes()))
}
fn valid_prompt(prompt: &str) -> bool {
    !prompt.trim().is_empty() && prompt.chars().count() <= 500 && prompt.len() <= 2000
}
async fn snapshot(app: &App, user: &str) -> Result<Value, mongodb::error::Error> {
    let at = now();
    let mut counts = vec![];
    for (kind, window) in [("minute", 60), ("day", 86400)] {
        let id = format!("user:{}:{kind}:{}", subject(user), at / window);
        let row = app
            .db
            .collection::<Document>("trial_quotas")
            .find_one(doc! {"_id":id})
            .await?;
        counts.push(
            row.and_then(|r| {
                r.get_i64("count")
                    .ok()
                    .or_else(|| r.get_i32("count").ok().map(i64::from))
            })
            .unwrap_or(0),
        );
    }
    Ok(
        json!({"minute_remaining":(2-counts[0]).max(0),"day_remaining":(DAILY-counts[1]).max(0),"minute_reset_at":(at/60+1)*60,"day_reset_at":(at/86400+1)*86400}),
    )
}
pub(crate) async fn take_lease(
    app: &App,
    id: &str,
    run: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    let r = app
        .db
        .collection::<Document>("trial_leases")
        .update_one(
            doc! {"_id":id,"until":{"$lte":now()}},
            doc! {"$set":{"until":now()+90,"run":run}},
        )
        .upsert(true)
        .await;
    match r {
        Ok(_) => Ok(()),
        Err(e) if e.to_string().contains("E11000") => Err(reject(
            StatusCode::TOO_MANY_REQUESTS,
            "A trial is already running for this account or project. Retry shortly.",
            10,
        )),
        Err(e) => Err(unavailable(e)),
    }
}
pub(crate) async fn release(app: &App, ids: &[String], run: &str) {
    for id in ids {
        let _ = app
            .db
            .collection::<Document>("trial_leases")
            .delete_one(doc! {"_id":id,"run":run})
            .await;
    }
}
async fn reserve(app: &App, user: &str, project: &str) -> Result<(), (StatusCode, Json<Value>)> {
    let at = now();
    let u = subject(user);
    for (prefix, window, max) in [
        (format!("user:{u}:minute"), 60, 2),
        (format!("user:{u}:day"), 86400, DAILY),
        (format!("project:{project}:hour"), 3600, 30),
        (format!("project:{project}:day"), 86400, 200),
    ] {
        let id = format!("{prefix}:{}", at / window);
        let result=app.db.collection::<Document>("trial_quotas").update_one(doc!{"_id":id,"count":{"$lt":max}},doc!{"$inc":{"count":1i64},"$setOnInsert":{"expires_at":DateTime::from_millis(((at/window+1)*window+172800)*1000)}}).upsert(true).await;
        match result {Ok(_)=>{},Err(e) if e.to_string().contains("E11000")=>return Err(reject(StatusCode::TOO_MANY_REQUESTS,"Trial quota reached. Wait until the quota resets; project capacity is also shared across users.",(at/window+1)*window-at)),Err(e)=>return Err(unavailable(e))}
    }
    Ok(())
}
pub(crate) async fn generate(
    p: &Document,
    key: &str,
    prompt: &str,
    max_output: u64,
) -> Result<Value, String> {
    let port = std::env::var("RPC_CHECK_PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse::<u16>()
        .map_err(|_| "Trial ingress unavailable")?;
    let host = format!(
        "p-{}.launchpad.internal",
        &hex::encode(Sha256::digest(
            p.get_str("provider_id").unwrap_or("").as_bytes()
        ))[..40]
    );
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .timeout(Duration::from_secs(40))
        .build()
        .map_err(|_| "Trial client unavailable")?;
    let mut r=client.post(format!("http://127.0.0.1:{port}/v1/chat/completions")).header("Host",host).bearer_auth(key).json(&json!({"model":"qwen3-8b-awq","messages":[{"role":"system","content":"You are a helpful assistant. Reply concisely."},{"role":"user","content":prompt}],"temperature":0.7,"max_tokens":max_output,"stream":false,"chat_template_kwargs":{"enable_thinking":false}})).send().await.map_err(|_|"Provider did not respond to the trial")?;
    if !r.status().is_success() {
        return Err("Provider could not complete this trial".into());
    }
    let mut bytes = vec![];
    while let Some(chunk) = r
        .chunk()
        .await
        .map_err(|_| "Trial response transfer failed")?
    {
        if bytes.len() + chunk.len() > 65536 {
            return Err("Provider response exceeded the trial limit".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    let v: Value = serde_json::from_slice(&bytes).map_err(|_| "Invalid provider response")?;
    let content = v["choices"][0]["message"]["content"]
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .ok_or("Provider returned an empty answer")?;
    let usage = &v["usage"];
    let a = usage["prompt_tokens"]
        .as_u64()
        .ok_or("Missing token accounting")?;
    let b = usage["completion_tokens"]
        .as_u64()
        .ok_or("Missing token accounting")?;
    let total = usage["total_tokens"]
        .as_u64()
        .ok_or("Missing token accounting")?;
    if a == 0 || b == 0 || b > max_output || a.checked_add(b) != Some(total) {
        return Err("Invalid trial token accounting".into());
    }
    let finish = v["choices"][0]["finish_reason"].as_str().unwrap_or("");
    if !["stop", "length"].contains(&finish) {
        return Err("Unsupported provider completion status".into());
    }
    Ok(
        json!({"text":content.replace(key,"[redacted]"),"usage":{"prompt_tokens":a,"completion_tokens":b,"total_tokens":total},"finish_reason":finish,"model_id":"qwen3-8b-awq"}),
    )
}
pub async fn handle(app: &App, user: &str, id: &str, method: &str, body: &[u8]) -> ApiResult {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(error(StatusCode::NOT_FOUND, "Project not found"));
    }
    let p = app
        .db
        .collection::<Document>("projects")
        .find_one(
            doc! {"_id":id,"status":"accepted","stake_finalized":true,"launch_id":{"$exists":true}},
        )
        .await
        .map_err(unavailable)?
        .ok_or_else(|| {
            error(
                StatusCode::CONFLICT,
                "This project has not completed finalized launch verification",
            )
        })?;
    crate::deployment::eligible(app, &p)
        .await
        .map_err(|e| error(StatusCode::CONFLICT, &e))?;
    if p.get_str("model_id").ok() != Some("qwen3-8b-awq") {
        return Err(error(
            StatusCode::CONFLICT,
            "This model profile does not support prompt trials yet",
        ));
    }
    let service = app.stake.as_ref().ok_or_else(|| {
        error(
            StatusCode::SERVICE_UNAVAILABLE,
            "Chain verification unavailable",
        )
    })?;
    if p.get_str("contract_group").is_err() {
        let finalized = service
            .state(p.get_str("onchain_key").map_err(unavailable)?, "finalized")
            .await
            .map_err(|_| {
                error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Finalized chain state unavailable",
                )
            })?;
        if finalized.0 != 3 {
            return Err(error(
                StatusCode::CONFLICT,
                "On-chain verification is not finalized",
            ));
        }
    }
    let key = crate::rpc_credentials::trial_key(&p).map_err(|e| error(StatusCode::CONFLICT, &e))?;
    if method == "GET" {
        return Ok(Json(
            json!({"project_id":id,"name":p.get_str("name").ok(),"model_id":p.get_str("model_id").ok(),"limits":limits(),"quota":snapshot(app,user).await.map_err(unavailable)?}),
        ));
    }
    if method != "POST" {
        return Err(error(StatusCode::METHOD_NOT_ALLOWED, "Use GET or POST"));
    }
    let input: Input = serde_json::from_slice(body).map_err(|_| {
        error(
            StatusCode::BAD_REQUEST,
            "Provide only prompt and request_id",
        )
    })?;
    if !valid_prompt(&input.prompt) || uuid::Uuid::parse_str(&input.request_id).is_err() {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Use a non-empty prompt of at most 500 characters and a UUID request_id",
        ));
    }
    let run = format!("{}:{}", subject(user), input.request_id);
    let lease_ids = vec![format!("user:{}", subject(user)), format!("project:{id}")];
    for lease in &lease_ids {
        if let Err(e) = take_lease(app, lease, &run).await {
            release(app, &lease_ids, &run).await;
            return Err(e);
        }
    }
    let existing = app
        .db
        .collection::<Document>("trial_requests")
        .find_one(doc! {"_id":&run})
        .await;
    match existing {
        Ok(None) => {}
        Ok(Some(_)) => {
            release(app, &lease_ids, &run).await;
            return Err(error(
                StatusCode::CONFLICT,
                "This trial request was already submitted. It will not generate a second answer.",
            ));
        }
        Err(e) => {
            release(app, &lease_ids, &run).await;
            return Err(unavailable(e));
        }
    }
    if let Err(e) = reserve(app, user, id).await {
        release(app, &lease_ids, &run).await;
        return Err(e);
    }
    let inserted=app.db.collection::<Document>("trial_requests").insert_one(doc!{"_id":&run,"project_id":id,"at":now(),"status":"requested","expires_at":DateTime::from_millis((now()+604800)*1000)}).await;
    if let Err(e) = inserted {
        if e.to_string().contains("E11000") {
            release(app, &lease_ids, &run).await;
            return Err(error(
                StatusCode::CONFLICT,
                "This trial request was already submitted. It will not generate a second answer.",
            ));
        }
        release(app, &lease_ids, &run).await;
        return Err(unavailable(e));
    }
    let start = Instant::now();
    let result = match tokio::time::timeout(
        Duration::from_secs(45),
        generate(&p, &key, input.prompt.trim(), OUTPUT),
    )
    .await
    {
        Ok(r) => r,
        Err(_) => Err("Trial timed out. The attempt still counts toward the quota.".into()),
    };
    drop(key);
    drop(input);
    let result = match result {
        Ok(value) => crate::deployment::eligible(app, &p)
            .await
            .map(|_| value)
            .map_err(|_| "Project eligibility changed during the trial".into()),
        Err(e) => Err(e),
    };
    release(app, &lease_ids, &run).await;
    let mut record = doc! {"status":if result.is_ok(){"completed"}else{"failed"},"duration_ms":start.elapsed().as_millis() as i64};
    if let Ok(value) = &result {
        record.insert(
            "usage",
            mongodb::bson::to_document(&value["usage"]).map_err(unavailable)?,
        );
    }
    app.db
        .collection::<Document>("trial_requests")
        .update_one(doc! {"_id":&run}, doc! {"$set":record})
        .await
        .map_err(unavailable)?;
    match result {
        Ok(mut value) => {
            value["duration_ms"] = json!(start.elapsed().as_millis() as u64);
            value["request_id"] = json!(run.split(':').next_back());
            value["quota"] = snapshot(app, user).await.map_err(unavailable)?;
            value["project_id"] = json!(id);
            Ok(Json(value))
        }
        Err(e) => Err(error(StatusCode::BAD_GATEWAY, &e)),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prompt_boundaries() {
        assert!(!valid_prompt("   "));
        assert!(valid_prompt(&"问".repeat(500)));
        assert!(!valid_prompt(&"a".repeat(501)));
    }
}
