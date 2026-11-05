//! Separate review candidates from neutral watches and operational diagnostics.
use super::*;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(super) struct IncidentFilter {
    chain_id: Option<i64>,
    severity: Option<String>,
    category: Option<String>,
    limit: Option<i64>,
}

pub(super) async fn incidents(
    State(s): State<Arc<Services>>,
    Query(f): Query<IncidentFilter>,
) -> Result<Json<Value>, StatusCode> {
    let category = f.category.as_deref().unwrap_or("active");
    let limit = f.limit.unwrap_or(100);
    if !matches!(category, "active" | "review" | "watch" | "diagnostics" | "retracted" | "archived" | "demo" | "all")
        || !(1..=256).contains(&limit) || f.chain_id.is_some_and(|id| id <= 0)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Filter before LIMIT so diagnostics cannot push review candidates out of the window.
    let rows: Vec<Value> = sqlx::query_scalar(
        "SELECT data || jsonb_build_object('category',category) FROM incident_catalog
         WHERE ($1::bigint IS NULL OR chain_id=$1)
          AND ($2::text IS NULL OR data->>'severity'=$2)
          AND ($3='all' OR category=$3 OR ($3='active' AND category IN ('review','watch')))
         ORDER BY updated_at DESC,id LIMIT $4")
        .bind(f.chain_id).bind(f.severity).bind(category).bind(limit)
        .fetch_all(&s.store.pool).await.map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(Json(json!({"incidents":rows,"category":category,"limit":limit})))
}

pub(super) async fn summary(State(s): State<Arc<Services>>) -> Result<Json<Value>, StatusCode> {
    let rows = sqlx::query("SELECT category,count(*) AS count FROM incident_catalog GROUP BY category")
        .fetch_all(&s.store.pool).await.map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let mut counts = serde_json::Map::new();
    for category in ["review", "watch", "diagnostics", "retracted", "archived", "demo"] {
        counts.insert(category.into(), json!(0));
    }
    for row in rows {
        counts.insert(row.get::<String,_>("category"), json!(row.get::<i64,_>("count")));
    }
    Ok(Json(json!({"counts":counts,"scope":"all_stored_incidents"})))
}
