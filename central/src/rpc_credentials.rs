//! Project-bound secrets stay in an operator-private directory, never API responses/MongoDB.
use crate::{error, unavailable, ApiResult, App};
use axum::{http::StatusCode, Json};
use mongodb::bson::{doc, Document};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::PathBuf,
};
#[derive(Serialize, Deserialize)]
struct Credential {
    provider_id: String,
    key: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    inference_api_key: Option<String>,
    #[serde(default)]
    revoke: bool,
}
fn path(id: &str) -> Result<PathBuf, String> {
    if uuid::Uuid::parse_str(id).is_err() {
        return Err("Invalid project ID".into());
    }
    let root = PathBuf::from(
        std::env::var("RPC_CREDENTIAL_DIR")
            .map_err(|_| "Platform recheck credential storage is not configured")?,
    );
    if !root.is_absolute() {
        return Err("Invalid credential storage configuration".into());
    }
    if !root.exists() {
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .map_err(|_| "Credential storage unavailable")?;
    }
    let m = std::fs::symlink_metadata(&root).map_err(|_| "Credential storage unavailable")?;
    if !m.is_dir() || m.permissions().mode() & 0o077 != 0 {
        return Err("Credential directory must be private (0700)".into());
    }
    Ok(root.join(format!("{id}.json")))
}
use std::os::unix::fs::DirBuilderExt;
pub fn configured() -> bool {
    std::env::var("RPC_CREDENTIAL_DIR").is_ok()
}
pub fn valid(key: &str) -> bool {
    (8..=512).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_graphic())
}
pub fn load(p: &Document) -> Result<String, String> {
    let file = path(p.get_str("_id").map_err(|_| "Missing project ID")?)?;
    let m = std::fs::symlink_metadata(&file)
        .map_err(|_| "Provider has not authorized platform rechecks")?;
    if !m.is_file() || m.permissions().mode() & 0o077 != 0 || m.len() > 4096 {
        return Err("Invalid private credential file".into());
    }
    let secret: Credential =
        serde_json::from_slice(&std::fs::read(file).map_err(|_| "Credential unavailable")?)
            .map_err(|_| "Credential unavailable")?;
    if p.get_str("provider_id").ok() != Some(secret.provider_id.as_str()) || !valid(&secret.key) {
        return Err("Credential does not match this provider".into());
    }
    Ok(secret.key)
}
pub async fn handle(
    app: &App,
    owners: Vec<String>,
    id: &str,
    method: &str,
    body: &[u8],
) -> ApiResult {
    let p = app
        .db
        .collection::<Document>("projects")
        .find_one(doc! {"_id":id,"account_id":{"$in":&owners}})
        .await
        .map_err(unavailable)?
        .ok_or_else(|| error(StatusCode::NOT_FOUND, "Project not found"))?;
    if method == "GET" {
        return Ok(Json(
            json!({"storage_available":configured(),"authorized":load(&p).is_ok()}),
        ));
    }
    if method != "POST" {
        return Err(error(StatusCode::METHOD_NOT_ALLOWED, "Use GET or POST"));
    }
    let input: Input = serde_json::from_slice(body).map_err(|_| {
        error(
            StatusCode::BAD_REQUEST,
            "Provide inference_api_key or revoke:true",
        )
    })?;
    if input.revoke == input.inference_api_key.is_some() {
        return Err(error(StatusCode::BAD_REQUEST, "Choose save or revoke"));
    }
    let file = path(id).map_err(|e| error(StatusCode::SERVICE_UNAVAILABLE, &e))?;
    if input.revoke {
        match std::fs::remove_file(file) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => {
                return Err(error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Could not revoke credential",
                ))
            }
        }
    } else {
        crate::deployment::eligible(app, &p)
            .await
            .map_err(|e| error(StatusCode::CONFLICT, &e))?;
        let key = input.inference_api_key.unwrap_or_default();
        if !valid(&key) {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Invalid inference API key format",
            ));
        }
        let secret = Credential {
            provider_id: p.get_str("provider_id").map_err(unavailable)?.into(),
            key,
        };
        let temp = file.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let write = (|| -> std::io::Result<()> {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&temp)?;
            f.write_all(&serde_json::to_vec(&secret)?)?;
            f.sync_all()?;
            std::fs::rename(&temp, file)
        })();
        if write.is_err() {
            let _ = std::fs::remove_file(temp);
            return Err(error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Could not save private credential",
            ));
        }
    }
    app.db.collection::<Document>("rpc_credential_events").insert_one(doc!{"project_id":id,"actor":owners.first(),"at":crate::now(),"operation":if input.revoke{"revoke"}else{"authorize"}}).await.map_err(unavailable)?;
    Ok(Json(
        json!({"authorized":!input.revoke,"storage_available":true}),
    ))
}
