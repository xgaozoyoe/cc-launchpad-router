use crate::{error, unavailable, ApiResult, App};
use axum::{http::StatusCode, Json};
use mongodb::bson::{doc, Document};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Instant;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    inference_api_key: String,
}
fn usage(v: &Value) -> bool {
    let Some(p) = v["prompt_tokens"].as_u64() else {
        return false;
    };
    let Some(c) = v["completion_tokens"].as_u64() else {
        return false;
    };
    let Some(t) = v["total_tokens"].as_u64() else {
        return false;
    };
    p > 0 && c > 0 && p.checked_add(c) == Some(t)
}
fn generation_failure(v: &Value) -> Option<String> {
    let content = v["choices"][0]["message"]["content"].as_str();
    let present = content.is_some_and(|s| !s.trim().is_empty());
    let reason = v["choices"][0]["finish_reason"].as_str();
    if present && reason == Some("stop") {
        return None;
    }
    // Only allow known metadata; never persist arbitrary provider strings or content.
    let reason = match reason {
        Some("stop") => "stop",
        Some("length") => "length",
        Some("tool_calls") => "tool_calls",
        Some("content_filter") => "content_filter",
        None => "missing",
        _ => "unrecognized",
    };
    let count = |key: &str| {
        v["usage"][key]
            .as_u64()
            .map(|n| n.to_string())
            .unwrap_or_else(|| "missing/invalid".into())
    };
    Some(format!("Generation failed: finish_reason={reason}; content={}; prompt_tokens={}; completion_tokens={}; total_tokens={}; max_tokens=48. {}",
        if present { "non-empty" } else if content.is_some() { "empty" } else { "missing/non-string" },
        count("prompt_tokens"), count("completion_tokens"), count("total_tokens"),
        if reason == "length" { "Output reached the token limit; no complete answer was returned." } else { "Expected non-empty text and finish_reason=stop." }))
}
async fn bounded(mut response: reqwest::Response) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Response transfer failed")?
    {
        if data.len() + chunk.len() > 262144 {
            return Err("Response exceeded the test size limit".into());
        }
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}
fn stream_valid(data: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(data) else {
        return false;
    };
    let (mut content, mut done, mut tokens) = (false, false, false);
    for line in text.lines() {
        if let Some(raw) = line.strip_prefix("data:") {
            let raw = raw.trim();
            if raw == "[DONE]" {
                done = true;
                continue;
            }
            let Ok(v) = serde_json::from_str::<Value>(raw) else {
                return false;
            };
            if v["choices"][0]["delta"]["content"]
                .as_str()
                .is_some_and(|s| !s.trim().is_empty())
            {
                content = true
            }
            if usage(&v["usage"]) {
                tokens = true
            }
        }
    }
    content && done && tokens
}
async fn probes(host: &str, key: &str, checks: &mut Vec<Value>) -> Result<(), String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(12))
        .no_proxy()
        .build()
        .map_err(|_| "Test client unavailable")?;
    let port = std::env::var("RPC_CHECK_PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse::<u16>()
        .map_err(|_| "Test ingress unavailable")?;
    let base = format!("http://127.0.0.1:{port}");
    let get = |path: &str| client.get(format!("{base}{path}")).header("Host", host);
    let r = get("/health")
        .send()
        .await
        .map_err(|_| "Assigned tunnel is unreachable")?;
    if r.status() != 200 {
        return Err("Assigned tunnel did not return a healthy inference API".into());
    }
    checks.push(json!({"id":"tunnel","passed":true,"detail":"Health request succeeded through the assigned FRPS route"}));
    for (id, bearer) in [
        ("missing_key", None),
        ("wrong_key", Some("invalid-platform-rpc-check-key")),
    ] {
        let mut req = get("/v1/models");
        if let Some(k) = bearer {
            req = req.bearer_auth(k)
        }
        let r = req
            .send()
            .await
            .map_err(|_| "Authentication request failed")?;
        if ![401, 403].contains(&r.status().as_u16()) {
            return Err("API did not reject missing or incorrect inference credentials".into());
        }
        checks.push(json!({"id":id,"passed":true,"detail":"Unauthorized request rejected"}));
    }
    let r = get("/v1/models")
        .bearer_auth(key)
        .send()
        .await
        .map_err(|_| "Authenticated model discovery failed")?;
    if r.status() != 200 {
        return Err("Provided inference API key was not accepted".into());
    }
    let v: Value =
        serde_json::from_slice(&bounded(r).await?).map_err(|_| "Invalid model discovery JSON")?;
    if !v["data"]
        .as_array()
        .is_some_and(|a| a.iter().any(|m| m["id"] == "qwen3-8b-awq"))
    {
        return Err("Expected served model ID was not advertised".into());
    }
    checks.push(json!({"id":"model","passed":true,"detail":"Authenticated API advertises qwen3-8b-awq; this does not attest model weights or GPU capacity"}));
    let body = json!({"model":"qwen3-8b-awq","messages":[{"role":"user","content":"Reply with one short greeting sentence."}],"temperature":0,"max_tokens":48,"chat_template_kwargs":{"enable_thinking":false}});
    let r = client
        .post(format!("{base}/v1/chat/completions"))
        .header("Host", host)
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .map_err(|_| "Generation request failed")?;
    if r.status() != 200 {
        return Err(format!(
            "Generation API returned HTTP {}; response body omitted",
            r.status().as_u16()
        ));
    }
    let v: Value =
        serde_json::from_slice(&bounded(r).await?).map_err(|_| "Invalid generation JSON")?;
    if let Some(detail) = generation_failure(&v) {
        checks.push(json!({"id":"generation","passed":false,"detail":detail}));
        return Err(detail);
    }
    checks.push(json!({"id":"generation","passed":true,"detail":"Complete non-streaming answer received; prompt/output content is not stored"}));
    if !usage(&v["usage"]) {
        return Err("Generated token usage is missing or inconsistent".into());
    }
    checks.push(json!({"id":"usage","passed":true,"detail":"Positive input/output token counts and total arithmetic are consistent"}));
    let mut body = body;
    body["stream"] = json!(true);
    body["stream_options"] = json!({"include_usage":true});
    let r = client
        .post(format!("{base}/v1/chat/completions"))
        .header("Host", host)
        .bearer_auth(key)
        .json(&body)
        .send()
        .await
        .map_err(|_| "Streaming request failed")?;
    if r.status() != 200
        || !r
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .is_some_and(|s| s.starts_with("text/event-stream"))
    {
        return Err("Streaming API did not return SSE".into());
    }
    if !stream_valid(&bounded(r).await?) {
        return Err("SSE content, completion marker or consistent final usage was missing".into());
    }
    checks.push(json!({"id":"streaming","passed":true,"detail":"SSE content, final token usage and completion marker received"}));
    Ok(())
}
pub async fn handle(
    app: &App,
    owners: Vec<String>,
    id: &str,
    action: &str,
    method: &str,
    body: &[u8],
) -> ApiResult {
    let p = app
        .db
        .collection::<Document>("projects")
        .find_one(doc! {"_id":id,"account_id":{"$in":owners}})
        .await
        .map_err(unavailable)?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Project not found"))?;
    let reports = app.db.collection::<Document>("rpc_checks");
    if action == "/rpc-check" && method == "GET" {
        let r = reports
            .find_one(doc! {"_id":id})
            .await
            .map_err(unavailable)?;
        return Ok(Json(
            json!({"report":r.as_ref().and_then(|d|d.get_document("report").ok()),"running":r.as_ref().and_then(|d|d.get_bool("running").ok()).unwrap_or(false)&&r.as_ref().and_then(|d|d.get_i64("lease_until").ok()).unwrap_or(0)>crate::now(),"capacity_accepted":false,"sales_enabled":false}),
        ));
    }
    if action != "/rpc-check" || method != "POST" {
        return Err(error(StatusCode::NOT_FOUND, "RPC check endpoint not found"));
    }
    let input: Input = serde_json::from_slice(body)
        .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide only inference_api_key"))?;
    run(app, &p, input.inference_api_key).await
}
// Both callers have already established owner/admin authorization; all probes and leases are shared.
pub(crate) async fn run(app: &App, p: &Document, key: String) -> ApiResult {
    let id = p.get_str("_id").map_err(unavailable)?;
    let reports = app.db.collection::<Document>("rpc_checks");
    if !crate::rpc_credentials::valid(&key) {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Invalid inference API key format",
        ));
    }
    crate::deployment::eligible(app, &p)
        .await
        .map_err(|e| error(StatusCode::CONFLICT, &e))?;
    if p.get_str("model_id").ok() != Some("qwen3-8b-awq") {
        return Err(error(
            StatusCode::BAD_REQUEST,
            "Model profile has no RPC test implementation",
        ));
    }
    let credential = app
        .db
        .collection::<Document>("provider_deployments")
        .find_one(doc! {"_id":id})
        .await
        .map_err(unavailable)?;
    if credential
        .as_ref()
        .and_then(|d| d.get_i64("last_proxy_at").ok())
        .is_none()
    {
        return Err(error(
            StatusCode::CONFLICT,
            "Connect the assigned FRPC proxy before testing",
        ));
    }
    let now = crate::now();
    let lease = uuid::Uuid::new_v4().to_string();
    let acquired = reports
        .update_one(
            doc! {"_id":id,"lease_until":{"$lte":now}},
            doc! {"$set":{"lease_until":now+65,"lease_id":&lease,"running":true}},
        )
        .upsert(true)
        .await;
    if let Err(e) = acquired {
        if e.to_string().contains("E11000") {
            return Err(error(
                StatusCode::TOO_MANY_REQUESTS,
                "A test is running or cooling down; retry shortly",
            ));
        }
        return Err(unavailable(e));
    }
    let host = format!(
        "p-{}.launchpad.internal",
        &hex::encode(Sha256::digest(
            p.get_str("provider_id").unwrap_or("").as_bytes()
        ))[..40]
    );
    let start = Instant::now();
    let mut checks = vec![];
    let result = match tokio::time::timeout(
        std::time::Duration::from_secs(45),
        probes(&host, &key, &mut checks),
    )
    .await
    {
        Ok(r) => r,
        Err(_) => Err("RPC check timed out".into()),
    };
    drop(key);
    let result = if result.is_ok() {
        crate::deployment::eligible(app, &p)
            .await
            .map_err(|_| "Stake eligibility changed before the test finished".to_string())
    } else {
        result
    };
    let failure = result.err();
    let names = [
        "tunnel",
        "missing_key",
        "wrong_key",
        "model",
        "generation",
        "usage",
        "streaming",
    ];
    for name in names {
        if !checks.iter().any(|c| c["id"] == name) {
            checks.push(json!({"id":name,"passed":false,"detail":"Failed or not reached; see overall failure"}));
        }
    }
    let report = json!({"status":if failure.is_none(){"passed"}else{"failed"},"checked_at":crate::now(),"duration_ms":start.elapsed().as_millis() as u64,"checks":checks,"failure":failure,"provider_id":p.get_str("provider_id").unwrap_or(""),"capacity_accepted":false,"sales_enabled":false});
    let record = mongodb::bson::to_document(&report).map_err(unavailable)?;
    let saved = reports
        .update_one(
            doc! {"_id":id,"lease_id":lease},
            doc! {"$set":{"report":record,"lease_until":crate::now()+10,"running":false}},
        )
        .await
        .map_err(unavailable)?;
    if saved.matched_count != 1 {
        return Err(error(
            StatusCode::CONFLICT,
            "Test lease changed; refresh status",
        ));
    }
    Ok(Json(
        json!({"report":report,"running":false,"capacity_accepted":false,"sales_enabled":false}),
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn generation_diagnostics_are_precise_and_do_not_echo_content() {
        let v = json!({"choices":[{"message":{"content":"private text"},"finish_reason":"length"}],"usage":{"prompt_tokens":12,"completion_tokens":48,"total_tokens":60}});
        let d = generation_failure(&v).unwrap();
        assert!(d.contains("finish_reason=length"));
        assert!(d.contains("completion_tokens=48"));
        assert!(!d.contains("private text"));
        assert!(generation_failure(
            &json!({"choices":[{"message":{"content":""},"finish_reason":"stop"}]})
        )
        .unwrap()
        .contains("content=empty"));
        assert!(generation_failure(
            &json!({"choices":[{"message":{"content":"ok"},"finish_reason":"stop"}]})
        )
        .is_none());
        assert!(
            !generation_failure(&json!({"choices":[{"finish_reason":"secret-value"}]}))
                .unwrap()
                .contains("secret-value")
        );
    }
    #[test]
    fn reject_bad_usage_and_incomplete_sse() {
        assert!(usage(
            &json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":5})
        ));
        assert!(!usage(
            &json!({"prompt_tokens":2,"completion_tokens":3,"total_tokens":6})
        ));
        assert!(!usage(
            &json!({"prompt_tokens":u64::MAX,"completion_tokens":1,"total_tokens":0})
        ));
        let text=b"data: {\"choices\":[{\"delta\":{\"content\":\"hello\"}}]}\n\ndata: {\"usage\":{\"prompt_tokens\":2,\"completion_tokens\":3,\"total_tokens\":5}}\n\ndata: [DONE]\n";
        assert!(stream_valid(text));
        assert!(!stream_valid(&text[..text.len() - 15]));
        assert!(!stream_valid(b"data: [DONE]\n"));
    }
}
