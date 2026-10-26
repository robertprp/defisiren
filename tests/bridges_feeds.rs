
use alloy_primitives::{B256,Bytes,keccak256};
use siren_lib::entity::{Observation,Ordering,Stage};
use siren_service::{chain::feeds,funding::bridge};
use serde_json::Value;

fn fixture(name:&str)->(Observation,Observation,B256){
    let value:Value=serde_json::from_str(include_str!("../fixtures/bridges.json")).unwrap();
    (serde_json::from_value(value[name]["source"].clone()).unwrap(),
     serde_json::from_value(value[name]["destination"].clone()).unwrap(),
     serde_json::from_value(value[name]["expected_id"].clone()).unwrap())
}
#[test]
fn bridge_ids_match_independent_protocol_vectors(){
    for name in ["base","arbitrum"]{
        let (source,destination,id)=fixture(name);
        let messages=bridge::sources(&source).unwrap();
        assert_eq!(messages.len(),1);
        assert_eq!(messages[0].message_id,id);
        assert!(bridge::matches(&messages[0],&destination).unwrap());
        assert_eq!(messages[0].source_finality,Stage::Included);
    }
}
#[test]
fn amounts_and_addresses_alone_never_correlate(){
    for name in ["base","arbitrum"]{
        let (source,mut destination,_)=fixture(name);
        let message=bridge::sources(&source).unwrap().remove(0);
        if name=="base"{destination.envelope["sourceHash"]=serde_json::json!(B256::ZERO);}
        else{destination.hash=B256::ZERO;}
        assert!(!bridge::matches(&message,&destination).unwrap());
        destination.chain_id=56;
        assert!(!bridge::matches(&message,&destination).unwrap());
    }
}
