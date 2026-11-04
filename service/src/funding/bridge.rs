use crate::contracts::{topic, word_address};
use alloy_primitives::{address, keccak256, Address, B256, U256};
use serde::{Deserialize, Serialize};
use siren_lib::{entity::*, AppResult, Error};

pub const BASE_PORTAL: Address = address!("49048044d57e1c92a77f79988d21fa8faf74e97e");
pub const ARB_INBOX: Address = address!("4dbd4fc535ac27206064b68ffcf827b0a60bab3f");
pub const ARB_BRIDGE: Address = address!("8315177ab297ba92a06054ce80a67ed4dbd7ed3a");

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BridgeSource {
    pub version: String,
    pub destination_chain: u64,
    pub message_id: B256,
    pub source_sender: Address,
    pub destination_recipient: Address,
    pub amount: String,
    pub source_transaction: B256,
    pub source_position: Position,
    pub source_finality: Stage,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub provenance_url: String,
}

fn dynamic_bytes(data: &[u8]) -> AppResult<&[u8]> {
    if data.len() < 64 || U256::from_be_slice(&data[..32]) != U256::from(32) {
        return Err(Error::Invalid("bridge dynamic ABI offset".into()));
    }
    let len = usize::try_from(U256::from_be_slice(&data[32..64]))
        .map_err(|_| Error::Limit("bridge ABI length".into()))?;
    if len > data.len() - 64 {
        return Err(Error::Invalid("bridge truncated dynamic data".into()));
    }
    let padded = len
        .checked_add(31)
        .map(|n| n / 32 * 32)
        .ok_or_else(|| Error::Limit("bridge length overflow".into()))?;
    if data.len() != 64 + padded || data[64 + len..].iter().any(|b| *b != 0) {
        return Err(Error::Invalid("bridge dynamic ABI padding".into()));
    }
    Ok(&data[64..64 + len])
}

pub fn op_source_hash(block_hash: B256, log_index: u64) -> B256 {
    let mut inner = [0u8; 64];
    inner[..32].copy_from_slice(block_hash.as_slice());
    inner[32..].copy_from_slice(&U256::from(log_index).to_be_bytes::<32>());
    let mut outer = [0u8; 64];
    outer[32..].copy_from_slice(keccak256(inner).as_slice());
    keccak256(outer)
}

fn rlp_bytes(bytes: &[u8]) -> Vec<u8> {
    if bytes.len() == 1 && bytes[0] < 128 {
        return bytes.to_vec();
    }
    let mut out = vec![];
    if bytes.len() < 56 {
        out.push(0x80 + bytes.len() as u8);
    } else {
        let len = (bytes.len() as u64).to_be_bytes();
        let first = len.iter().position(|b| *b != 0).unwrap_or(7);
        out.push(0xb7 + (8 - first) as u8);
        out.extend_from_slice(&len[first..]);
    }
    out.extend_from_slice(bytes);
    out
}

fn minimal(value: U256) -> Vec<u8> {
    let bytes = value.to_be_bytes::<32>();
    let first = bytes.iter().position(|b| *b != 0).unwrap_or(32);
    bytes[first..].to_vec()
}

pub fn arbitrum_deposit_hash(
    message_number: U256,
    aliased_sender: Address,
    to: Address,
    value: U256,
) -> B256 {
    let fields = [
        minimal(U256::from(42161)),
        message_number.to_be_bytes::<32>().to_vec(),
        aliased_sender.to_vec(),
        to.to_vec(),
        minimal(value),
    ];
    let payload = fields.iter().flat_map(|v| rlp_bytes(v)).collect::<Vec<_>>();
    let mut encoded = vec![0x64];
    if payload.len() < 56 {
        encoded.push(0xc0 + payload.len() as u8);
    } else {
        let len = (payload.len() as u64).to_be_bytes();
        let first = len.iter().position(|b| *b != 0).unwrap_or(7);
        encoded.push(0xf7 + (8 - first) as u8);
        encoded.extend_from_slice(&len[first..]);
    }
    encoded.extend(payload);
    keccak256(encoded)
}

pub fn alias(address: Address) -> Address {
    let offset = address!("1111000000000000000000000000000000001111");
    let sum = U256::from_be_slice(address.as_slice()) + U256::from_be_slice(offset.as_slice());
    Address::from_slice(&sum.to_be_bytes::<32>()[12..])
}

/// Narrow paths: direct native deposit into Base Portal v0 and Arbitrum Inbox kind 12.
pub fn sources(o: &Observation) -> AppResult<Vec<BridgeSource>> {
    if o.chain_id != 1 || o.success != Some(true) || !o.stage.canonical() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for log in o.logs.iter().filter(|l| !l.removed) {
        if log.address == BASE_PORTAL
            && o.to == Some(BASE_PORTAL)
            && log.topics.first()
                == Some(&topic(
                    "TransactionDeposited(address,address,uint256,bytes)",
                ))
        {
            if log.topics.len() != 4 || log.topics[3] != B256::ZERO {
                continue;
            }
            let sender = word_address(log.topics[1].as_slice())?;
            let to = word_address(log.topics[2].as_slice())?;
            if sender != o.from {
                continue;
            }
            let opaque = dynamic_bytes(&log.data)?;
            if opaque.len() != 73 || opaque[72] != 0 {
                continue;
            }
            let value = U256::from_be_slice(&opaque[32..64]);
            let mint = U256::from_be_slice(&opaque[..32]);
            if mint < value {
                return Err(Error::Invalid("Base deposit mint below value".into()));
            }
            let Some(block_hash) = o.position.block_hash else {
                return Err(Error::Invalid("bridge source state".into()));
            };
            let mut position = o.position.clone();
            position.log_index = Some(log.index);
            out.push(BridgeSource {
                version: "base-portal-native-v0/1".into(),
                destination_chain: 8453,
                message_id: op_source_hash(block_hash, log.index),
                source_sender: o.from,
                destination_recipient: to,
                amount: value.to_string(),
                source_transaction: o.hash,
                source_position: position,
                source_finality: o.stage,
                observed_at: o.observed_at,
                provenance_url: "https://docs.base.org/specifications/reference/base-contracts"
                    .into(),
            });
        }
        if log.address==ARB_BRIDGE&&o.to==Some(ARB_INBOX)&&log.topics.first()==Some(&topic("MessageDelivered(uint256,bytes32,address,uint8,address,bytes32,uint256,uint64)")){
            if log.topics.len()!=3||log.data.len()!=192{continue;}
            let inbox=word_address(&log.data[..32])?;let kind=U256::from_be_slice(&log.data[32..64]);let sender=word_address(&log.data[64..96])?;
            if inbox!=ARB_INBOX||kind!=U256::from(12)||sender!=alias(o.from){continue;}
            let Some(delivery)=o.logs.iter().find(|l|!l.removed&&l.address==ARB_INBOX&&l.topics.len()==2
                &&l.topics[0]==topic("InboxMessageDelivered(uint256,bytes)")&&l.topics[1]==log.topics[1])else{continue;};
            let data=dynamic_bytes(&delivery.data)?;
            if data.len()!=52||keccak256(data).as_slice()!=&log.data[96..128]{return Err(Error::Invalid("Arbitrum inbox data hash".into()));}
            let to=Address::from_slice(&data[..20]);let amount=U256::from_be_slice(&data[20..]);
            let id=arbitrum_deposit_hash(U256::from_be_slice(log.topics[1].as_slice()),sender,to,amount);
            let mut position=o.position.clone();position.log_index=Some(log.index);
            out.push(BridgeSource{version:"arbitrum-inbox-native-kind12/1".into(),destination_chain:42161,message_id:id,
                source_sender:o.from,destination_recipient:to,amount:amount.to_string(),source_transaction:o.hash,source_position:position,source_finality:o.stage,observed_at:o.observed_at,
                provenance_url:"https://github.com/OffchainLabs/arbitrum-sdk/blob/main/packages/sdk/src/lib/message/ParentToChildMessage.ts".into()});
        }
    }
    Ok(out)
}

pub fn destination_id(o: &Observation) -> AppResult<Option<B256>> {
    if o.success != Some(true) || !o.stage.canonical() {
        return Ok(None);
    }
    match (o.chain_id, o.tx_type) {
        (8453, 0x7e) => Ok(Some(serde_json::from_value(
            o.envelope
                .get("sourceHash")
                .cloned()
                .ok_or_else(|| Error::Dependency("Base deposit sourceHash".into()))?,
        )?)),
        (42161, 0x64) => Ok(Some(o.hash)),
        _ => Ok(None),
    }
}

pub fn matches(source: &BridgeSource, destination: &Observation) -> AppResult<bool> {
    Ok(destination.chain_id == source.destination_chain
        && destination_id(destination)? == Some(source.message_id)
        && destination.to == Some(source.destination_recipient)
        && destination.value.to_string() == source.amount)
}
