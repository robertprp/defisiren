use alloy_primitives::{keccak256, Bytes, B256};
use serde_json::Value;
use siren_lib::entity::{Observation, Ordering, Stage};
use siren_service::{chain::feeds, funding::bridge};

fn fixture(name: &str) -> (Observation, Observation, B256) {
    let value: Value = serde_json::from_str(include_str!("../fixtures/bridges.json")).unwrap();
    (
        serde_json::from_value(value[name]["source"].clone()).unwrap(),
        serde_json::from_value(value[name]["destination"].clone()).unwrap(),
        serde_json::from_value(value[name]["expected_id"].clone()).unwrap(),
    )
}

#[test]
fn bridge_ids_match_independent_protocol_vectors() {
    for name in ["base", "arbitrum"] {
        let (source, destination, id) = fixture(name);
        let messages = bridge::sources(&source).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].message_id, id);
        assert!(bridge::matches(&messages[0], &destination).unwrap());
        assert_eq!(messages[0].source_finality, Stage::Included);
    }
}

#[test]
fn amounts_and_addresses_alone_never_correlate() {
    for name in ["base", "arbitrum"] {
        let (source, mut destination, _) = fixture(name);
        let message = bridge::sources(&source).unwrap().remove(0);
        if name == "base" {
            destination.envelope["sourceHash"] = serde_json::json!(B256::ZERO);
        } else {
            destination.hash = B256::ZERO;
        }
        assert!(!bridge::matches(&message, &destination).unwrap());
        destination.chain_id = 56;
        assert!(!bridge::matches(&message, &destination).unwrap());
    }
}

#[test]
fn bridge_requires_successful_canonical_source_and_destination() {
    for name in ["base", "arbitrum"] {
        let (original, destination, _) = fixture(name);
        for stage in [Stage::Pending, Stage::Preconfirmed, Stage::Reorged] {
            let mut source = original.clone();
            source.stage = stage;
            assert!(bridge::sources(&source).unwrap().is_empty());
            let mut dst = destination.clone();
            dst.stage = stage;
            assert_eq!(bridge::destination_id(&dst).unwrap(), None);
        }
        let mut source = original.clone();
        source.success = Some(false);
        assert!(bridge::sources(&source).unwrap().is_empty());
        source = original;
        for log in &mut source.logs {
            log.removed = true;
        }
        assert!(bridge::sources(&source).unwrap().is_empty());
    }
}

#[test]
fn missing_inbox_data_does_not_create_a_counterpart() {
    let (mut source, _, _) = fixture("arbitrum");
    source.logs.pop();
    assert!(bridge::sources(&source).unwrap().is_empty());
}

#[test]
fn inbox_hash_mismatch_and_malformed_abi_are_rejected() {
    let (mut source, _, _) = fixture("arbitrum");
    let mut data = source.logs[1].data.to_vec();
    data[64] ^= 1;
    source.logs[1].data = data.into();
    assert!(bridge::sources(&source).is_err());
    let (mut source, _, _) = fixture("base");
    source.logs[0].data = Bytes::from(vec![0xff; 64]);
    assert!(bridge::sources(&source).is_err());
}

#[test]
fn flashblocks_preserve_order_without_claiming_inclusion() {
    let raw: Value = serde_json::from_str(include_str!("../fixtures/flashblock.json")).unwrap();
    let observation = feeds::flashblock_transaction(raw.clone(), 8453).unwrap();
    assert_eq!(observation.stage, Stage::Preconfirmed);
    assert_eq!(observation.ordering, Ordering::Sequenced);
    assert_eq!(observation.position.block_hash, None);
    assert_eq!(observation.position.block_number, Some(100));
    assert!(feeds::flashblock_transaction(raw.clone(), 1).is_err());
    let mut wrong = raw;
    wrong["chainId"] = serde_json::json!("0x1");
    assert!(feeds::flashblock_transaction(wrong, 8453).is_err());
}

#[test]
fn nitro_framing_preserves_batch_order_and_reports_system_messages() {
    // Arbitrary signed bytes exercise framing only; not signature validation or EVM conformance.
    let a = [4u8, 2, 0xc0];
    let b = [4u8, 0xf8, 0x00];
    let system = [12u8, 0];
    let mut batch = vec![3u8];
    for child in [&a[..], &system[..], &b[..]] {
        batch.extend_from_slice(&(child.len() as u64).to_be_bytes());
        batch.extend_from_slice(child);
    }
    let (hashes, unsupported) = feeds::nitro_signed_hashes(&batch, 10).unwrap();
    assert_eq!(hashes, vec![keccak256(&a[1..]), keccak256(&b[1..])]);
    assert_eq!(unsupported, 1);
    assert!(feeds::nitro_signed_hashes(&batch, 1).is_err());
    batch.pop();
    assert!(feeds::nitro_signed_hashes(&batch, 10).is_err());
    assert!(feeds::nitro_signed_hashes(&[], 10).is_err());
}

#[test]
fn nitro_nested_batches_are_bounded() {
    let mut input = vec![4, 1];
    for _ in 0..18 {
        let mut outer = vec![3];
        outer.extend_from_slice(&(input.len() as u64).to_be_bytes());
        outer.extend(input);
        input = outer;
    }
    assert!(feeds::nitro_signed_hashes(&input, 100).is_err());
}
