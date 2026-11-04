pub mod bridge;
pub mod evidence;
pub mod lifecycle;
use crate::{
    config::Config,
    contracts::Pool,
    detection::{self, Engine},
    events::Accepted,
};
use alloy_primitives::{Address, B256};
use chrono::Utc;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use siren_lib::{entity::*, AppResult, Error};
use sqlx::{postgres::PgPoolOptions, PgPool, Postgres, Row, Transaction};
use std::sync::Arc;
use uuid::Uuid;

pub struct Store {
    pub pool: PgPool,
}

fn db(_: sqlx::Error) -> Error {
    Error::Database
}

impl Store {
    pub async fn connect(url: &str) -> AppResult<Arc<Self>> {
        let pool = PgPoolOptions::new()
            .max_connections(16)
            .acquire_timeout(std::time::Duration::from_secs(5))
            .connect(url)
            .await
            .map_err(db)?;
        sqlx::migrate!("../migrations")
            .run(&pool)
            .await
            .map_err(|_| Error::Database)?;
        Ok(Arc::new(Self { pool }))
    }

    pub async fn cursor(&self, chain: u64) -> AppResult<Option<(u64, B256)>> {
        let row =
            sqlx::query("SELECT block_number,block_hash FROM chain_cursors WHERE chain_id=$1")
                .bind(chain as i64)
                .fetch_optional(&self.pool)
                .await
                .map_err(db)?;
        row.map(|r| {
            let h: Vec<u8> = r.get("block_hash");
            if h.len() != 32 {
                return Err(Error::Database);
            }
            Ok((r.get::<i64, _>("block_number") as u64, B256::from_slice(&h)))
        })
        .transpose()
    }

    pub async fn accept(
        &self,
        event: &Accepted,
        cfg: &Config,
        pools: &[Pool],
    ) -> AppResult<Vec<Finding>> {
        let chain = match event {
            Accepted::Block(b) => b.chain_id,
            Accepted::Observation(o) => o.chain_id,
            Accepted::Feed(f) => f.chain_id,
            Accepted::Progress(p) => p.chain_id,
        };
        let mut tx = self.pool.begin().await.map_err(db)?;
        sqlx::query("SET LOCAL synchronous_commit=on")
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let journal_id = Sha256::digest(serde_json::to_vec(event)?).to_vec();
        let inserted =
            sqlx::query("INSERT INTO applied_journal(id) VALUES($1) ON CONFLICT DO NOTHING")
                .bind(journal_id)
                .execute(&mut *tx)
                .await
                .map_err(db)?
                .rows_affected();
        if inserted == 0 {
            tx.rollback().await.map_err(db)?;
            return Ok(vec![]);
        }
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(chain as i64)
            .execute(&mut *tx)
            .await
            .map_err(db)?;
        let cursor = sqlx::query(
            "SELECT block_number,block_hash,engine FROM chain_cursors WHERE chain_id=$1 FOR UPDATE",
        )
        .bind(chain as i64)
        .fetch_optional(&mut *tx)
        .await
        .map_err(db)?;
        let mut engine: Engine = cursor
            .as_ref()
            .map(|r| serde_json::from_value(r.get("engine")))
            .transpose()?
            .unwrap_or_default();
        let manual = sqlx::query("SELECT address,expires_at FROM watchlist WHERE chain_id=$1 AND expires_at>now() LIMIT $2")
            .bind(chain as i64).bind(cfg.funding.max_profiles as i64).fetch_all(&mut *tx).await.map_err(db)?;
        for row in manual {
            let bytes: Vec<u8> = row.get("address");
            if bytes.len() != 20 {
                return Err(Error::Database);
            }
            let address = Address::from_slice(&bytes);
            let expiry: chrono::DateTime<chrono::Utc> = row.get("expires_at");
            engine.watch.derived_only.remove(&address);
            engine
                .watch
                .profiles
                .entry(address)
                .or_insert_with(|| Profile {
                    chain_id: chain,
                    address,
                    first_observed: Utc::now(),
                    last_activity: Utc::now(),
                    history_start: None,
                    history_complete: false,
                    nonce: None,
                    nonce_state: None,
                    code_status: "unknown".into(),
                    freshness_confidence: 0,
                    watch_until: expiry,
                    funding: vec![],
                    deployments: vec![],
                });
        }
        let mut published = vec![];
        match event {
            Accepted::Progress(progress) => {
                published.extend(lifecycle::progress(&mut tx, progress).await?);
            }
            Accepted::Feed(feed) => {
                let hash = alloy_primitives::keccak256(serde_json::to_vec(&feed.payload)?);
                sqlx::query("INSERT INTO raw_feed_messages(chain_id,source,sequence,payload_hash,data,observed_at) VALUES($1,$2,$3::numeric,$4,$5,$6) ON CONFLICT DO NOTHING")
                    .bind(feed.chain_id as i64).bind(&feed.source).bind(feed.sequence.map(|n|n.to_string())).bind(hash.as_slice()).bind(&feed.payload).bind(feed.observed_at)
                    .execute(&mut *tx).await.map_err(db)?;
            }
            Accepted::Block(b) => {
                let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM canonical_blocks WHERE chain_id=$1 AND hash=$2 AND canonical)")
                    .bind(chain as i64).bind(b.hash.as_slice()).fetch_one(&mut *tx).await.map_err(db)?;
                if exists {
                    tx.commit().await.map_err(db)?;
                    return Ok(vec![]);
                }
                if let Some(c) = &cursor {
                    let previous: Vec<u8> = c.get("block_hash");
                    let height = c.get::<i64, _>("block_number") as u64;
                    if previous.as_slice() != b.parent_hash.as_slice() || b.number != height + 1 {
                        let parent = sqlx::query("SELECT number,engine FROM canonical_blocks WHERE chain_id=$1 AND hash=$2 AND canonical")
                            .bind(chain as i64).bind(b.parent_hash.as_slice()).fetch_optional(&mut *tx).await.map_err(db)?
                            .ok_or_else(|| Error::Dependency("reorg ancestor not in canonical window; bounded backfill required".into()))?;
                        let ancestor = parent.get::<i64, _>("number") as u64;
                        if b.number != ancestor + 1
                            || height.saturating_sub(ancestor) > cfg.max_reorg_depth
                        {
                            return Err(Error::Limit(
                                "deep reorg requires operator safe-checkpoint recovery".into(),
                            ));
                        }
                        engine = serde_json::from_value(parent.get("engine"))?;
                        sqlx::query("DELETE FROM finality_points WHERE chain_id=$1 AND number>$2")
                            .bind(chain as i64)
                            .bind(ancestor as i64)
                            .execute(&mut *tx)
                            .await
                            .map_err(db)?;
                        published
                            .extend(lifecycle::bridge_retractions(&mut tx, chain, ancestor).await?);
                        let rows = sqlx::query("SELECT i.data FROM incidents i WHERE i.chain_id=$1 AND (i.data->'position'->>'block_number')::bigint > $2 AND COALESCE((i.data->>'retracted')::boolean,false)=false")
                            .bind(chain as i64).bind(ancestor as i64).fetch_all(&mut *tx).await.map_err(db)?;
                        for row in rows {
                            let mut f: Finding = serde_json::from_value(row.get("data"))?;
                            f.retracted = true;
                            f.finality = Stage::Reorged;
                            f.observed_at = Utc::now();
                            f.finding_type = "retraction".into();
                            published.push(publish(&mut tx, f).await?);
                        }
                        sqlx::query("UPDATE observations SET canonical=false WHERE chain_id=$1 AND block_hash IN (SELECT hash FROM canonical_blocks WHERE chain_id=$1 AND number>$2 AND canonical)")
                            .bind(chain as i64).bind(ancestor as i64).execute(&mut *tx).await.map_err(db)?;
                        sqlx::query("UPDATE funding_edges SET canonical=false WHERE chain_id=$1 AND block_hash IN (SELECT hash FROM canonical_blocks WHERE chain_id=$1 AND number>$2 AND canonical)")
                            .bind(chain as i64).bind(ancestor as i64).execute(&mut *tx).await.map_err(db)?;
                        sqlx::query("DELETE FROM contracts WHERE chain_id=$1 AND state_hash IN (SELECT hash FROM canonical_blocks WHERE chain_id=$1 AND number>$2 AND canonical)")
                            .bind(chain as i64).bind(ancestor as i64).execute(&mut *tx).await.map_err(db)?;
                        sqlx::query("UPDATE canonical_blocks SET canonical=false WHERE chain_id=$1 AND number>$2")
                            .bind(chain as i64).bind(ancestor as i64).execute(&mut *tx).await.map_err(db)?;
                    }
                }
                let observation_time = b
                    .observations
                    .iter()
                    .map(|o| o.observed_at)
                    .max()
                    .unwrap_or(
                        chrono::DateTime::from_timestamp(
                            i64::try_from(b.timestamp)
                                .map_err(|_| Error::Invalid("block timestamp".into()))?,
                            0,
                        )
                        .ok_or_else(|| Error::Invalid("block timestamp".into()))?,
                    );
                bridge::refresh_watches(&mut tx, chain, &mut engine, cfg, observation_time).await?;
                published.extend(lifecycle::preconfirmation_corrections(&mut tx, b).await?);
                for (index, o) in b.observations.iter().enumerate() {
                    if o.chain_id != chain
                        || o.position.block_hash != Some(b.hash)
                        || o.position.transaction_index != Some(index as u64)
                        || !o.stage.canonical()
                        || o.position.block_number != Some(b.number)
                    {
                        return Err(Error::Invalid("block transaction position".into()));
                    }
                    let previous_pending = sqlx::query("SELECT data FROM observations WHERE chain_id=$1 AND sender=$2 AND nonce=$3::numeric AND stage IN ('pending','preconfirmed')")
                        .bind(chain as i64).bind(o.from.as_slice()).bind(o.nonce.to_string()).fetch_all(&mut *tx).await.map_err(db)?;
                    for old in previous_pending {
                        let previous: Observation = serde_json::from_value(old.get("data"))?;
                        let stage = if previous.hash == o.hash {
                            Stage::Included
                        } else if previous.stage == Stage::Preconfirmed {
                            Stage::Reorged
                        } else {
                            Stage::Replaced
                        };
                        let existing = sqlx::query("SELECT data FROM incidents WHERE chain_id=$1 AND data->>'finality' IN ('pending','preconfirmed','expired') AND data->'evidence' @> $2::jsonb")
                            .bind(chain as i64).bind(json!([{"transaction":previous.hash}])).fetch_all(&mut *tx).await.map_err(db)?;
                        for row in existing {
                            let mut f: Finding = serde_json::from_value(row.get("data"))?;
                            f.finality = stage;
                            f.position = o.position.clone();
                            f.ordering = o.ordering;
                            f.retracted = matches!(stage, Stage::Replaced | Stage::Reorged);
                            f.evidence.push(Evidence {
                                id: format!("included-nonce:{}", o.hash),
                                kind: "nonce_resolution".into(),
                                description:
                                    "Canonical inclusion resolves observed same-nonce alternatives."
                                        .into(),
                                transaction: Some(o.hash),
                                data: json!({"previous_hash":previous.hash}),
                            });
                            published.push(publish(&mut tx, f).await?);
                        }
                    }
                    insert_observation(&mut tx, o).await?;
                    published.extend(bridge::observe(&mut tx, o).await?);
                    if crate::funding::bridge::destination_id(o)?.is_some() {
                        bridge::refresh_watches(&mut tx, chain, &mut engine, cfg, o.observed_at)
                            .await?;
                    }
                    for f in engine.process(o, cfg, pools)? {
                        published.push(publish(&mut tx, f).await?);
                    }
                }
                let state = serde_json::to_value(&engine)?;
                sqlx::query("INSERT INTO canonical_blocks(chain_id,number,hash,parent_hash,engine) VALUES($1,$2,$3,$4,$5) ON CONFLICT(chain_id,hash) DO UPDATE SET canonical=true,engine=EXCLUDED.engine")
                    .bind(chain as i64).bind(b.number as i64).bind(b.hash.as_slice()).bind(b.parent_hash.as_slice()).bind(&state).execute(&mut *tx).await.map_err(db)?;
                sqlx::query("INSERT INTO chain_cursors(chain_id,block_number,block_hash,engine) VALUES($1,$2,$3,$4) ON CONFLICT(chain_id) DO UPDATE SET block_number=EXCLUDED.block_number,block_hash=EXCLUDED.block_hash,engine=EXCLUDED.engine,updated_at=now()")
                    .bind(chain as i64).bind(b.number as i64).bind(b.hash.as_slice()).bind(state).execute(&mut *tx).await.map_err(db)?;
                sqlx::query("DELETE FROM address_profiles WHERE chain_id=$1")
                    .bind(chain as i64)
                    .execute(&mut *tx)
                    .await
                    .map_err(db)?;
                for p in engine.watch.profiles.values() {
                    sqlx::query(
                        "INSERT INTO address_profiles(chain_id,address,data) VALUES($1,$2,$3)",
                    )
                    .bind(chain as i64)
                    .bind(p.address.as_slice())
                    .bind(serde_json::to_value(p)?)
                    .execute(&mut *tx)
                    .await
                    .map_err(db)?;
                    for e in &p.funding {
                        sqlx::query("INSERT INTO funding_edges(id,chain_id,block_hash,recipient,data,canonical) VALUES($1,$2,$3,$4,$5,true) ON CONFLICT(id) DO UPDATE SET canonical=true")
                            .bind(&e.id).bind(chain as i64).bind(e.block_hash.map(|h| h.to_vec())).bind(e.to.as_slice())
                            .bind(serde_json::to_value(e)?).execute(&mut *tx).await.map_err(db)?;
                    }
                }
            }
            Accepted::Observation(o) => {
                let already_included:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM observations WHERE chain_id=$1 AND hash=$2 AND canonical)").bind(chain as i64).bind(o.hash.as_slice()).fetch_one(&mut *tx).await.map_err(db)?;
                if insert_observation(&mut tx, o).await?
                    && !(already_included
                        && matches!(o.stage, Stage::Pending | Stage::Preconfirmed))
                {
                    if o.stage == Stage::Pending {
                        let rows = sqlx::query("SELECT data FROM observations WHERE chain_id=$1 AND sender=$2 AND nonce=$3::numeric AND hash<>$4 AND stage='pending'")
                            .bind(chain as i64).bind(o.from.as_slice()).bind(o.nonce.to_string()).bind(o.hash.as_slice()).fetch_all(&mut *tx).await.map_err(db)?;
                        for row in rows {
                            let old: Observation = serde_json::from_value(row.get("data"))?;
                            let mut f = detection::finding(&old, "pending_alternative", vec![], 0);
                            f.evidence.push(Evidence { id: format!("alternative:{}",o.hash),kind:"same_nonce_alternative".into(),description:"Same-nonce alternatives observed; provider arrival order does not prove replacement.".into(),transaction:Some(o.hash),data:json!({"other":old.hash}) });
                            published.push(publish(&mut tx, f).await?);
                        }
                    }
                    for f in engine.process(o, cfg, pools)? {
                        published.push(publish(&mut tx, f).await?);
                    }
                }
            }
        }
        tx.commit().await.map_err(db)?;
        Ok(published)
    }

    pub async fn stream(&self, stream: &str, after: i64, limit: i64) -> AppResult<Vec<Value>> {
        let floor: i64 = sqlx::query_scalar("SELECT floor FROM stream_heads WHERE stream=$1")
            .bind(stream)
            .fetch_one(&self.pool)
            .await
            .map_err(db)?;
        if after < floor {
            return Err(Error::Invalid("retention_expired".into()));
        }
        Ok(sqlx::query_scalar("SELECT data FROM delivery_outbox WHERE stream=$1 AND cursor>$2 ORDER BY cursor LIMIT $3")
            .bind(stream).bind(after).bind(limit.clamp(1,256)).fetch_all(&self.pool).await.map_err(db)?)
    }

    pub async fn new_api_key(&self, name: &str) -> AppResult<String> {
        let key = format!("cs_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple());
        sqlx::query("INSERT INTO api_clients(id,key_hash,name,expires_at) VALUES($1,$2,$3,now()+interval '90 days')")
            .bind(Uuid::new_v4()).bind(Sha256::digest(key.as_bytes()).to_vec()).bind(name).execute(&self.pool).await.map_err(db)?;
        Ok(key)
    }

    pub async fn authenticate(&self, key: &str, max_per_minute: u32) -> AppResult<bool> {
        if key.len() > 128 {
            return Ok(false);
        }
        let client: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM api_clients WHERE key_hash=$1 AND enabled AND expires_at>now()",
        )
        .bind(Sha256::digest(key.as_bytes()).to_vec())
        .fetch_optional(&self.pool)
        .await
        .map_err(db)?;
        let Some(client) = client else {
            return Ok(false);
        };
        let count: i32 = sqlx::query_scalar("INSERT INTO api_rate_limits(client_id,minute,count) VALUES($1,$2,1) ON CONFLICT(client_id,minute) DO UPDATE SET count=api_rate_limits.count+1 RETURNING count")
            .bind(client).bind(Utc::now().timestamp()/60).fetch_one(&self.pool).await.map_err(db)?;
        if count > max_per_minute as i32 {
            return Err(Error::Limit("API rate".into()));
        }
        Ok(true)
    }

    pub async fn watch(
        &self,
        chain: u64,
        address: Address,
        hours: i64,
        reason: &str,
    ) -> AppResult<()> {
        if !(1..=8760).contains(&hours) {
            return Err(Error::Invalid("watch hours".into()));
        }
        sqlx::query("INSERT INTO watchlist(chain_id,address,expires_at,reason) VALUES($1,$2,now()+make_interval(hours => $3),$4) ON CONFLICT(chain_id,address) DO UPDATE SET expires_at=EXCLUDED.expires_at,reason=EXCLUDED.reason")
            .bind(chain as i64).bind(address.as_slice()).bind(hours as i32).bind(reason).execute(&self.pool).await.map_err(db)?;
        Ok(())
    }
}

async fn insert_observation(
    tx: &mut Transaction<'_, Postgres>,
    o: &Observation,
) -> AppResult<bool> {
    let stage = serde_json::to_value(o.stage)?
        .as_str()
        .ok_or_else(|| Error::Invalid("stage".into()))?
        .to_owned();
    let result = sqlx::query("INSERT INTO observations(chain_id,hash,source,stage,block_hash,sender,nonce,data,canonical) VALUES($1,$2,$3,$4,$5,$6,$7::numeric,$8,$9) ON CONFLICT(chain_id,hash,source,stage,block_hash) DO UPDATE SET canonical=EXCLUDED.canonical WHERE NOT observations.canonical AND EXCLUDED.canonical")
        .bind(o.chain_id as i64).bind(o.hash.as_slice()).bind(&o.source).bind(stage).bind(o.position.block_hash.unwrap_or(B256::ZERO).as_slice())
        .bind(o.from.as_slice()).bind(o.nonce.to_string()).bind(serde_json::to_value(o)?).bind(o.stage.canonical()).execute(&mut **tx).await.map_err(db)?;
    Ok(result.rows_affected() > 0)
}

pub async fn publish(tx: &mut Transaction<'_, Postgres>, mut f: Finding) -> AppResult<Finding> {
    sqlx::query("SET LOCAL synchronous_commit=on")
        .execute(&mut **tx)
        .await
        .map_err(db)?;
    // The stream-head row lock serializes allocation and commit; sequence gaps cannot hide later commits.
    let cursor: i64 = sqlx::query_scalar(
        "UPDATE stream_heads SET cursor=cursor+1 WHERE stream='findings' RETURNING cursor",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(db)?;
    let old: Option<i64> =
        sqlx::query_scalar("SELECT revision FROM incidents WHERE id=$1 FOR UPDATE")
            .bind(f.incident_id)
            .fetch_optional(&mut **tx)
            .await
            .map_err(db)?;
    f.revision = old.unwrap_or(0) + 1;
    f.cursor = cursor;
    f.event_id = Uuid::new_v5(&f.incident_id, &f.revision.to_be_bytes());
    let data = serde_json::to_value(&f)?;
    sqlx::query("INSERT INTO incidents(id,chain_id,revision,data) VALUES($1,$2,$3,$4) ON CONFLICT(id) DO UPDATE SET revision=EXCLUDED.revision,data=EXCLUDED.data,updated_at=now()")
        .bind(f.incident_id).bind(f.chain_id as i64).bind(f.revision).bind(&data).execute(&mut **tx).await.map_err(db)?;
    sqlx::query("INSERT INTO finding_revisions(event_id,incident_id,revision,chain_id,block_hash,data) VALUES($1,$2,$3,$4,$5,$6)")
        .bind(f.event_id).bind(f.incident_id).bind(f.revision).bind(f.chain_id as i64).bind(f.position.block_hash.map(|h| h.to_vec())).bind(&data).execute(&mut **tx).await.map_err(db)?;
    sqlx::query("INSERT INTO delivery_outbox(stream,cursor,event_id,data,telegram_sent) VALUES('findings',$1,$2,$3,$4)")
        .bind(cursor).bind(f.event_id).bind(data).bind(f.source == "offline_replay").execute(&mut **tx).await.map_err(db)?;
    if f.reviewed {
        let reviewed_cursor: i64 = sqlx::query_scalar(
            "UPDATE stream_heads SET cursor=cursor+1 WHERE stream='reviewed' RETURNING cursor",
        )
        .fetch_one(&mut **tx)
        .await
        .map_err(db)?;
        let mut reviewed = f.clone();
        reviewed.cursor = reviewed_cursor;
        sqlx::query(
            "INSERT INTO delivery_outbox(stream,cursor,event_id,data) VALUES('reviewed',$1,$2,$3)",
        )
        .bind(reviewed_cursor)
        .bind(f.event_id)
        .bind(serde_json::to_value(reviewed)?)
        .execute(&mut **tx)
        .await
        .map_err(db)?;
    }
    Ok(f)
}
