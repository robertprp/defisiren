use crate::{config::FundingConfig, contracts::{Pool, topic, word_address}};
use alloy_primitives::{Address, U256};
use chrono::Duration;
use serde::{Deserialize, Serialize};
use siren_lib::{entity::*, AppResult, Error};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// No recipient inference from transaction sender or a call to a mixer.
pub fn withdrawals(o: &Observation, pools: &[Pool]) -> AppResult<Vec<FundingEdge>> {
    let mut edges = Vec::new();
    if o.success != Some(true) || !o.stage.canonical() { return Ok(edges); }
    for log in &o.logs {
        let Some(pool) = pools.iter().find(|p| p.chain_id == o.chain_id && p.address == log.address
            && o.position.block_number.is_some_and(|b| b >= p.deployment_block)) else { continue; };
        if log.removed || log.topics.first() != Some(&topic("Withdrawal(address,bytes32,address,uint256)")) { continue; }
        if log.topics.len() != 2 || log.data.len() != 96 { return Err(Error::Invalid("Withdrawal ABI".into())); }
        let recipient = word_address(&log.data[..32])?;
        let relayer = word_address(log.topics[1].as_slice())?;
        let fee = U256::from_be_slice(&log.data[64..]);
        let amount = pool.denomination.checked_sub(fee).ok_or_else(|| Error::Invalid("withdrawal fee".into()))?;
        edges.push(FundingEdge { id: format!("{}:{}:{}", o.chain_id, o.hash, log.index), chain_id: o.chain_id,
            from: pool.address, to: recipient, asset: pool.asset, amount: amount.to_string(), fee: Some(fee.to_string()),
            relayer: Some(relayer), transaction: o.hash, log_index: Some(log.index), block_hash: o.position.block_hash,
            observed_at: o.observed_at, provisional: false, bridge_origin: None, hop: 0, confidence: 10000 });
    }
    Ok(edges)
}
