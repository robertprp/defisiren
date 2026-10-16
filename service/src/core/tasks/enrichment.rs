use crate::{services::Services,simulation::SimulationBackend,store::{publish,evidence::EvidenceStore}};
use siren_lib::{entity::*,AppResult,Error};
use std::{sync::Arc,time::Duration};
use tokio_util::sync::CancellationToken;
use serde_json::{Value,json};
/// One bounded enrichment worker; raw publication is independent of this expensive path.
pub async fn run(s:Arc<Services>,cancel:CancellationToken)->AppResult<()> {
    let objects=EvidenceStore::new(&std::path::Path::new(&s.config.data_dir).join("evidence"),s.config.max_input_bytes)?;
    let mut after=0i64;
    loop {
        tokio::select!{_=cancel.cancelled()=>return Ok(()),_=tokio::time::sleep(Duration::from_millis(250))=>{}}
        let rows=s.store.stream("findings",after,16).await?;
        for value in rows {
            let f:Finding=serde_json::from_value(value)?;after=f.cursor;
            if f.source=="offline_replay"||f.retracted||!f.finality.canonical()||!matches!(f.severity.as_str(),"high"|"medium")||f.simulation.fidelity!="none"{continue;}
            let Some(hash)=f.evidence.iter().find_map(|e|e.transaction)else{continue;};
            let Some(adapter)=s.chains.iter().find(|a|a.config.id==f.chain_id)else{continue;};
            let row:Option<Value>=sqlx::query_scalar("SELECT data FROM observations WHERE chain_id=$1 AND hash=$2 AND canonical ORDER BY id DESC LIMIT 1")
                .bind(f.chain_id as i64).bind(hash.as_slice()).fetch_optional(&s.store.pool).await.map_err(|_|Error::Database)?;
            let Some(row)=row else{continue;};let o:Observation=serde_json::from_value(row)?;
            let mut result=match adapter.simulate(&o).await{Ok(r)=>r,Err(_)=>{s.metrics.simulation_errors.fetch_add(1,std::sync::atomic::Ordering::Relaxed);continue;}};
            for e in &mut result.evidence {
                let object=e.data.clone();let store=objects.clone();
                let (checksum,location,bytes)=tokio::task::spawn_blocking(move||store.put(&object)).await.map_err(|_|Error::Durability)??;
                sqlx::query("INSERT INTO evidence_objects(checksum,location,bytes) VALUES($1,$2,$3) ON CONFLICT DO NOTHING")
                    .bind(&checksum).bind(&location).bind(bytes as i64).execute(&s.store.pool).await.map_err(|_|Error::Database)?;
                e.data=json!({"sha256":checksum,"evidence_object":location,"encoding":"zstd","uncompressed_bytes":bytes});
            }
            let mut tx=s.store.pool.begin().await.map_err(|_|Error::Database)?;
            sqlx::query("SELECT pg_advisory_xact_lock($1)").bind(f.chain_id as i64).execute(&mut *tx).await.map_err(|_|Error::Database)?;
            sqlx::query("SELECT cursor FROM stream_heads WHERE stream='findings' FOR UPDATE").execute(&mut *tx).await.map_err(|_|Error::Database)?;
            let latest:Value=sqlx::query_scalar("SELECT data FROM incidents WHERE id=$1 FOR UPDATE").bind(f.incident_id).fetch_one(&mut *tx).await.map_err(|_|Error::Database)?;
            let mut latest:Finding=serde_json::from_value(latest)?;
            if latest.revision!=f.revision||latest.retracted{tx.rollback().await.map_err(|_|Error::Database)?;continue;}
            latest.asset_deltas.extend(result.deltas.clone());latest.simulation=result;
            // Enrichment is evidence, not deterministic exploit confirmation or LLM review.
            publish(&mut tx,latest).await?;tx.commit().await.map_err(|_|Error::Database)?;
        }
    }
}
