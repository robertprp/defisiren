use siren_service::{config::Config,events::Accepted,contracts,store::Store};
use alloy_primitives::B256;
#[tokio::test]
#[ignore="requires an isolated PostgreSQL DATABASE_URL; not a passed offline test"]
async fn db_atomic_replay_reorg_and_cursor_expiry(){
    let url=std::env::var("DATABASE_URL").expect("DATABASE_URL required for explicitly enabled integration");
    let db=Store::connect(&url).await.unwrap();let cfg=Config::load(std::path::Path::new("config/example.toml")).await.unwrap();
    let pools=contracts::pools().unwrap();let mut records=include_str!("../fixtures/demo.jsonl").lines().map(|l|serde_json::from_str::<Accepted>(l).unwrap()).collect::<Vec<_>>();
    for r in &records {db.accept(r,&cfg,&pools).await.unwrap();}
    let before=db.stream("findings",0,256).await.unwrap();assert!(before.iter().any(|f|f["finding_type"]=="funding_watch"));
    for r in &records{assert!(db.accept(r,&cfg,&pools).await.unwrap().is_empty());}assert_eq!(db.stream("findings",0,256).await.unwrap().len(),before.len());
    if let Accepted::Block(b)=&mut records[2]{b.hash=B256::repeat_byte(0x99);for o in &mut b.observations{o.hash=B256::repeat_byte(0x98);o.position.block_hash=Some(b.hash);o.calls.clear();o.trace_complete=false;}}
    db.accept(&records[2],&cfg,&pools).await.unwrap();assert!(db.stream("findings",0,256).await.unwrap().iter().any(|f|f["retracted"]==true));
    bridge_and_finality(&db,&cfg,&pools).await;
    pending_lifecycle(&db,&cfg,&pools).await;
    sqlx::query("UPDATE stream_heads SET floor=1 WHERE stream='findings'").execute(&db.pool).await.unwrap();assert!(db.stream("findings",0,10).await.is_err());
    let key=db.new_api_key("test").await.unwrap();assert!(db.authenticate(&key,10).await.unwrap());assert!(!db.authenticate("invalid",10).await.unwrap());
}
#[tokio::test]
#[ignore="requires explicit provider credentials; absence is unverified coverage"]
async fn live_provider_capabilities(){
    use siren_service::chain::ChainAdapter;
    let cfg=Config::load(std::path::Path::new("config/example.toml")).await.unwrap();
    for chain in &cfg.chains{let a=siren_service::chain::RpcAdapter::new(chain.clone(),&cfg).expect("credentials required");let report=a.capabilities().await.expect("live capability probe");assert_eq!(report.chain_id,chain.id);}
}

async fn bridge_and_finality(db:&Store,cfg:&Config,pools:&[siren_service::contracts::Pool]){
    use siren_lib::entity::{BlockBatch,Observation,Stage};
    use siren_service::{events::{ChainProgress,FinalityPoint},funding::bridge};
    let (height,parent)=db.cursor(1).await.unwrap().unwrap();
    let value:serde_json::Value=serde_json::from_str(include_str!("../fixtures/bridges.json")).unwrap();
    let mut source:Observation=serde_json::from_value(value["base"]["source"].clone()).unwrap();
    source.position.block_number=Some(height+1);source.position.block_hash=Some(B256::repeat_byte(0xa4));
    let mut destination:Observation=serde_json::from_value(value["base"]["destination"].clone()).unwrap();
    let id=bridge::sources(&source).unwrap().remove(0).message_id;
    destination.envelope["sourceHash"]=serde_json::json!(id);
    let l2=BlockBatch{chain_id:8453,number:1,hash:destination.position.block_hash.unwrap(),parent_hash:B256::ZERO,timestamp:1791288000,observations:vec![destination.clone()]};
    // Destination-first arrival must wait for the exact L1 message.
    let initial=db.accept(&Accepted::Block(l2.clone()),cfg,pools).await.unwrap();
    assert!(!initial.iter().any(|f|f.finding_type=="bridge_transfer"));
    let l1=BlockBatch{chain_id:1,number:height+1,hash:source.position.block_hash.unwrap(),parent_hash:parent,timestamp:1791288000,observations:vec![source]};
    let correlated=db.accept(&Accepted::Block(l1.clone()),cfg,pools).await.unwrap();
    assert!(correlated.iter().any(|f|f.finding_type=="bridge_transfer"&&f.chain_id==8453));
    let catchup=BlockBatch{chain_id:8453,number:2,hash:B256::repeat_byte(0xb2),parent_hash:l2.hash,timestamp:1791288001,observations:vec![]};
    db.accept(&Accepted::Block(catchup.clone()),cfg,pools).await.unwrap();
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM address_profiles WHERE chain_id=8453 AND address=$1")
        .bind(destination.to.unwrap().as_slice()).fetch_one(&db.pool).await.unwrap();assert_eq!(count,1);
    let now=chrono::DateTime::from_timestamp(1791288002,0).unwrap();
    let progress=Accepted::Progress(ChainProgress{chain_id:8453,source:"offline_replay".into(),observed_at:now,
        safe:Some(FinalityPoint{number:1,hash:l2.hash}),finalized:None,pending_before:now-chrono::Duration::seconds(900)});
    let promoted=db.accept(&progress,cfg,pools).await.unwrap();assert!(promoted.iter().any(|f|f.finality==Stage::Safe));
    // Orphaning L1 evidence retracts the bridge finding despite a still-canonical L2 transaction.
    let fork=BlockBatch{hash:B256::repeat_byte(0xa5),observations:vec![],..l1};
    let retracted=db.accept(&Accepted::Block(fork),cfg,pools).await.unwrap();
    assert!(retracted.iter().any(|f|f.finding_type=="bridge_retraction"&&f.retracted));
    let next=BlockBatch{number:3,hash:B256::repeat_byte(0xb3),parent_hash:catchup.hash,timestamp:1791288002,..catchup};
    db.accept(&Accepted::Block(next),cfg,pools).await.unwrap();
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM address_profiles WHERE chain_id=8453 AND address=$1")
        .bind(destination.to.unwrap().as_slice()).fetch_one(&db.pool).await.unwrap();assert_eq!(count,0);
}

async fn pending_lifecycle(db:&Store,cfg:&Config,pools:&[siren_service::contracts::Pool]){
    use siren_lib::entity::{Observation,Position,Stage,Ordering,BlockBatch};
    use siren_service::events::ChainProgress;
    let record:Accepted=serde_json::from_str(include_str!("../fixtures/demo.jsonl").lines().nth(1).unwrap()).unwrap();
    let Accepted::Block(mut block)=record else{panic!("block fixture")};
    let mut pending:Observation=block.observations.remove(0);
    pending.hash=B256::repeat_byte(0xe1);pending.stage=Stage::Pending;pending.ordering=Ordering::Candidate;
    pending.position=Position{block_number:None,block_hash:None,transaction_index:None,log_index:None};
    pending.success=None;pending.created=None;pending.nonce=2;
    let found=db.accept(&Accepted::Observation(pending.clone()),cfg,pools).await.unwrap();
    assert!(found.iter().any(|f|f.finality==Stage::Pending));
    let now=pending.observed_at+chrono::Duration::seconds(1800);
    let expired=db.accept(&Accepted::Progress(ChainProgress{chain_id:1,source:"offline_replay".into(),observed_at:now,
        safe:None,finalized:None,pending_before:now-chrono::Duration::seconds(900)}),cfg,pools).await.unwrap();
    assert!(expired.iter().any(|f|f.finality==Stage::Expired&&f.retracted));
    let (height,parent)=db.cursor(1).await.unwrap().unwrap();
    let hash=B256::repeat_byte(0xe2);
    pending.stage=Stage::Included;pending.ordering=Ordering::BlockOrdered;pending.success=Some(true);
    pending.position=Position{block_number:Some(height+1),block_hash:Some(hash),transaction_index:Some(0),log_index:None};
    let included=db.accept(&Accepted::Block(BlockBatch{chain_id:1,number:height+1,hash,parent_hash:parent,timestamp:1791288000,observations:vec![pending.clone()]}),cfg,pools).await.unwrap();
    assert!(included.iter().any(|f|f.finality==Stage::Included&&!f.retracted));
    pending.stage=Stage::Pending;pending.position.block_hash=None;pending.source="late_pending_source".into();
    assert!(db.accept(&Accepted::Observation(pending),cfg,pools).await.unwrap().is_empty());
}
