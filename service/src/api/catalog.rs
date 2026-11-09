//! Separate review candidates from neutral watches and operational diagnostics.
use super::*;
use chrono::{DateTime, Utc};

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(super) struct IncidentFilter {
    chain_id: Option<i64>,
    severity: Option<String>,
    category: Option<String>,
    limit: Option<i64>,
    before: Option<String>,
    id: Option<Uuid>,
}

pub(super) async fn incidents(
    State(s): State<Arc<Services>>,
    Query(f): Query<IncidentFilter>,
) -> Result<Json<Value>, StatusCode> {
    let category = f.category.as_deref().unwrap_or("active");
    let limit = f.limit.unwrap_or(100);
    let before = f.before.as_deref().map(|value| DateTime::parse_from_rfc3339(value))
        .transpose().map_err(|_| StatusCode::BAD_REQUEST)?.map(|value| value.with_timezone(&Utc));
    if !matches!(category, "active" | "review" | "watch" | "diagnostics" | "retracted" | "archived" | "demo" | "all")
        || !(1..=256).contains(&limit) || f.chain_id.is_some_and(|id| id <= 0)
        || before.is_some() != f.id.is_some()
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    // Filter and paginate before LIMIT so diagnostics cannot push review candidates out.
    let rows: Vec<(Value, DateTime<Utc>, Uuid)> = sqlx::query_as(
        "SELECT data || jsonb_build_object('category',category),updated_at,id
         FROM incident_catalog
         WHERE ($1::bigint IS NULL OR chain_id=$1)
          AND ($2::text IS NULL OR data->>'severity'=$2)
          AND ($3='all' OR category=$3 OR ($3='active' AND category IN ('review','watch')))
          AND ($4::timestamptz IS NULL OR (updated_at,id)<($4::timestamptz,$5::uuid))
         ORDER BY updated_at DESC,id DESC LIMIT $6")
        .bind(f.chain_id).bind(f.severity).bind(category).bind(before).bind(f.id).bind(limit)
        .fetch_all(&s.store.pool).await.map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let scanned = rows.len();
    let next = if scanned == limit as usize {
        rows.last().map(|(_, updated_at, id)| json!({"before":updated_at,"id":id}))
    } else { None };
    let incidents: Vec<Value> = rows.into_iter().map(|(incident, _, _)| incident).collect();
    Ok(Json(json!({"incidents":incidents,"category":category,"limit":limit,
        "scanned":scanned,"page_full":scanned == limit as usize,"next":next})))
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
