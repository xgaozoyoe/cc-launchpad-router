mod admin;
mod auth;
mod deployment;
mod rpc_check;
mod rpc_credentials;
mod stake;
use axum::{
    body::Bytes,
    extract::{OriginalUri, State},
    http::{HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use futures_util::TryStreamExt;
use mongodb::{
    bson::{doc, Document},
    options::IndexOptions,
    Client, IndexModel,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::Arc;

struct App {
    db: mongodb::Database,
    key: Vec<u8>,
    stake: Option<stake::Stake>,
}
fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;
fn error(status: StatusCode, message: &str) -> (StatusCode, Json<Value>) {
    (status, Json(json!({"error":message})))
}
fn unavailable(_: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    error(StatusCode::SERVICE_UNAVAILABLE, "Database unavailable")
}
async fn health(State(app): State<Arc<App>>) -> ApiResult {
    app.db
        .run_command(doc! {"ping":1})
        .await
        .map_err(unavailable)?;
    Ok(Json(
        json!({"status":"ok","service":"cc-central","storage":"mongodb","sales_enabled":false}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Draft {
    name: String,
    model_id: String,
    request_id: String,
}
async fn api(
    State(app): State<Arc<App>>,
    OriginalUri(uri): OriginalUri,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if uri.query().is_some() {
        return error(
            StatusCode::BAD_REQUEST,
            "Query parameters are not supported",
        )
        .into_response();
    }
    match dispatch(app, uri.path(), method, headers, body).await {
        Ok(v) => v.into_response(),
        Err(e) => e.into_response(),
    }
}
async fn account_ids(app: &App, user: &str) -> Result<Vec<String>, mongodb::error::Error> {
    let links = app
        .db
        .collection::<Document>("account_links")
        .find(doc! {"canonical_id":user})
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    let mut ids = vec![user.to_string()];
    for link in links {
        if let Ok(id) = link.get_str("_id") {
            ids.push(id.to_string());
        }
    }
    Ok(ids)
}
// Join only public progress fields for the already owner-filtered project set.
async fn project_progress(app: &App, rows: &mut [Document]) -> Result<(), mongodb::error::Error> {
    let ids: Vec<String> = rows
        .iter()
        .filter_map(|p| p.get_str("_id").ok().map(str::to_string))
        .collect();
    if ids.is_empty() {
        return Ok(());
    }
    let deployments = app
        .db
        .collection::<Document>("provider_deployments")
        .find(doc! {"_id":{"$in":&ids}})
        .projection(doc! {"_id":1,"provider_id":1,"last_proxy_at":1})
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    let reports = app
        .db
        .collection::<Document>("rpc_checks")
        .find(doc! {"_id":{"$in":&ids}})
        .projection(doc! {"_id":1,"report.status":1,"report.checked_at":1,"report.provider_id":1})
        .await?
        .try_collect::<Vec<_>>()
        .await?;
    for p in rows {
        let id = p.get_str("_id").unwrap_or("");
        let provider = p.get_str("provider_id").unwrap_or("");
        let d = deployments.iter().find(|d| {
            d.get_str("_id").ok() == Some(id) && d.get_str("provider_id").ok() == Some(provider)
        });
        let r = reports
            .iter()
            .find(|d| d.get_str("_id").ok() == Some(id))
            .and_then(|d| d.get_document("report").ok())
            .filter(|r| !provider.is_empty() && r.get_str("provider_id").ok() == Some(provider));
        let mut progress = doc! {"bundle_issued":d.is_some()};
        if let Some(at) = d.and_then(|d| d.get_i64("last_proxy_at").ok()) {
            progress.insert("last_proxy_at", at);
        }
        if let Some(r) = r {
            progress.insert("rpc_status", r.get_str("status").unwrap_or("unknown"));
            if let Ok(at) = r.get_i64("checked_at") {
                progress.insert("rpc_checked_at", at);
            }
        }
        p.insert("progress", progress);
    }
    Ok(())
}
async fn dispatch(
    app: Arc<App>,
    path: &str,
    method: Method,
    headers: HeaderMap,
    body: Bytes,
) -> ApiResult {
    let mut identity = auth::verify(&app.key, method.as_str(), path, &headers, &body, now())
        .map_err(|_| error(StatusCode::UNAUTHORIZED, "Invalid signed request"))?;
    // Unique insert is the replay barrier across processes; TTL only performs cleanup.
    let nonce = doc! {"_id":format!("{}:{}",identity.user,identity.nonce),"expires_at":mongodb::bson::DateTime::from_millis((now()+180)*1000)};
    if let Err(e) = app
        .db
        .collection::<Document>("request_nonces")
        .insert_one(nonce)
        .await
    {
        if e.to_string().contains("E11000") {
            return Err(error(StatusCode::UNAUTHORIZED, "Request already used"));
        }
        return Err(unavailable(e));
    }
    if let Some(link) = app
        .db
        .collection::<Document>("account_links")
        .find_one(doc! {"_id":&identity.user})
        .await
        .map_err(unavailable)?
    {
        identity.user = link
            .get_str("canonical_id")
            .map_err(unavailable)?
            .to_string();
    }
    let owners = account_ids(&app, &identity.user)
        .await
        .map_err(unavailable)?;
    if let Some(action) = path.strip_prefix("/api/v1/admin/") {
        return admin::handle(&app, &identity.user, action, method.as_str(), &body).await;
    }
    if let Some(rest) = path.strip_prefix("/api/v1/projects/") {
        if let Some((id, action)) = rest.split_once("/deployment") {
            if action == "/rpc-credential" {
                return rpc_credentials::handle(&app, owners, id, method.as_str(), &body).await;
            }
            if action == "/rpc-check" {
                return rpc_check::handle(&app, owners, id, action, method.as_str(), &body).await;
            }
            return deployment::handle(&app, owners, id, action, method.as_str()).await;
        }
    }
    if let Some(rest) = path.strip_prefix("/api/v1/projects/") {
        if let Some((id, action)) = rest.split_once("/stake") {
            if uuid::Uuid::parse_str(id).is_err()
                || !(action.is_empty() || matches!(action, "/challenge" | "/prepare" | "/submit"))
            {
                return Err(error(StatusCode::NOT_FOUND, "Stake endpoint not found"));
            }
            let service = app.stake.as_ref().ok_or_else(|| {
                error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Stake service is not configured",
                )
            })?;
            return service
                .handle(
                    &app,
                    &identity.user,
                    id,
                    method.as_str(),
                    action.trim_start_matches('/'),
                    &body,
                )
                .await
                .map(Json)
                .map_err(|e| {
                    error(
                        if e == "Project not found" {
                            StatusCode::NOT_FOUND
                        } else if e.contains("unavailable") || e.starts_with("Chain RPC") {
                            StatusCode::SERVICE_UNAVAILABLE
                        } else {
                            StatusCode::BAD_REQUEST
                        },
                        &e,
                    )
                });
        }
    }
    match (method.as_str(), path) {
        ("POST", "/api/v1/account/link-legacy") => {
            let input: Value = serde_json::from_slice(&body)
                .map_err(|_| error(StatusCode::BAD_REQUEST, "Invalid account link"))?;
            let legacy = input
                .get("legacy_account_id")
                .and_then(Value::as_str)
                .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Invalid legacy account"))?;
            if !identity.user.starts_with("google:") || uuid::Uuid::parse_str(legacy).is_err() {
                return Err(error(StatusCode::BAD_REQUEST, "Invalid account link"));
            }
            let links = app.db.collection::<Document>("account_links");
            let result = links
                .update_one(
                    doc! {"_id":legacy},
                    doc! {"$setOnInsert":{"canonical_id":&identity.user,"created_at":now()}},
                )
                .upsert(true)
                .await;
            if let Err(e) = result {
                if !e.to_string().contains("E11000") {
                    return Err(unavailable(e));
                }
            }
            let link = links
                .find_one(doc! {"_id":legacy})
                .await
                .map_err(unavailable)?
                .ok_or_else(|| unavailable("Missing account link"))?;
            if link.get_str("canonical_id").ok() != Some(identity.user.as_str()) {
                return Err(error(StatusCode::CONFLICT, "Legacy account already linked"));
            }
            Ok(Json(json!({"linked":true})))
        }
        ("GET", "/api/v1/status") => {
            let chain = if let Some(service) = &app.stake {
                let id = format!(
                    "{}:{}",
                    service.config.chain_id, service.config.staking_contract
                );
                let row = app
                    .db
                    .collection::<Document>("chain_state")
                    .find_one(doc! {"_id":id})
                    .await
                    .map_err(unavailable)?;
                json!({"configured":true,"chain_id":service.config.chain_id,"staking_contract":service.config.staking_contract,"token_address":service.config.token_address,"finalized_block":row.as_ref().and_then(|d|d.get_i64("block").ok()),"halted":row.as_ref().and_then(|d|d.get_bool("halted").ok()).unwrap_or(false),"error":row.as_ref().and_then(|d|d.get_str("error").ok())})
            } else {
                json!({"configured":false})
            };
            Ok(Json(
                json!({"network":"sepolia","sales_enabled":false,"chain":chain,"wallet_verification":"project-bound EIP-191 signature","rpc_acceptance":"not_integrated"}),
            ))
        }
        ("GET", "/api/v1/models") => Ok(Json(
            json!({"models":[{"id":"qwen3-8b-awq","model":"Qwen/Qwen3-8B-AWQ","revision":"93eb32ea9ef98cc266b85563ad0dd29e5f2940b9","context":4096}]}),
        )),
        ("POST", "/api/v1/projects") => {
            let draft: Draft = serde_json::from_slice(&body)
                .map_err(|_| error(StatusCode::BAD_REQUEST, "Invalid draft"))?;
            if draft.name.trim().is_empty()
                || draft.name.chars().count() > 100
                || draft.model_id != "qwen3-8b-awq"
                || uuid::Uuid::parse_str(&draft.request_id).is_err()
            {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "Provide a name, supported model and UUID request_id",
                ));
            }
            let filter = doc! {"account_id":{"$in":owners.clone()},"request_id":&draft.request_id};
            let collection = app.db.collection::<Document>("projects");
            let record = doc! {"_id":uuid::Uuid::new_v4().to_string(),"account_id":&identity.user,"request_id":&draft.request_id,"name":draft.name.trim(),"model_id":&draft.model_id,"status":"draft","created_at":now()};
            // Atomic upsert plus unique index makes retried draft creation idempotent.
            let inserted = collection
                .update_one(filter.clone(), doc! {"$setOnInsert":record})
                .upsert(true)
                .await;
            if let Err(e) = inserted {
                if !e.to_string().contains("E11000") {
                    return Err(unavailable(e));
                }
            }
            let saved = collection
                .find_one(filter)
                .await
                .map_err(unavailable)?
                .ok_or_else(|| unavailable("missing draft"))?;
            if saved.get_str("name").unwrap_or("") != draft.name.trim()
                || saved.get_str("model_id").unwrap_or("") != draft.model_id
            {
                return Err(error(
                    StatusCode::CONFLICT,
                    "request_id already used with different parameters",
                ));
            }
            Ok(Json(json!({"project":saved})))
        }
        ("GET", "/api/v1/projects") => {
            let mut rows = app
                .db
                .collection::<Document>("projects")
                .find(doc! {"account_id":{"$in":owners.clone()}})
                .sort(doc! {"created_at":-1,"_id":1})
                .limit(100)
                .await
                .map_err(unavailable)?
                .try_collect::<Vec<_>>()
                .await
                .map_err(unavailable)?;
            project_progress(&app, &mut rows)
                .await
                .map_err(unavailable)?;
            Ok(Json(json!({"projects":rows})))
        }
        ("GET", p) if p.starts_with("/api/v1/projects/") => {
            let id = p.trim_start_matches("/api/v1/projects/");
            let row = app
                .db
                .collection::<Document>("projects")
                .find_one(doc! {"_id":id,"account_id":{"$in":owners.clone()}})
                .await
                .map_err(unavailable)?
                .ok_or_else(|| error(StatusCode::NOT_FOUND, "Project not found"))?;
            let mut rows = vec![row];
            project_progress(&app, &mut rows)
                .await
                .map_err(unavailable)?;
            Ok(Json(json!({"project":rows.remove(0)})))
        }
        _ => Err(error(StatusCode::NOT_FOUND, "Endpoint not available")),
    }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let key =
        hex::decode(std::fs::read_to_string(std::env::var("CENTRAL_HMAC_KEY_FILE")?)?.trim())?;
    if key.len() != 32 {
        return Err("HMAC key must contain 32 bytes encoded as hex".into());
    }
    let client = Client::with_uri_str(std::env::var("MONGODB_URI")?).await?;
    let db = client.database(&std::env::var("MONGODB_DATABASE").unwrap_or("launchpad".into()));
    let hello = db.run_command(doc! {"hello":1}).await?;
    if !hello.contains_key("setName") && hello.get_str("msg").unwrap_or("") != "isdbgrid" {
        return Err("MongoDB replica set or Atlas required for future stake transactions".into());
    }
    db.collection::<Document>("projects")
        .create_index(
            IndexModel::builder()
                .keys(doc! {"account_id":1,"request_id":1})
                .options(IndexOptions::builder().unique(true).build())
                .build(),
        )
        .await?;
    db.collection::<Document>("request_nonces")
        .create_index(
            IndexModel::builder()
                .keys(doc! {"expires_at":1})
                .options(
                    IndexOptions::builder()
                        .expire_after(std::time::Duration::ZERO)
                        .build(),
                )
                .build(),
        )
        .await?;
    for field in ["onchain_key", "provider_id", "launch_id"] {
        db.collection::<Document>("projects")
            .create_index(
                IndexModel::builder()
                    .keys({
                        let mut keys = Document::new();
                        keys.insert(field, 1);
                        keys
                    })
                    .options(IndexOptions::builder().unique(true).sparse(true).build())
                    .build(),
            )
            .await?;
    }
    let stake = stake::Stake::load().await.map_err(std::io::Error::other)?;
    let app = Arc::new(App { db, key, stake });
    tokio::spawn(stake::run(app.clone()));
    let routes = Router::new()
        .route("/healthz", get(health))
        .route(
            "/internal/frp/handler",
            axum::routing::post(deployment::hook),
        )
        .route("/api/v1/{*path}", get(api).post(api))
        .layer(axum::extract::DefaultBodyLimit::max(65536))
        .layer(axum::middleware::from_fn(
            |req: axum::extract::Request, next: axum::middleware::Next| async move {
                let mut r = next.run(req).await;
                r.headers_mut().insert(
                    "cache-control",
                    axum::http::HeaderValue::from_static("no-store"),
                );
                r
            },
        ))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(
        std::env::var("BIND_ADDRESS").unwrap_or("127.0.0.1:9001".into()),
    )
    .await?;
    println!("cc-central ready; sales remain disabled");
    axum::serve(listener, routes)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

#[cfg(test)]
mod integration {
    use super::*;
    use hmac::{Hmac, Mac};
    use sha2::{Digest, Sha256};
    fn headers(user: &str, path: &str, method: &str, body: &[u8]) -> HeaderMap {
        let stamp = now().to_string();
        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let text = format!(
            "{method}\n{path}\n{user}\n{stamp}\n{nonce}\n{}",
            hex::encode(Sha256::digest(body))
        );
        let mut mac = Hmac::<Sha256>::new_from_slice(&[7; 32]).unwrap();
        mac.update(text.as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());
        let mut h = HeaderMap::new();
        for (k, v) in [
            ("x-cc-user", user),
            ("x-cc-timestamp", &stamp),
            ("x-cc-nonce", &nonce),
            ("x-cc-signature", &sig),
        ] {
            h.insert(k, v.parse().unwrap());
        }
        h
    }
    #[tokio::test]
    #[ignore = "requires disposable local MongoDB with --replSet rs0 on port 27187"]
    async fn mongodb_project_isolation_and_replay() {
        let client = Client::with_uri_str("mongodb://127.0.0.1:27187/?directConnection=true")
            .await
            .unwrap();
        let admin = client.database("admin");
        let hello = admin.run_command(doc! {"hello":1}).await.unwrap();
        if !hello.contains_key("setName") {
            admin.run_command(doc!{"replSetInitiate":{"_id":"rs0","members":[{"_id":0,"host":"127.0.0.1:27187"}]}}).await.unwrap();
        }
        for _ in 0..100 {
            if admin
                .run_command(doc! {"hello":1})
                .await
                .unwrap()
                .get_bool("isWritablePrimary")
                .unwrap_or(false)
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let db = client.database(&format!("cc_test_{}", uuid::Uuid::new_v4().simple()));
        db.collection::<Document>("projects")
            .create_index(
                IndexModel::builder()
                    .keys(doc! {"account_id":1,"request_id":1})
                    .options(IndexOptions::builder().unique(true).build())
                    .build(),
            )
            .await
            .unwrap();
        let app = Arc::new(App {
            db: db.clone(),
            key: vec![7; 32],
            stake: None,
        });
        let path = "/api/v1/projects";
        let body=serde_json::to_vec(&json!({"name":"My Qwen","model_id":"qwen3-8b-awq","request_id":uuid::Uuid::new_v4().to_string()})).unwrap();
        let h = headers("alice", path, "POST", &body);
        let first = dispatch(
            app.clone(),
            path,
            Method::POST,
            h.clone(),
            body.clone().into(),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(
            dispatch(app.clone(), path, Method::POST, h, body.clone().into())
                .await
                .unwrap_err()
                .0,
            StatusCode::UNAUTHORIZED
        );
        let again = dispatch(
            app.clone(),
            path,
            Method::POST,
            headers("alice", path, "POST", &body),
            body.into(),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(first, again);
        assert_eq!(
            db.collection::<Document>("projects")
                .count_documents(doc! {})
                .await
                .unwrap(),
            1
        );
        let id = first["project"]["_id"].as_str().unwrap();
        let detail = format!("{path}/{id}");
        assert_eq!(
            dispatch(
                app.clone(),
                &detail,
                Method::GET,
                headers("bob", &detail, "GET", b""),
                Bytes::new()
            )
            .await
            .unwrap_err()
            .0,
            StatusCode::NOT_FOUND
        );
        let list = dispatch(
            app.clone(),
            path,
            Method::GET,
            headers("bob", path, "GET", b""),
            Bytes::new(),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(list["projects"], json!([]));
        // A fresh application instance reads persisted data, not process memory.
        let restarted = Arc::new(App {
            db: db.clone(),
            key: vec![7; 32],
            stake: None,
        });
        assert!(dispatch(
            restarted,
            &detail,
            Method::GET,
            headers("alice", &detail, "GET", b""),
            Bytes::new()
        )
        .await
        .is_ok());
        db.drop().await.unwrap();
    }
}
