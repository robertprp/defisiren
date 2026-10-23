use alloy_primitives::{Address,B256,Bytes,U256,keccak256};
use siren_lib::entity::*;
use siren_service::{analysis,contracts,detection::Engine,events::{Accepted,journal::Journal},funding,simulation,response};
use std::path::Path;

fn record(index:usize)->BlockBatch {
    let line=include_str!("../fixtures/demo.jsonl").lines().nth(index).expect("fixture index");
    match serde_json::from_str(line).expect("fixture JSON"){Accepted::Block(b)=>b,_=>panic!("block")}
}
async fn config()->siren_service::config::Config{siren_service::config::Config::load(Path::new("config/example.toml")).await.expect("config")}
#[test]
fn direct_and_relayed_recipient_is_not_sender(){
    let mut o=record(0).observations.remove(0);let pools=contracts::pools().unwrap();
    let relayed=funding::withdrawals(&o,&pools).unwrap();assert_eq!(relayed.len(),1);
    assert_ne!(relayed[0].to,o.from);assert_ne!(relayed[0].relayer,Some(relayed[0].to));
    o.from=relayed[0].to;
    assert_eq!(funding::withdrawals(&o,&pools).unwrap()[0].to,o.from);
}
#[test]
fn reverted_unrelated_removed_and_pending_do_not_prove_funding(){
    let original=record(0).observations.remove(0);let pools=contracts::pools().unwrap();
    for n in 0..4 {let mut o=original.clone();match n{0=>o.success=Some(false),1=>o.logs[0].address=Address::ZERO,2=>o.logs[0].removed=true,_=>o.stage=Stage::Pending};assert!(funding::withdrawals(&o,&pools).unwrap().is_empty());}
}
#[test]
fn erc20_fee_is_denomination_minus_fee(){
    let mut o=record(0).observations.remove(0);let pools=contracts::pools().unwrap();let pool=pools.iter().find(|p|p.asset.is_some()).unwrap();
    o.logs[0].address=pool.address;let mut data=o.logs[0].data.to_vec();data[64..96].copy_from_slice(&U256::from(17).to_be_bytes::<32>());o.logs[0].data=data.into();
    let edge=funding::withdrawals(&o,&pools).unwrap().remove(0);
    assert_eq!(edge.asset,pool.asset);assert_eq!(edge.amount,(pool.denomination-U256::from(17)).to_string());assert_eq!(edge.fee,Some("17".into()));
}
#[tokio::test]
async fn mixer_watch_is_neutral_old_nonce_does_not_become_fresh(){
    let cfg=config().await;let mut engine=Engine::default();let pools=contracts::pools().unwrap();
    let f=engine.process(&record(0).observations[0],&cfg,&pools).unwrap();assert_eq!(f[0].finding_type,"funding_watch");assert_eq!(f[0].severity,"informational");assert!(!f[0].reviewed);
    let mut o=record(1).observations.remove(0);o.nonce=50;engine.process(&o,&cfg,&pools).unwrap();
    assert_eq!(engine.watch.profiles[&o.from].freshness_confidence,0);assert!(!engine.watch.profiles[&o.from].history_complete);
}
#[tokio::test]
async fn dust_shared_accounts_and_hop_budget_stop_propagation(){
    let cfg=config().await;let pools=contracts::pools().unwrap();let edge=funding::withdrawals(&record(0).observations[0],&pools).unwrap().remove(0);
    let mut state=funding::WatchState::default();state.add(edge.clone(),&cfg.funding).unwrap();
    let mut o=record(2).observations.remove(0);o.calls.clear();o.value=U256::from(1);o.to=Some(Address::repeat_byte(0x51));
    assert!(state.propagate(&o,&cfg.funding,&[]).unwrap().is_empty());
    o.value=U256::from(10u64.pow(18));assert!(state.propagate(&o,&cfg.funding,&[o.from]).unwrap().is_empty());
    assert_eq!(state.propagate(&o,&cfg.funding,&[]).unwrap()[0].hop,1);
    o.from=o.to.unwrap();o.to=Some(Address::repeat_byte(0x52));assert_eq!(state.propagate(&o,&cfg.funding,&[]).unwrap()[0].hop,2);
    o.from=o.to.unwrap();o.to=Some(Address::repeat_byte(0x53));assert!(state.propagate(&o,&cfg.funding,&[]).unwrap().is_empty());
}
