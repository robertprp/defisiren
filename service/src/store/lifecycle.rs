use crate::store::publish;
use siren_lib::{entity::*,AppResult,Error};
use sqlx::{Postgres,Transaction,Row};
use serde_json::{Value,json};
use alloy_primitives::B256;
use uuid::Uuid;

pub async fn preconfirmation_corrections(tx:&mut Transaction<'_,Postgres>,block:&BlockBatch)->AppResult<Vec<Finding>>{
    let rows=sqlx::query("SELECT data FROM incidents WHERE chain_id=$1 AND data->>'finality'='preconfirmed' AND (data->'position'->>'block_number')::bigint=$2")
        .bind(block.chain_id as i64).bind(block.number as i64).fetch_all(&mut **tx).await.map_err(|_|Error::Database)?;
    let mut out=vec![];
    for row in rows{
        let mut f:Finding=serde_json::from_value(row.get("data"))?;
        let hash=f.evidence.iter().find_map(|e|e.transaction);
        let included=hash.and_then(|h|block.observations.iter().find(|o|o.hash==h));
        match included{
            Some(o)=>{f.finality=Stage::Included;f.position=o.position.clone();f.ordering=Ordering::BlockOrdered;},
            None=>{f.finality=Stage::Reorged;f.retracted=true;f.finding_type="preconfirmation_rollback".into();}
        }
        f.evidence.push(Evidence{id:format!("preconf:{}",block.hash),kind:"preconfirmation_resolution".into(),
            description:"Canonical block resolves the provisional observation; this is separate from a canonical-chain reorg.".into(),transaction:hash,
            data:json!({"canonical_hash":block.hash,"canonical_number":block.number})});
        out.push(publish(tx,f).await?);
    }
    Ok(out)
}

pub async fn bridge_retractions(tx:&mut Transaction<'_,Postgres>,chain:u64,ancestor:u64)->AppResult<Vec<Finding>>{
    let rows=sqlx::query("SELECT destination_chain,message_id FROM bridge_messages WHERE correlated AND (($1=1 AND source_block IN (SELECT hash FROM canonical_blocks WHERE chain_id=$1 AND number>$2 AND canonical)) OR (destination_chain=$1 AND destination_block IN (SELECT hash FROM canonical_blocks WHERE chain_id=$1 AND number>$2 AND canonical)))")
        .bind(chain as i64).bind(ancestor as i64).fetch_all(&mut **tx).await.map_err(|_|Error::Database)?;
    let mut out=vec![];
    for row in rows{
        let destination:i64=row.get("destination_chain");let bytes:Vec<u8>=row.get("message_id");
        if bytes.len()!=32{return Err(Error::Database);}let hash=B256::from_slice(&bytes);
        let id=Uuid::new_v5(&Uuid::NAMESPACE_OID,format!("bridge:{destination}:{hash}").as_bytes());
        let value:Option<Value>=sqlx::query_scalar("SELECT data FROM incidents WHERE id=$1").bind(id).fetch_optional(&mut **tx).await.map_err(|_|Error::Database)?;
        if let Some(value)=value{
            let mut f:Finding=serde_json::from_value(value)?;f.retracted=true;f.finality=Stage::Reorged;f.finding_type="bridge_retraction".into();
            out.push(publish(tx,f).await?);
        }
    }
    sqlx::query("UPDATE bridge_messages SET source_canonical=false,correlated=false WHERE $1=1 AND source_block IN (SELECT hash FROM canonical_blocks WHERE chain_id=$1 AND number>$2 AND canonical)")
        .bind(chain as i64).bind(ancestor as i64).execute(&mut **tx).await.map_err(|_|Error::Database)?;
    sqlx::query("UPDATE bridge_messages SET destination_canonical=false,correlated=false WHERE destination_chain=$1 AND destination_block IN (SELECT hash FROM canonical_blocks WHERE chain_id=$1 AND number>$2 AND canonical)")
        .bind(chain as i64).bind(ancestor as i64).execute(&mut **tx).await.map_err(|_|Error::Database)?;
    Ok(out)
}

pub async fn progress(tx:&mut Transaction<'_,Postgres>,progress:&crate::events::ChainProgress)->AppResult<Vec<Finding>>{
    if progress.pending_before>progress.observed_at{return Err(Error::Invalid("pending expiry time".into()));}
    let mut out=vec![];
    // Promote finalized first; safe never downgrades a finalized finding.
    for (stage,point) in [(Stage::Finalized,&progress.finalized),(Stage::Safe,&progress.safe)]{
        let Some(point)=point else{continue;};
        let known:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM canonical_blocks WHERE chain_id=$1 AND number=$2 AND hash=$3 AND canonical)")
            .bind(progress.chain_id as i64).bind(point.number as i64).bind(point.hash.as_slice())
            .fetch_one(&mut **tx).await.map_err(|_|Error::Database)?;
        if !known{continue;} // Backfill may not have reached the provider's tag yet.
        let name=if stage==Stage::Finalized{"finalized"}else{"safe"};
        sqlx::query("INSERT INTO finality_points(chain_id,stage,number,hash,observed_at,source) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT(chain_id,stage) DO UPDATE SET number=EXCLUDED.number,hash=EXCLUDED.hash,observed_at=EXCLUDED.observed_at,source=EXCLUDED.source")
            .bind(progress.chain_id as i64).bind(name).bind(point.number as i64).bind(point.hash.as_slice()).bind(progress.observed_at).bind(&progress.source)
            .execute(&mut **tx).await.map_err(|_|Error::Database)?;
        let stages=if stage==Stage::Finalized{vec!["included","safe"]}else{vec!["included"]};
        let rows=sqlx::query("SELECT i.data FROM incidents i JOIN canonical_blocks b ON b.chain_id=i.chain_id AND encode(b.hash,'hex')=substring(i.data->'position'->>'block_hash' from 3) WHERE i.chain_id=$1 AND b.canonical AND b.number<=$2 AND i.data->>'finality'=ANY($3::text[]) AND COALESCE((i.data->>'retracted')::boolean,false)=false ORDER BY b.number,i.id LIMIT 1000")
            .bind(progress.chain_id as i64).bind(point.number as i64).bind(stages).fetch_all(&mut **tx).await.map_err(|_|Error::Database)?;
        for row in rows{
            let mut f:Finding=serde_json::from_value(row.get("data"))?;f.finality=stage;
            f.evidence.push(Evidence{id:format!("finality:{name}:{}",point.hash),kind:"provider_finality".into(),
                description:"Provider native finality tag matched the ingested canonical block hash; ordering is unchanged.".into(),transaction:None,
                data:json!({"source":progress.source,"tag":name,"number":point.number,"hash":point.hash,"observed_at":progress.observed_at})});
            out.push(publish(tx,f).await?);
        }
    }
    let rows=sqlx::query("SELECT data FROM incidents WHERE chain_id=$1 AND data->>'finality'='pending' AND (data->>'observed_at')::timestamptz<$2 ORDER BY id LIMIT 1000")
        .bind(progress.chain_id as i64).bind(progress.pending_before).fetch_all(&mut **tx).await.map_err(|_|Error::Database)?;
    for row in rows{
        let mut f:Finding=serde_json::from_value(row.get("data"))?;
        f.finality=Stage::Expired;f.retracted=true;
        f.evidence.push(Evidence{id:format!("expiry:{}",progress.observed_at.timestamp()),kind:"pending_watch_expired".into(),
            description:"Observation exceeded the configured pending watch TTL. This does not prove transaction cancellation or prevent later inclusion.".into(),
            transaction:None,data:json!({"cutoff":progress.pending_before,"observed_at":progress.observed_at})});
        out.push(publish(tx,f).await?);
    }
    Ok(out)
}
