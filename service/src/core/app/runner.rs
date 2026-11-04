use crate::{
    api,
    chain::{BlockSource, ChainAdapter, PendingSource, RpcAdapter, TraceProvider},
    core::{signals, tasks::manager::TaskManager},
    detection,
    events::Accepted,
    ingestion,
    services::Services,
};
use alloy_primitives::{keccak256, Bytes, U256};
use serde_json::json;
use siren_lib::{entity::*, AppResult, Error};
use std::{
    sync::{atomic::Ordering, Arc},
    time::Duration,
};
use tokio_util::sync::CancellationToken;

pub async fn run(s: Arc<Services>) -> AppResult<()> {
    tracing::info!(mode=?s.config.mode,chains=s.chains.len(),rpc_per_second=s.config.rpc_requests_per_second,"Monitor starting");
    let mut tasks = TaskManager::default();
    for chain in &s.chains {
        let a = chain.clone();
        let state = s.clone();
        let cancel = tasks.cancel.clone();
        tasks.spawn(async move { canonical_loop(state, a, cancel).await });
        for base in [true, false] {
            if (base && chain.config.preconfirmed_ws_env.is_some())
                || (!base && chain.config.sequencer_ws_env.is_some())
            {
                let state = s.clone();
                let adapter = chain.clone();
                let cancel = tasks.cancel.clone();
                tasks.spawn(async move {
                    crate::chain::feeds::supervise(state, adapter, cancel, base).await
                });
            }
        }
        if chain.config.pending {
            let (tx, mut rx) = tokio::sync::mpsc::channel(1024);
            let a = chain.clone();
            let state = s.clone();
            let cancel = tasks.cancel.clone();
            tasks.spawn(async move{
                let mut failures=0u32;
                while !cancel.is_cancelled(){
                    let result=a.subscribe(tx.clone(),cancel.clone()).await;
                    if cancel.is_cancelled(){break;}
                    failures=failures.saturating_add(1);
                    let retry_s=(2u64.saturating_mul(1u64<<failures.min(5))).min(60);
                    if let Err(e)=result {state.metrics.source_gaps.fetch_add(1,Ordering::Relaxed);
                        if failures==1||failures.is_power_of_two(){tracing::warn!(chain_id=a.config.id,error=%e,retry_in_s=retry_s,"Pending feed disconnected; ephemeral coverage gap");}
                    }
                    tokio::select!{_=cancel.cancelled()=>break,_=tokio::time::sleep(Duration::from_secs(retry_s))=>{}}
                }Ok(())
            });
            let state = s.clone();
            let cancel = tasks.cancel.clone();
            tasks.spawn(async move{
                loop{tokio::select!{_=cancel.cancelled()=>{rx.close();break;},o=rx.recv()=>{match o{Some(o)=>{state.accept(Accepted::Observation(o)).await?;},None=>break}}}}
                while let Some(o)=rx.recv().await{state.accept(Accepted::Observation(o)).await?;}Ok(())
            });
        }
    }
    if s.config.telegram_token_env.is_some() && s.config.telegram_chat_env.is_some() {
        let state = s.clone();
        let cancel = tasks.cancel.clone();
        tasks.spawn(async move { crate::telegram::run(state, cancel).await });
    }
    if s.config.mode == Mode::Simulate {
        let state = s.clone();
        let cancel = tasks.cancel.clone();
        tasks.spawn(async move { crate::core::tasks::enrichment::run(state, cancel).await });
    }
    let listener = tokio::net::TcpListener::bind(&s.config.listen).await?;
    let cancel = tasks.cancel.clone();
    let router = api::router(s.clone());
    tasks.spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(cancel.cancelled_owned())
            .await?;
        Ok(())
    });
    let result = tokio::select! {
        result=signals::shutdown()=>result,
        result=tasks.tasks.join_next()=>match result{Some(Ok(Err(e)))=>Err(e),_=>Err(Error::Dependency("supervised worker exited".into()))}
    };
    let drained = tasks
        .shutdown(Duration::from_secs(s.config.shutdown_seconds))
        .await;
    let j = s.journal.clone();
    tokio::task::spawn_blocking(move || j.flush())
        .await
        .map_err(|_| Error::Durability)??;
    result.and(drained)
}

async fn verify_pools(s: &Services, a: &RpcAdapter) -> AppResult<()> {
    for pool in s.pools.iter().filter(|p| p.chain_id == a.config.id) {
        let code: Bytes = serde_json::from_value(
            a.pool
                .request("eth_getCode", json!([pool.address, "latest"]))
                .await?,
        )?;
        if code.is_empty() {
            return Err(Error::Provider("configured pool has no code".into()));
        }
        let selector = format!("0x{}", hex::encode(&keccak256(b"denomination()")[..4]));
        let raw: Bytes = serde_json::from_value(
            a.pool
                .request(
                    "eth_call",
                    json!([{"to":pool.address,"data":selector},"latest"]),
                )
                .await?,
        )?;
        if raw.len() != 32 || U256::from_be_slice(&raw) != pool.denomination {
            return Err(Error::Provider("pool denomination/ABI probe failed".into()));
        }
    }
    Ok(())
}

async fn canonical_loop(
    s: Arc<Services>,
    a: Arc<RpcAdapter>,
    cancel: CancellationToken,
) -> AppResult<()> {
    let mut initial_start = None;
    let mut trace_available = false;
    let mut last_status = std::time::Instant::now() - Duration::from_secs(60);
    let mut last_progress = std::time::Instant::now() - Duration::from_secs(120);
    let mut failures = 0u32;
    let mut last_probe = std::time::Instant::now() - Duration::from_secs(120);
    loop {
        if cancel.is_cancelled() {
            return Ok(());
        }
        let result=async{
            if last_probe.elapsed()>Duration::from_secs(60){
                verify_pools(&s,&a).await?;
                let report=a.capabilities().await?;
                trace_available=!report.traces.is_empty();
                tracing::debug!(chain_id=a.config.id,degraded=?report.degraded,"Capability probe");
                sqlx::query("INSERT INTO capabilities(chain_id,data) VALUES($1,$2) ON CONFLICT(chain_id) DO UPDATE SET data=EXCLUDED.data,updated_at=now()")
                    .bind(a.config.id as i64).bind(serde_json::to_value(report)?).execute(&s.store.pool).await.map_err(|_|Error::Database)?;
                last_probe=std::time::Instant::now();
            }
            let head=a.head().await?;
            let cursor=s.store.cursor(a.config.id).await?;
            let start=*initial_start.get_or_insert_with(||cursor.map(|(h,_)|h+1).unwrap_or(if a.config.start_at_head{head}else{a.config.start_block}));
            let mut next=cursor.map(|(h,_)|h+1).unwrap_or(start);
            if let Some((height,hash))=cursor{
                let current=a.pool.request("eth_getBlockByNumber",json!([format!("0x{height:x}"),false])).await?;
                if current["hash"]!=json!(hash){
                    let mut ancestor=None;
                    for depth in 1..=s.config.max_reorg_depth.min(height){
                        let n=height-depth;
                        let row:Option<Vec<u8>>=sqlx::query_scalar("SELECT hash FROM canonical_blocks WHERE chain_id=$1 AND number=$2 AND canonical")
                            .bind(a.config.id as i64).bind(n as i64).fetch_optional(&s.store.pool).await.map_err(|_|Error::Database)?;
                        let Some(old)=row else{continue;};
                        let new=a.pool.request("eth_getBlockByNumber",json!([format!("0x{n:x}"),false])).await?;
                        if new["hash"]==json!(format!("0x{}",hex::encode(old))){ancestor=Some(n);break;}
                    }
                    next=ancestor.ok_or_else(||Error::Limit("deep reorg; no safe ancestor within configured window".into()))?+1;
                }
            }
            if next<=head{
                // Canonical application is ordered. Limit each catch-up turn so shutdown and probes run.
                for number in next..=head.min(next+31){
                    if cancel.is_cancelled(){break;}
                    let mut block=a.block(number).await?;
                    let mut budget=s.config.trace_candidates_per_block;
                    for o in &mut block.observations{
                        if detection::candidate(o,&s.config){
                            s.metrics.candidates.fetch_add(1,Ordering::Relaxed);
                            if a.config.trace&&trace_available&&budget>0{budget-=1;
                                match a.trace(o.hash,block.number,block.hash).await.and_then(|v|ingestion::flatten_trace(&v,20_000)){
                                    Ok(calls)=>{o.calls=calls;o.trace_complete=true;s.metrics.traced.fetch_add(1,Ordering::Relaxed);},
                                    Err(_)=>{s.metrics.rpc_errors.fetch_add(1,Ordering::Relaxed);}
                                }
                            }
                        }
                        if !o.trace_complete{s.metrics.untraced.fetch_add(1,Ordering::Relaxed);}
                    }
                    s.accept(Accepted::Block(block)).await?;
                }
            }
            if last_progress.elapsed()>Duration::from_secs(30) && s.store.cursor(a.config.id).await?.is_some_and(|(height,_)|height>=head) {
                report_progress(&s,&a).await?;last_progress=std::time::Instant::now();
            }
            if last_status.elapsed()>Duration::from_secs(30){
                let committed=s.store.cursor(a.config.id).await?.map(|(h,_)|h);
                tracing::info!(chain_id=a.config.id,head,committed,lag_blocks=committed.map(|n|head.saturating_sub(n)),trace_available,pending=a.config.pending,"Chain progress");
                last_status=std::time::Instant::now();
            }
            Ok::<_,Error>(())
        }.await;
        let delay = match result {
            Ok(()) => {
                if failures > 0 {
                    tracing::info!(chain_id = a.config.id, "RPC recovered");
                }
                failures = 0;
                s.config.poll_ms
            }
            Err(Error::Durability) => return Err(Error::Durability),
            Err(e) => {
                failures = failures.saturating_add(1);
                s.metrics.rpc_errors.fetch_add(1, Ordering::Relaxed);
                let delay = match &e {
                    Error::RateLimited { retry_after_secs } => {
                        retry_after_secs.saturating_mul(1000)
                    }
                    _ => 2000u64.saturating_mul(1u64 << failures.min(5)).min(60_000),
                };
                if failures == 1 || failures.is_power_of_two() {
                    tracing::warn!(chain_id=a.config.id,error=%e,retry_in_s=delay/1000,failures,"Chain waiting; durable cursor retained");
                }
                delay
            }
        };
        tokio::select! {_=cancel.cancelled()=>return Ok(()),_=tokio::time::sleep(Duration::from_millis(delay+a.config.id%71))=>{}}
    }
}

async fn report_progress(s: &Services, a: &RpcAdapter) -> AppResult<()> {
    let Some((height, _)) = s.store.cursor(a.config.id).await? else {
        return Ok(());
    };
    let mut points = vec![];
    for tag in ["safe", "finalized"] {
        let point = match a
            .pool
            .request("eth_getBlockByNumber", json!([tag, false]))
            .await
        {
            Ok(value) if !value.is_null() => {
                let number = crate::chain::provider::quantity(&value["number"])?;
                let hash = crate::chain::provider::field(&value, "hash")?;
                (number <= height).then_some(crate::events::FinalityPoint { number, hash })
            }
            _ => {
                s.metrics.rpc_errors.fetch_add(1, Ordering::Relaxed);
                None
            }
        };
        points.push(point);
    }
    let finalized = points.pop().flatten();
    let safe = points.pop().flatten();
    let observed_at = chrono::Utc::now();
    s.accept(Accepted::Progress(crate::events::ChainProgress {
        chain_id: a.config.id,
        source: "native_rpc_tags".into(),
        observed_at,
        safe,
        finalized,
        pending_before: observed_at - chrono::Duration::seconds(s.config.pending_ttl_seconds),
    }))
    .await?;
    Ok(())
}
