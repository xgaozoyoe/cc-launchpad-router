//! Per-project modular-credit API. Legacy records keep their original contract and ledger epoch.
use crate::{
    credits as abi, error, now,
    protocol_groups::{self, Group},
    stake, unavailable, ApiResult, App,
};
use axum::{http::StatusCode, Json};
use futures_util::TryStreamExt;
use mongodb::bson::{doc, DateTime, Document};
use serde::Deserialize;
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
type R<T> = Result<T, String>;
fn problem(e: String) -> (StatusCode, Json<Value>) {
    error(StatusCode::CONFLICT, &e)
}
pub(crate) async fn call(
    app: &App,
    g: &Group,
    component: &str,
    signature: &str,
    args: &[String],
    tag: &str,
) -> R<String> {
    app.stake.as_ref().ok_or("Chain unavailable")?.rpc("eth_call",json!([{"to":g.contracts.get(component).ok_or("Unknown component")?,"data":abi::data(signature,args)},tag])).await?.as_str().map(str::to_owned).ok_or_else(||"Invalid chain response".into())
}
pub(crate) async fn group(app: &App, p: &Document) -> R<Group> {
    let g = protocol_groups::select(
        app,
        p.get_str("contract_group")
            .map_err(|_| "Project has no contract group")?,
    )?;
    if p.get_str("ledger_epoch").unwrap_or("").is_empty()
        || !p.get_bool("migration_finalized").unwrap_or(false)
    {
        return Err("Migration is not finalized and reconciled".into());
    }
    let row = app
        .db
        .collection::<Document>("protocol_group_state")
        .find_one(doc! {"_id":&g.id,"manifest_digest":g.digest(),"halted":false})
        .await
        .map_err(|_| "Contract monitor unavailable")?;
    if row.is_none() {
        return Err("Contract group monitor is not ready".into());
    }
    Ok(g)
}
pub(crate) async fn service(app: &App, p: &Document) -> R<()> {
    if p.get_str("migration_phase").ok() == Some("frozen") {
        return Err("Project is paused for migration".into());
    }
    let g = group(app, p).await?;
    let key = stake::project_key(p.get_str("_id").map_err(|_| "Missing project")?);
    let terms = call(
        app,
        &g,
        "registry",
        "project(bytes32)",
        &[key[2..].into()],
        "finalized",
    )
    .await?;
    if abi::uint(abi::w(&terms, 7)?)? != 3 || abi::uint(abi::w(&terms, 9)?)? != 0 {
        return Err("Modular project service is not available".into());
    }
    Ok(())
}
pub(crate) async fn offer(app: &App, p: &Document, tag: &str) -> R<abi::Offer> {
    let g = group(app, p).await?;
    let key = stake::project_key(p.get_str("_id").map_err(|_| "Missing project")?);
    let terms = call(
        app,
        &g,
        "registry",
        "project(bytes32)",
        &[key[2..].into()],
        tag,
    )
    .await?;
    let raw = call(
        app,
        &g,
        "credit_token",
        "issued(bytes32)",
        &[key[2..].into()],
        tag,
    )
    .await?;
    let provider = format!("0x{}", &abi::w(&terms, 0)?[24..]);
    let expected = hex::encode(Keccak256::digest(
        p.get_str("model_id").unwrap_or("").as_bytes(),
    ));
    if abi::w(&terms, 4)? != expected || p.get_str("wallet_address").unwrap_or("") != provider {
        return Err("Project terms/provider mismatch".into());
    }
    Ok(abi::Offer {
        provider,
        supply: abi::uint(abi::w(&terms, 2)?)?,
        issued: abi::uint(&raw)?,
        price: abi::uint(abi::w(&terms, 3)?)?,
        active: abi::uint(abi::w(&terms, 7)?)? == 3
            && abi::uint(abi::w(&terms, 8)?)? == 0
            && abi::uint(abi::w(&terms, 9)?)? == 0,
    })
}
async fn account(app: &App, g: &Group, id: &str, wallet: &str, tag: &str) -> R<[u128; 5]> {
    let raw = call(
        app,
        g,
        "usage_escrow",
        "accounts(bytes32,address)",
        &[stake::project_key(id)[2..].into(), abi::addr(wallet)],
        tag,
    )
    .await?;
    let mut values = [0; 5];
    for (i, v) in values.iter_mut().enumerate() {
        *v = abi::uint(abi::w(&raw, i)?)?;
    }
    Ok(values)
}
fn tx(g: &Group, wallet: &str, component: &str, data: String, value: u128) -> Value {
    json!({"from":wallet,"to":g.contracts[component],"chainId":format!("0x{:x}",g.chain_id),"data":data,"value":format!("0x{value:x}")})
}
fn valid_hash(v: &str) -> bool {
    v.len() == 66 && v.starts_with("0x") && v[2..].bytes().all(|b| b.is_ascii_hexdigit())
}
async fn receipt(app: &App, hash: &str) -> R<Value> {
    let rpc = app.stake.as_ref().ok_or("Chain unavailable")?;
    let r = rpc.rpc("eth_getTransactionReceipt", json!([hash])).await?;
    if r.is_null() {
        return Ok(json!({"status":"submitted"}));
    }
    let block = rpc
        .rpc("eth_getBlockByNumber", json!([r["blockNumber"], false]))
        .await?;
    if block["hash"] != r["blockHash"] {
        return Ok(json!({"status":"reorg-pending"}));
    }
    let head = rpc
        .rpc("eth_getBlockByNumber", json!(["finalized", false]))
        .await?;
    let finalized = abi::uint(head["number"].as_str().ok_or("Invalid finality head")?)?
        >= abi::uint(r["blockNumber"].as_str().ok_or("Invalid receipt block")?)?;
    Ok(
        json!({"status":if r["status"]!=json!("0x1"){"reverted"}else if finalized{"finalized"}else{"mined"},"block":r["blockNumber"],"block_hash":r["blockHash"],"finalized_block":head["number"]}),
    )
}
async fn track_rows(app: &App, g: &Group, id: &str, wallet: &str) -> R<Vec<Value>> {
    let rows = app
        .db
        .collection::<Document>("modular_transactions")
        .find(doc! {"group":&g.id,"project_id":id,"wallet":wallet})
        .sort(doc! {"at":-1})
        .limit(50)
        .await
        .map_err(|_| "Transaction tracking unavailable")?
        .try_collect::<Vec<_>>()
        .await
        .map_err(|_| "Transaction tracking unavailable")?;
    let mut out = vec![];
    for row in rows {
        let h = row
            .get_str("tx_hash")
            .map_err(|_| "Invalid transaction record")?;
        let status = receipt(app, h).await?;
        out.push(json!({"tx_hash":h,"kind":row.get_str("kind").unwrap_or("transaction"),"at":row.get_i64("at").unwrap_or(0),"progress":status}));
    }
    Ok(out)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Quantity {
    quantity: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reserve {
    maximum: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Infer {
    reservation_id: String,
    prompt: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submission {
    tx_hash: String,
    kind: String,
    quantity: Option<u64>,
    reservation_id: Option<String>,
}
async fn reservation(app: &App, g: &Group, id: &str, tag: &str) -> R<String> {
    call(
        app,
        g,
        "usage_escrow",
        "reservations(bytes32)",
        &[id.trim_start_matches("0x").into()],
        tag,
    )
    .await
}
fn reservation_open(raw: &str, project: &str, wallet: &str, maximum: u64) -> R<bool> {
    Ok(abi::w(raw, 0)? == &stake::project_key(project)[2..]
        && format!("0x{}", &abi::w(raw, 1)?[24..]) == wallet
        && abi::uint(abi::w(raw, 2)?)? == maximum as u128
        && abi::uint(abi::w(raw, 4)?)? == 0)
}
pub async fn state(app: &App, user: &str, id: &str, p: &Document) -> ApiResult {
    let g = group(app, p).await.map_err(problem)?;
    let wallet = abi::wallet(app, user).await?;
    let a = account(app, &g, id, &wallet, "finalized")
        .await
        .map_err(problem)?;
    let latest = account(app, &g, id, &wallet, "latest")
        .await
        .map_err(problem)?;
    let o = offer(app, p, "finalized").await.map_err(problem)?;
    let requests=app.db.collection::<Document>("modular_requests").find(doc!{"group":&g.id,"project_id":id,"wallet":&wallet,"epoch":p.get_str("ledger_epoch").unwrap_or("")}).projection(doc!{"raw":0}).sort(doc!{"at":-1}).limit(50).await.map_err(unavailable)?.try_collect::<Vec<_>>().await.map_err(unavailable)?;
    let consumed_pending = requests
        .iter()
        .filter(|r| {
            matches!(
                r.get_str("status").unwrap_or(""),
                "consumed" | "settling" | "settlement-error"
            )
        })
        .map(|r| r.get_i64("amount").unwrap_or(0).max(0) as u128)
        .sum::<u128>()
        .max(latest[2].saturating_sub(a[2]));
    let available = a[0]
        .saturating_sub(a[1])
        .min(latest[0].saturating_sub(latest[1]));
    let raw = call(
        app,
        &g,
        "payment_escrow",
        "funds(bytes32)",
        &[stake::project_key(id)[2..].into()],
        "finalized",
    )
    .await
    .map_err(problem)?;
    Ok(Json(
        json!({"protocol":"modular","contract_group":g.id,"ledger_epoch":p.get_str("ledger_epoch").ok(),"project_id":id,"name":p.get_str("name").unwrap_or("Launch"),"model_id":p.get_str("model_id").unwrap_or(""),"provider":o.provider,"wallet":wallet,"chain_id":g.chain_id,"contract":g.contracts["credit_token"],"contracts":g.contracts,"available":available.to_string(),"reserved":latest[1].to_string(),"consumed_pending":consumed_pending.to_string(),"finalized":{"locked":a[0].to_string(),"burned":a[2].to_string(),"deposited":a[3].to_string(),"withdrawn":a[4].to_string()},"supply":o.supply.to_string(),"issued":o.issued.to_string(),"price_wei_per_credit":o.price.to_string(),"sales_open":o.active,"earned_wei":abi::uint(abi::w(&raw,2).map_err(problem)?).map_err(problem)?.to_string(),"requests":requests,"transactions":track_rows(app,&g,id,&wallet).await.map_err(problem)?}),
    ))
}
pub async fn handle(app: &App, user: &str, path: &str, method: &str, body: &[u8]) -> ApiResult {
    let (id, action) = path.split_once('/').unwrap_or((path, ""));
    if uuid::Uuid::parse_str(id).is_err() {
        return Err(error(StatusCode::NOT_FOUND, "Unknown modular project"));
    }
    let p = abi::project(app, id).await?;
    if ["consent-plan", "consent"].contains(&action) {
        return crate::migration::consent(app, user, &p, action, method, body).await;
    }
    if action == "info" && method == "GET" && p.get_str("migration_phase").ok() == Some("frozen") {
        return Ok(Json(json!({"protocol":"migration"})));
    }
    if p.get_str("contract_group").is_err() {
        return Err(error(
            StatusCode::NOT_FOUND,
            "Project has no modular ledger",
        ));
    }
    if method == "GET" && action == "info" {
        let g = group(app, &p).await.map_err(problem)?;
        return Ok(Json(json!({"protocol":"modular","group_id":g.id})));
    }
    if method == "GET" && action.is_empty() {
        return state(app, user, id, &p).await;
    }
    if method != "POST" {
        return Err(error(StatusCode::METHOD_NOT_ALLOWED, "Use POST"));
    }
    let g = group(app, &p).await.map_err(problem)?;
    let wallet = abi::wallet(app, user).await?;
    let key = stake::project_key(id);
    if action == "transactions" {
        let input: Submission = serde_json::from_slice(body)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide transaction details"))?;
        if !valid_hash(&input.tx_hash) {
            return Err(error(StatusCode::BAD_REQUEST, "Invalid transaction hash"));
        }
        let chain = app
            .stake
            .as_ref()
            .ok_or_else(|| problem("Chain unavailable".into()))?
            .rpc("eth_getTransactionByHash", json!([input.tx_hash]))
            .await
            .map_err(problem)?;
        let expected = match input.kind.as_str() {
            "purchase" => tx(
                &g,
                &wallet,
                "sale_router",
                abi::data(
                    "buyAndLock(bytes32,uint256)",
                    &[
                        key[2..].into(),
                        abi::word(input.quantity.unwrap_or(0) as u128),
                    ],
                ),
                input.quantity.unwrap_or(0) as u128
                    * offer(app, &p, "latest").await.map_err(problem)?.price,
            ),
            "withdraw" => tx(
                &g,
                &wallet,
                "usage_escrow",
                abi::data(
                    "withdraw(bytes32,uint256)",
                    &[
                        key[2..].into(),
                        abi::word(input.quantity.unwrap_or(0) as u128),
                    ],
                ),
                0,
            ),
            "reserve" => {
                let rid = input.reservation_id.as_deref().unwrap_or("");
                let row = app
                    .db
                    .collection::<Document>("modular_requests")
                    .find_one(doc! {"_id":rid,"group":&g.id,"project_id":id,"wallet":&wallet})
                    .await
                    .map_err(unavailable)?
                    .ok_or_else(|| error(StatusCode::NOT_FOUND, "Reservation not found"))?;
                tx(
                    &g,
                    &wallet,
                    "usage_escrow",
                    row.get_str("calldata").map_err(unavailable)?.into(),
                    0,
                )
            }
            "deposit" => tx(
                &g,
                &wallet,
                "credit_token",
                abi::data(
                    "safeTransferFrom(address,address,uint256,uint256,bytes)",
                    &[
                        abi::addr(&wallet),
                        abi::addr(&g.contracts["usage_escrow"]),
                        key[2..].into(),
                        abi::word(input.quantity.unwrap_or(0) as u128),
                        abi::word(160),
                        abi::word(0),
                    ],
                ),
                0,
            ),
            "claim-revenue" => {
                let data = chain["input"].as_str().unwrap_or("");
                if wallet != p.get_str("wallet_address").unwrap_or("")
                    || data.len() != 138
                    || !data.starts_with(&format!(
                        "0x{}{}",
                        abi::sel("claimRevenue(bytes32,uint256)"),
                        &key[2..]
                    ))
                {
                    return Err(error(
                        StatusCode::CONFLICT,
                        "Revenue claim does not match provider/project",
                    ));
                }
                tx(&g, &wallet, "payment_escrow", data.into(), 0)
            }
            "release" => {
                let rid = input
                    .reservation_id
                    .as_deref()
                    .filter(|r| valid_hash(r))
                    .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Invalid reservation"))?;
                tx(
                    &g,
                    &wallet,
                    "usage_escrow",
                    abi::data("releaseExpired(bytes32)", &[rid[2..].into()]),
                    0,
                )
            }
            _ => {
                return Err(error(
                    StatusCode::BAD_REQUEST,
                    "Unsupported transaction kind",
                ))
            }
        };
        for (field, want) in [
            ("from", "from"),
            ("to", "to"),
            ("input", "data"),
            ("value", "value"),
            ("chainId", "chainId"),
        ] {
            if chain[field].as_str().map(str::to_lowercase)
                != expected[want].as_str().map(str::to_lowercase)
            {
                return Err(error(
                    StatusCode::CONFLICT,
                    "Transaction does not match wallet, project and action",
                ));
            }
        }
        app.db.collection::<Document>("modular_transactions").update_one(doc!{"_id":format!("{}:{}",g.id,input.tx_hash.to_lowercase())},doc!{"$setOnInsert":{"group":&g.id,"project_id":id,"wallet":&wallet,"kind":&input.kind,"tx_hash":input.tx_hash.to_lowercase(),"at":now()}}).upsert(true).await.map_err(unavailable)?;
        if input.kind == "reserve" {
            app.db
                .collection::<Document>("modular_requests")
                .update_one(
                    doc! {"_id":input.reservation_id.unwrap_or_default(),"wallet":&wallet},
                    doc! {"$set":{"reservation_tx":input.tx_hash.to_lowercase()}},
                )
                .await
                .map_err(unavailable)?;
        }
        return Ok(Json(json!({"tracked":true})));
    }
    if ["purchase", "reserve", "infer", "deposit"].contains(&action) {
        service(app, &p).await.map_err(problem)?;
    }
    if ["purchase", "withdraw", "deposit", "claim-revenue"].contains(&action) {
        let q = if action == "claim-revenue" {
            0
        } else {
            serde_json::from_slice::<Quantity>(body)
                .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide quantity"))?
                .quantity
        };
        if action != "claim-revenue" && q == 0 {
            return Err(error(StatusCode::BAD_REQUEST, "Positive quantity required"));
        }
        let transaction = match action {
            "purchase" => {
                let o = offer(app, &p, "latest").await.map_err(problem)?;
                if !o.active || q as u128 > o.supply.saturating_sub(o.issued) {
                    return Err(error(StatusCode::CONFLICT, "Offering unavailable"));
                }
                tx(
                    &g,
                    &wallet,
                    "sale_router",
                    abi::data(
                        "buyAndLock(bytes32,uint256)",
                        &[key[2..].into(), abi::word(q as u128)],
                    ),
                    q as u128 * o.price,
                )
            }
            "withdraw" => tx(
                &g,
                &wallet,
                "usage_escrow",
                abi::data(
                    "withdraw(bytes32,uint256)",
                    &[key[2..].into(), abi::word(q as u128)],
                ),
                0,
            ),
            "deposit" => tx(
                &g,
                &wallet,
                "credit_token",
                abi::data(
                    "safeTransferFrom(address,address,uint256,uint256,bytes)",
                    &[
                        abi::addr(&wallet),
                        abi::addr(&g.contracts["usage_escrow"]),
                        key[2..].into(),
                        abi::word(q as u128),
                        abi::word(160),
                        abi::word(0),
                    ],
                ),
                0,
            ),
            _ => {
                if wallet != p.get_str("wallet_address").unwrap_or("") {
                    return Err(error(StatusCode::FORBIDDEN, "Only provider"));
                }
                let raw = call(
                    app,
                    &g,
                    "payment_escrow",
                    "funds(bytes32)",
                    &[key[2..].into()],
                    "latest",
                )
                .await
                .map_err(problem)?;
                tx(
                    &g,
                    &wallet,
                    "payment_escrow",
                    abi::data(
                        "claimRevenue(bytes32,uint256)",
                        &[
                            key[2..].into(),
                            abi::w(&raw, 2).map_err(|e| problem(e))?.into(),
                        ],
                    ),
                    0,
                )
            }
        };
        app.db
            .collection::<Document>("credit_positions")
            .update_one(
                doc! {"_id":format!("{wallet}:{id}")},
                doc! {"$setOnInsert":{"wallet":&wallet,"project_id":id}},
            )
            .upsert(true)
            .await
            .map_err(unavailable)?;
        return Ok(Json(json!({"transaction":transaction})));
    }
    if action == "reserve" {
        let input: Reserve = serde_json::from_slice(body)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide maximum"))?;
        if !(16..=256).contains(&input.maximum) {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Use 16–256 maximum output tokens",
            ));
        }
        let a = account(app, &g, id, &wallet, "finalized")
            .await
            .map_err(problem)?;
        let l = account(app, &g, id, &wallet, "latest")
            .await
            .map_err(problem)?;
        if input.maximum as u128 > a[0].saturating_sub(a[1]).min(l[0].saturating_sub(l[1])) {
            return Err(error(
                StatusCode::CONFLICT,
                "Insufficient available credits",
            ));
        }
        let head = app
            .stake
            .as_ref()
            .unwrap()
            .rpc("eth_getBlockByNumber", json!(["latest", false]))
            .await
            .map_err(problem)?;
        let expiry = abi::uint(head["timestamp"].as_str().unwrap_or("")).map_err(problem)? + 3600;
        let rid = format!(
            "0x{}",
            hex::encode(Keccak256::digest(uuid::Uuid::new_v4().as_bytes()))
        );
        let data = abi::data(
            "reserve(bytes32,bytes32,uint256,uint64)",
            &[
                key[2..].into(),
                rid[2..].into(),
                abi::word(input.maximum as u128),
                abi::word(expiry),
            ],
        );
        app.db.collection::<Document>("modular_requests").insert_one(doc!{"_id":&rid,"group":&g.id,"epoch":p.get_str("ledger_epoch").unwrap_or(""),"project_id":id,"wallet":&wallet,"status":"prepared","maximum":input.maximum as i64,"expiry":expiry as i64,"amount":0_i64,"calldata":&data,"at":now()}).await.map_err(unavailable)?;
        return Ok(Json(
            json!({"reservation_id":rid,"expires_at":expiry,"transaction":tx(&g,&wallet,"usage_escrow",data,0)}),
        ));
    }
    if action == "release" {
        let input: Value = serde_json::from_slice(body)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide reservation_id"))?;
        let rid = input["reservation_id"]
            .as_str()
            .filter(|r| valid_hash(r))
            .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Invalid reservation"))?;
        let row = app
            .db
            .collection::<Document>("modular_requests")
            .find_one(doc! {"_id":rid,"group":&g.id,"project_id":id,"wallet":&wallet})
            .await
            .map_err(unavailable)?
            .ok_or_else(|| error(StatusCode::NOT_FOUND, "Reservation not found"))?;
        let head = app
            .stake
            .as_ref()
            .unwrap()
            .rpc("eth_getBlockByNumber", json!(["latest", false]))
            .await
            .map_err(problem)?;
        if abi::uint(head["timestamp"].as_str().unwrap_or("")).map_err(problem)?
            < row.get_i64("expiry").map_err(unavailable)? as u128
        {
            return Err(error(StatusCode::CONFLICT, "Reservation has not expired"));
        }
        return Ok(Json(
            json!({"transaction":tx(&g,&wallet,"usage_escrow",abi::data("releaseExpired(bytes32)",&[rid[2..].into()]),0)}),
        ));
    }
    if action == "infer" {
        let input: Infer = serde_json::from_slice(body)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide reservation_id and prompt"))?;
        if !valid_hash(&input.reservation_id)
            || input.prompt.trim().is_empty()
            || input.prompt.chars().count() > 500
        {
            return Err(error(
                StatusCode::BAD_REQUEST,
                "Invalid reservation or prompt",
            ));
        }
        let rid = &input.reservation_id;
        let requests = app.db.collection::<Document>("modular_requests");
        let row=requests.find_one(doc!{"_id":rid,"group":&g.id,"project_id":id,"wallet":&wallet,"epoch":p.get_str("ledger_epoch").unwrap_or("")}).await.map_err(unavailable)?.ok_or_else(||error(StatusCode::NOT_FOUND,"Reservation not found"))?;
        if row.get_str("status").ok() != Some("prepared") {
            let cached = app
                .db
                .collection::<Document>("modular_results")
                .find_one(doc! {"_id":rid,"expires_at":{"$gt":DateTime::from_millis(now()*1000)}})
                .await
                .map_err(unavailable)?;
            if let Some(result) = cached {
                return Ok(Json(
                    json!({"result":result.get_document("result").ok(),"replayed":true}),
                ));
            }
            return Err(error(
                StatusCode::CONFLICT,
                "Request already started; recover its status instead",
            ));
        }
        let maximum = row.get_i64("maximum").map_err(unavailable)? as u64;
        for tag in ["finalized", "latest"] {
            let raw = reservation(app, &g, rid, tag).await.map_err(problem)?;
            if !reservation_open(&raw, id, &wallet, maximum).map_err(problem)? {
                return Err(error(
                    StatusCode::CONFLICT,
                    "Wait for finalized live reservation",
                ));
            }
        }
        let head = app
            .stake
            .as_ref()
            .unwrap()
            .rpc("eth_getBlockByNumber", json!(["latest", false]))
            .await
            .map_err(problem)?;
        if abi::uint(head["timestamp"].as_str().unwrap_or("")).map_err(problem)? + 120
            >= row.get_i64("expiry").map_err(unavailable)? as u128
        {
            return Err(error(
                StatusCode::CONFLICT,
                "Reservation expires too soon; release it and reserve again",
            ));
        }
        let secret = crate::rpc_credentials::paid_key(&p).map_err(problem)?;
        let lease = format!("project:{id}");
        crate::trial::take_lease(app, &lease, rid).await?;
        let claimed = requests
            .update_one(
                doc! {"_id":rid,"status":"prepared"},
                doc! {"$set":{"status":"inflight","started_at":now()}},
            )
            .await
            .map_err(unavailable)?;
        if claimed.modified_count != 1 {
            crate::trial::release(app, &[lease], rid).await;
            return Err(error(StatusCode::CONFLICT, "Request already started"));
        }
        let output = crate::trial::generate(&p, &secret, &input.prompt, maximum).await;
        let answer = match output {
            Ok(result) => {
                let amount = result["usage"]["completion_tokens"].as_u64().unwrap_or(0);
                let mut session = app.db.client().start_session().await.map_err(unavailable)?;
                session.start_transaction().await.map_err(unavailable)?;
                app.db.collection::<Document>("modular_results").insert_one(doc!{"_id":rid,"result":mongodb::bson::to_document(&result).map_err(unavailable)?,"expires_at":DateTime::from_millis((now()+900)*1000)}).session(&mut session).await.map_err(unavailable)?;
                requests
                    .update_one(
                        doc! {"_id":rid,"status":"inflight"},
                        doc! {"$set":{"status":"consumed","amount":amount as i64}},
                    )
                    .session(&mut session)
                    .await
                    .map_err(unavailable)?;
                let mut committed = false;
                for _ in 0..3 {
                    match session.commit_transaction().await {
                        Ok(()) => {
                            committed = true;
                            break;
                        }
                        Err(e) if e.contains_label("UnknownTransactionCommitResult") => continue,
                        Err(e) => return Err(unavailable(e)),
                    }
                }
                if !committed {
                    return Err(error(
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Result commit uncertain; recover without generating again",
                    ));
                }
                Ok(Json(
                    json!({"result":result,"reservation_id":rid,"settlement":"queued"}),
                ))
            }
            Err(_) => {
                requests
                    .update_one(
                        doc! {"_id":rid,"status":"inflight"},
                        doc! {"$set":{"status":"release-pending","amount":0_i64}},
                    )
                    .await
                    .map_err(unavailable)?;
                Err(error(
                    StatusCode::BAD_GATEWAY,
                    "Provider failed; reservation release queued, no credits consumed",
                ))
            }
        };
        crate::trial::release(app, &[lease], rid).await;
        return answer;
    }
    Err(error(StatusCode::NOT_FOUND, "Unknown modular action"))
}

/// Called only after the existing owner filter has selected the project.
pub(crate) async fn collateral(
    app: &App,
    p: &Document,
    action: &str,
    method: &str,
    body: &[u8],
) -> ApiResult {
    let g = group(app, p).await.map_err(problem)?;
    let id = p.get_str("_id").map_err(unavailable)?;
    let wallet = p.get_str("wallet_address").map_err(unavailable)?;
    let key = stake::project_key(id);
    let f = call(
        app,
        &g,
        "stake_vault",
        "collateralState(bytes32)",
        &[key[2..].into()],
        "finalized",
    )
    .await
    .map_err(problem)?;
    let l = call(
        app,
        &g,
        "stake_vault",
        "collateralState(bytes32)",
        &[key[2..].into()],
        "latest",
    )
    .await
    .map_err(problem)?;
    let free = abi::uint(abi::w(&f, 2).map_err(problem)?)
        .map_err(problem)?
        .min(abi::uint(abi::w(&l, 2).map_err(problem)?).map_err(problem)?);
    if method == "POST" && action == "/withdraw" {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            amount_raw: String,
        }
        let input: Input = serde_json::from_slice(body)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide amount_raw"))?;
        if input.amount_raw.is_empty() || !input.amount_raw.bytes().all(|c| c.is_ascii_digit()) {
            return Err(error(StatusCode::BAD_REQUEST, "Invalid amount"));
        }
        let amount = input
            .amount_raw
            .parse::<u128>()
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Invalid amount"))?;
        if amount == 0 || amount > free {
            return Err(error(
                StatusCode::CONFLICT,
                "Amount exceeds finalized free stake",
            ));
        }
        return Ok(Json(
            json!({"transaction":tx(&g,wallet,"stake_vault",abi::data("withdraw(bytes32,uint256)",&[key[2..].into(),abi::word(amount)]),0)}),
        ));
    }
    if method == "POST" && action == "/submit" {
        let input: Value = serde_json::from_slice(body)
            .map_err(|_| error(StatusCode::BAD_REQUEST, "Provide tx_hash"))?;
        let hash = input["tx_hash"]
            .as_str()
            .filter(|v| valid_hash(v))
            .ok_or_else(|| error(StatusCode::BAD_REQUEST, "Invalid hash"))?;
        let raw = app
            .stake
            .as_ref()
            .unwrap()
            .rpc("eth_getTransactionByHash", json!([hash]))
            .await
            .map_err(problem)?;
        let data = raw["input"].as_str().unwrap_or("");
        if raw["from"].as_str().map(str::to_lowercase).as_deref() != Some(wallet)
            || raw["to"].as_str().map(str::to_lowercase).as_deref()
                != Some(g.contracts["stake_vault"].as_str())
            || data.len() != 138
            || !data.starts_with(&format!(
                "0x{}{}",
                abi::sel("withdraw(bytes32,uint256)"),
                &key[2..]
            ))
            || raw["value"] != json!("0x0")
            || raw["chainId"] != json!(format!("0x{:x}", g.chain_id))
        {
            return Err(error(
                StatusCode::CONFLICT,
                "Withdrawal does not match provider/project/chain",
            ));
        }
        app.db.collection::<Document>("modular_transactions").update_one(doc!{"_id":format!("{}:{}",g.id,hash.to_lowercase())},doc!{"$setOnInsert":{"group":&g.id,"project_id":id,"wallet":wallet,"kind":"prl-withdraw","tx_hash":hash.to_lowercase(),"at":now()}}).upsert(true).await.map_err(unavailable)?;
        return Ok(Json(json!({"tracked":true})));
    }
    let delivered = call(
        app,
        &g,
        "usage_escrow",
        "providerDelivered(bytes32)",
        &[key[2..].into()],
        "finalized",
    )
    .await
    .map_err(problem)?;
    let pending_delivered = call(
        app,
        &g,
        "usage_escrow",
        "providerDelivered(bytes32)",
        &[key[2..].into()],
        "latest",
    )
    .await
    .map_err(problem)?;
    let terms = call(
        app,
        &g,
        "registry",
        "project(bytes32)",
        &[key[2..].into()],
        "finalized",
    )
    .await
    .map_err(problem)?;
    let view = |raw: &str, d: &str| -> Result<Value, (StatusCode, Json<Value>)> {
        Ok(
            json!({"initial":abi::uint(abi::w(raw,0).map_err(problem)?).map_err(problem)?.to_string(),"locked":abi::uint(abi::w(raw,1).map_err(problem)?).map_err(problem)?.to_string(),"free":abi::uint(abi::w(raw,2).map_err(problem)?).map_err(problem)?.to_string(),"returned":abi::uint(abi::w(raw,3).map_err(problem)?).map_err(problem)?.to_string(),"delivered_credits":abi::uint(d).map_err(problem)?.to_string(),"committed_credits":abi::uint(abi::w(&terms,2).map_err(problem)?).map_err(problem)?.to_string()}),
        )
    };
    let final_values = view(&f, &delivered)?;
    let latest_values = view(&l, &pending_delivered)?;
    let withdrawals = track_rows(app, &g, id, wallet)
        .await
        .map_err(problem)?
        .into_iter()
        .filter(|r| r["kind"] == json!("prl-withdraw"))
        .map(|r| json!({"tx_hash":r["tx_hash"],"status":r["progress"]["status"]}))
        .collect::<Vec<_>>();
    let final_returned = abi::uint(abi::w(&f, 3).map_err(problem)?).map_err(problem)?;
    let latest_returned = abi::uint(abi::w(&l, 3).map_err(problem)?).map_err(problem)?;
    let unlock = abi::uint(abi::w(&l, 2).map_err(problem)?)
        .map_err(problem)?
        .saturating_add(latest_returned)
        .saturating_sub(
            abi::uint(abi::w(&f, 2).map_err(problem)?)
                .map_err(problem)?
                .saturating_add(final_returned),
        );
    Ok(Json(
        json!({"progressive":true,"protocol":"modular","chain_id":g.chain_id,"staking_contract":g.contracts["stake_vault"],"wallet":wallet,"finalized":final_values,"latest":latest_values,"withdrawable_raw":free.to_string(),"unlock_pending_raw":unlock.to_string(),"withdrawal_pending_raw":latest_returned.saturating_sub(final_returned).to_string(),"chain_halted":false,"withdrawals":withdrawals}),
    ))
}
