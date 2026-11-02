//! Bounded REST replay and append-only incident revision inspection.
use super::*;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReplayQuery {
    #[serde(default)]
    after: i64,
    #[serde(default = "default_limit")]
    limit: i64,
    chain_id: Option<i64>,
    severity: Option<String>,
}

fn default_limit() -> i64 {
    100
}

impl ReplayQuery {
    fn validate(&self) -> Result<(), StatusCode> {
        if self.after < 0
            || !(1..=256).contains(&self.limit)
            || self.chain_id.is_some_and(|id| id <= 0)
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        Ok(())
    }
}

pub(super) async fn findings(
    State(s): State<Arc<Services>>,
    Query(q): Query<ReplayQuery>,
) -> Result<Json<Value>, StatusCode> {
    replay(&s, q, "findings").await
}

pub(super) async fn reviewed(
    State(s): State<Arc<Services>>,
    Query(q): Query<ReplayQuery>,
) -> Result<Json<Value>, StatusCode> {
    replay(&s, q, "reviewed").await
}

async fn replay(s: &Services, q: ReplayQuery, stream: &str) -> Result<Json<Value>, StatusCode> {
    q.validate()?;
    let rows = s
        .store
        .stream(stream, q.after, q.limit)
        .await
        .map_err(|e| match e {
            Error::Invalid(_) => StatusCode::GONE,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        })?;
    // Advance over filtered records too, so a sparse filter cannot stall polling.
    let next_after = rows
        .last()
        .and_then(|r| r["cursor"].as_i64())
        .unwrap_or(q.after);
    let scanned = rows.len();
    let findings: Vec<_> = rows
        .into_iter()
        .filter(|r| {
            q.chain_id
                .is_none_or(|id| r["chain_id"].as_i64() == Some(id))
                && q.severity
                    .as_ref()
                    .is_none_or(|v| r["severity"].as_str() == Some(v.as_str()))
        })
        .collect();
    Ok(Json(
        json!({"stream": stream, "findings": findings, "next_after": next_after,
        "scanned": scanned, "page_full": scanned == q.limit as usize}),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RevisionQuery {
    #[serde(default)]
    after: i64,
    #[serde(default = "default_limit")]
    limit: i64,
}

pub(super) async fn revisions(
    State(s): State<Arc<Services>>,
    Path(id): Path<Uuid>,
    Query(q): Query<RevisionQuery>,
) -> Result<Json<Value>, StatusCode> {
    if q.after < 0 || !(1..=256).contains(&q.limit) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM incidents WHERE id=$1)")
        .bind(id)
        .fetch_one(&s.store.pool)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT data FROM finding_revisions WHERE incident_id=$1 AND revision>$2 ORDER BY revision LIMIT $3")
        .bind(id).bind(q.after).bind(q.limit).fetch_all(&s.store.pool).await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let next_after = rows
        .last()
        .and_then(|r| r["revision"].as_i64())
        .unwrap_or(q.after);
    Ok(Json(json!({"incident_id": id, "next_after": next_after,
        "page_full": rows.len() == q.limit as usize, "revisions": rows})))
}

