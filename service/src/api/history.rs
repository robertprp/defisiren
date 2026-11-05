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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, services::ServiceFactory};

    #[tokio::test]
    #[ignore = "requires an isolated PostgreSQL DATABASE_URL"]
    async fn rest_replay_and_revision_history() {
        let mut cfg: Config = toml::from_str(include_str!("../../../config/offline.toml")).unwrap();
        let directory = std::env::temp_dir().join(format!("siren-rest-{}", Uuid::new_v4()));
        cfg.data_dir = directory.to_str().unwrap().into();
        let services = ServiceFactory::default().initialize(cfg).await.unwrap();
        let key = services
            .store
            .new_api_key("REST integration")
            .await
            .unwrap();
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO incidents(id,chain_id,revision,data) VALUES($1,1,3,'{\"severity\":\"high\",\"finding_type\":\"unusual_outflow\"}')")
            .bind(id)
            .execute(&services.store.pool)
            .await
            .unwrap();
        for revision in 1..=3i64 {
            let event = Uuid::new_v4();
            let data = json!({"incident_id": id, "event_id": event, "revision": revision,
                "cursor": revision, "chain_id": 1, "severity": "high", "retracted": revision == 3});
            sqlx::query("INSERT INTO finding_revisions(event_id,incident_id,revision,chain_id,data) VALUES($1,$2,$3,1,$4)")
                .bind(event).bind(id).bind(revision).bind(&data).execute(&services.store.pool).await.unwrap();
            sqlx::query("INSERT INTO delivery_outbox(stream,cursor,event_id,data) VALUES('findings',$1,$2,$3)")
                .bind(revision).bind(event).bind(&data).execute(&services.store.pool).await.unwrap();
        }
        sqlx::query("UPDATE stream_heads SET cursor=3 WHERE stream='findings'")
            .execute(&services.store.pool)
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let app = super::super::router(services.clone());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::new();
        let get = |path: String| client.get(format!("{base}{path}")).bearer_auth(&key);
        assert_eq!(
            client
                .get(format!("{base}/v1/findings"))
                .send()
                .await
                .unwrap()
                .status(),
            401
        );
        let catalog: Value = get("/v1/incidents?category=review".into())
            .send().await.unwrap().json().await.unwrap();
        assert_eq!(catalog["incidents"].as_array().unwrap().len(), 1);
        assert_eq!(catalog["incidents"][0]["category"], "review");
        let watches: Value = get("/v1/incidents?category=watch".into())
            .send().await.unwrap().json().await.unwrap();
        assert_eq!(watches["incidents"], json!([]));
        assert_eq!(get("/v1/incidents?category=invalid".into()).send().await.unwrap().status(), 400);
        let page: Value = get("/v1/findings?limit=2".into())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(page["findings"].as_array().unwrap().len(), 2);
        assert_eq!(page["next_after"], 2);
        assert_eq!(page["page_full"], true);
        let tail: Value = get("/v1/findings?after=2&limit=2".into())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(tail["findings"][0]["retracted"], true);
        assert_eq!(tail["next_after"], 3);
        assert_eq!(tail["page_full"], false);
        for filter in ["chain_id=2", "severity=low"] {
            let page: Value = get(format!("/v1/findings?limit=2&{filter}"))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            assert_eq!(page["findings"], json!([]));
            assert_eq!(page["next_after"], 2);
            assert_eq!(page["scanned"], 2);
        }
        let empty: Value = get("/v1/reviewed".into())
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(empty["findings"], json!([]));
        assert_eq!(empty["next_after"], 0);
        for query in [
            "after=-1",
            "limit=0",
            "limit=257",
            "chain_id=0",
            "chain_id=18446744073709551615",
            "unknown=1",
        ] {
            assert_eq!(
                get(format!("/v1/findings?{query}"))
                    .send()
                    .await
                    .unwrap()
                    .status(),
                400
            );
        }
        let history: Value = get(format!("/v1/incidents/{id}/revisions?after=1&limit=2"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(history["revisions"][0]["revision"], 2);
        assert_eq!(history["revisions"][1]["retracted"], true);
        assert_eq!(history["next_after"], 3);
        let empty: Value = get(format!("/v1/incidents/{id}/revisions?after=3"))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(empty["revisions"], json!([]));
        assert_eq!(empty["next_after"], 3);
        assert_eq!(
            get(format!("/v1/incidents/{}/revisions", Uuid::new_v4()))
                .send()
                .await
                .unwrap()
                .status(),
            404
        );
        assert_eq!(
            get(format!("/v1/incidents/{id}/revisions?after=-1"))
                .send()
                .await
                .unwrap()
                .status(),
            400
        );
        sqlx::query("UPDATE stream_heads SET floor=2 WHERE stream='findings'")
            .execute(&services.store.pool)
            .await
            .unwrap();
        assert_eq!(
            get("/v1/findings?after=1".into())
                .send()
                .await
                .unwrap()
                .status(),
            410
        );
        assert_eq!(
            get("/v1/findings?after=2".into())
                .send()
                .await
                .unwrap()
                .status(),
            200
        );
        server.abort();
        let _ = server.await;
        drop(services);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
