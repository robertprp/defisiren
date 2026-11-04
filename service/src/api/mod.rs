mod history;
use crate::services::Services;
use alloy_primitives::Address;
use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, Query, Request, State, WebSocketUpgrade,
    },
    http::{header, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use siren_lib::Error;
use sqlx::Row;
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use uuid::Uuid;

pub fn router(s: Arc<Services>) -> Router {
    let protected = Router::new()
        .route("/v1/incidents", get(incidents))
        .route("/v1/incidents/{id}", get(incident))
        .route("/v1/incidents/{id}/revisions", get(history::revisions))
        .route("/v1/findings", get(history::findings))
        .route("/v1/reviewed", get(history::reviewed))
        .route("/v1/addresses/{chain_id}/{address}", get(address))
        .route("/v1/chains", get(chains))
        .route("/ws/findings", get(ws_findings))
        .route("/ws/reviewed", get(ws_reviewed))
        .route_layer(middleware::from_fn_with_state(s.clone(), auth));
    Router::new()
        .merge(protected)
        .route("/health/live", get(|| async { Json(json!({"live":true})) }))
        .route("/health/ready", get(ready))
        .route("/metrics", get(metrics))
        .with_state(s)
}
async fn auth(
    State(s): State<Arc<Services>>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let key = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .ok_or(StatusCode::UNAUTHORIZED)?;
    match s
        .store
        .authenticate(key, s.config.api_requests_per_minute)
        .await
    {
        Ok(true) => Ok(next.run(request).await),
        Ok(false) => Err(StatusCode::UNAUTHORIZED),
        Err(Error::Limit(_)) => Err(StatusCode::TOO_MANY_REQUESTS),
        Err(_) => Err(StatusCode::SERVICE_UNAVAILABLE),
    }
}
#[derive(Deserialize, Default)]
struct Filter {
    #[serde(default)]
    after: i64,
    chain_id: Option<u64>,
    severity: Option<String>,
    limit: Option<i64>,
}
async fn incidents(
    State(s): State<Arc<Services>>,
    Query(f): Query<Filter>,
) -> Result<Json<Value>, StatusCode> {
    let rows:Vec<Value>=sqlx::query_scalar("SELECT data FROM incidents WHERE ($1::bigint IS NULL OR chain_id=$1) ORDER BY updated_at DESC,id LIMIT $2")
        .bind(f.chain_id.map(|x|x as i64)).bind(f.limit.unwrap_or(100).clamp(1,256)).fetch_all(&s.store.pool).await.map_err(|_|StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(json!({"incidents":rows})))
}
async fn incident(
    State(s): State<Arc<Services>>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, StatusCode> {
    let value: Option<Value> = sqlx::query_scalar("SELECT data FROM incidents WHERE id=$1")
        .bind(id)
        .fetch_optional(&s.store.pool)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    value.map(Json).ok_or(StatusCode::NOT_FOUND)
}
async fn address(
    State(s): State<Arc<Services>>,
    Path((chain, address)): Path<(u64, String)>,
) -> Result<Json<Value>, StatusCode> {
    let a: Address = address.parse().map_err(|_| StatusCode::BAD_REQUEST)?;
    let value: Option<Value> =
        sqlx::query_scalar("SELECT data FROM address_profiles WHERE chain_id=$1 AND address=$2")
            .bind(chain as i64)
            .bind(a.as_slice())
            .fetch_optional(&s.store.pool)
            .await
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    value.map(Json).ok_or(StatusCode::NOT_FOUND)
}
async fn chains(State(s): State<Arc<Services>>) -> Result<Json<Value>, StatusCode> {
    let values: Vec<Value> = sqlx::query_scalar("SELECT data FROM capabilities ORDER BY chain_id")
        .fetch_all(&s.store.pool)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(json!({"chains":values})))
}
async fn ready(State(s): State<Arc<Services>>) -> Response {
    let db = sqlx::query("SELECT 1").execute(&s.store.pool).await.is_ok();
    let rows = sqlx::query(
        "SELECT chain_id,updated_at FROM capabilities WHERE updated_at>now()-interval '2 minutes'",
    )
    .fetch_all(&s.store.pool)
    .await;
    let active = rows
        .map(|r| {
            r.into_iter()
                .map(|v| v.get::<i64, _>("chain_id") as u64)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let missing = s
        .config
        .chains
        .iter()
        .filter(|c| !active.contains(&c.id))
        .map(|c| c.id)
        .collect::<Vec<_>>();
    let ready = db && s.healthy.load(Ordering::SeqCst) && missing.is_empty();
    (if ready{StatusCode::OK}else{StatusCode::SERVICE_UNAVAILABLE},Json(json!({"ready":ready,"database":db,"chains_without_recent_probe":missing,"coverage":"see /v1/chains"}))).into_response()
}
async fn metrics(State(s): State<Arc<Services>>) -> Response {
    (
        [(header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        s.metrics.render(),
    )
        .into_response()
}
async fn ws_findings(
    State(s): State<Arc<Services>>,
    Query(f): Query<Filter>,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    upgrade(s, f, ws, "findings").await
}
async fn ws_reviewed(
    State(s): State<Arc<Services>>,
    Query(f): Query<Filter>,
    ws: WebSocketUpgrade,
) -> Result<Response, StatusCode> {
    upgrade(s, f, ws, "reviewed").await
}
async fn upgrade(
    s: Arc<Services>,
    f: Filter,
    ws: WebSocketUpgrade,
    stream: &'static str,
) -> Result<Response, StatusCode> {
    match s.store.stream(stream, f.after, 1).await {
        Ok(_) => {}
        Err(Error::Invalid(_)) => return Err(StatusCode::GONE),
        Err(_) => return Err(StatusCode::SERVICE_UNAVAILABLE),
    }
    Ok(ws
        .max_message_size(4096)
        .max_frame_size(4096)
        .on_upgrade(move |socket| delivery(socket, s, f, stream)))
}
async fn send(socket: &mut WebSocket, message: Message) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(5), socket.send(message)).await,
        Ok(Ok(()))
    )
}
async fn delivery(mut socket: WebSocket, s: Arc<Services>, f: Filter, stream: &str) {
    let mut cursor = f.after;
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    let mut heartbeat = tokio::time::interval(Duration::from_secs(15));
    let maximum_lifetime = tokio::time::sleep(Duration::from_secs(900));
    tokio::pin!(maximum_lifetime);
    loop {
        tokio::select! {
            _=&mut maximum_lifetime=>break,
            message=socket.next()=>match message {Some(Ok(Message::Ping(p)))=>{if !send(&mut socket,Message::Pong(p)).await{break;}},Some(Ok(Message::Close(_)))|None|Some(Err(_))=>break,_=>{}},
            _=heartbeat.tick()=>{if !send(&mut socket,Message::Text(json!({"type":"heartbeat","stream":stream,"cursor":cursor}).to_string().into())).await{break;}},
            _=tick.tick()=>{
                let rows=match s.store.stream(stream,cursor,64).await {Ok(r)=>r,Err(_)=>break};
                for row in rows {
                    let Some(next)=row["cursor"].as_i64() else{return;};
                    if f.chain_id.is_some_and(|id|row["chain_id"].as_u64()!=Some(id)) || f.severity.as_ref().is_some_and(|v|row["severity"].as_str()!=Some(v.as_str())) {cursor=next;continue;}
                    if !send(&mut socket,Message::Text(row.to_string().into())).await{return;}
                    cursor=next;
                }
            }
        }
    }
    let _ = send(&mut socket, Message::Close(None)).await;
}
