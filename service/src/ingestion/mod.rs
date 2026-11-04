use crate::chain::provider::{field, quantity};
use alloy_primitives::{Address, Bytes, U256};
use chrono::Utc;
use serde_json::Value;
use siren_lib::{entity::*, AppResult, Error};
pub fn normalize(
    chain_id: u64,
    tx: Value,
    receipt: Option<Value>,
    stage: Stage,
    source: &str,
) -> AppResult<Observation> {
    let tx_type = tx.get("type").map(quantity).transpose()?.unwrap_or(0);
    if tx_type > 255 {
        return Err(Error::Invalid("transaction envelope type overflow".into()));
    }
    let r = receipt.as_ref();
    let position = Position {
        block_number: r.map(|v| quantity(&v["blockNumber"])).transpose()?,
        block_hash: r
            .map(|v| field::<Option<alloy_primitives::B256>>(v, "blockHash"))
            .transpose()?
            .flatten(),
        transaction_index: r.map(|v| quantity(&v["transactionIndex"])).transpose()?,
        log_index: None,
    };
    let mut logs = vec![];
    if let Some(r) = r {
        for l in r["logs"]
            .as_array()
            .ok_or_else(|| Error::Invalid("receipt logs".into()))?
        {
            logs.push(Log {
                address: field(l, "address")?,
                topics: field(l, "topics")?,
                data: field(l, "data")?,
                index: quantity(&l["logIndex"])?,
                removed: l["removed"].as_bool().unwrap_or(false),
            });
        }
    }
    let success = r
        .map(|v| {
            quantity(&v["status"]).and_then(|s| match s {
                0 => Ok(false),
                1 => Ok(true),
                _ => Err(Error::Invalid("receipt status".into())),
            })
        })
        .transpose()?;
    Ok(Observation {
        chain_id,
        hash: field(&tx, "hash")?,
        source: source.into(),
        observed_at: Utc::now(),
        stage,
        ordering: if stage.canonical() {
            Ordering::BlockOrdered
        } else {
            Ordering::Unknown
        },
        position,
        from: field(&tx, "from")?,
        to: field(&tx, "to")?,
        nonce: quantity(&tx["nonce"])?,
        tx_type: tx_type as u8,
        input: field(&tx, "input")?,
        value: field(&tx, "value")?,
        success,
        created: r
            .map(|v| field::<Option<Address>>(v, "contractAddress"))
            .transpose()?
            .flatten(),
        logs,
        calls: vec![],
        trace_complete: false,
        envelope: tx,
    })
}
pub fn flatten_trace(v: &Value, max_calls: usize) -> AppResult<Vec<Call>> {
    let mut stack = vec![(v, 0u32, false)];
    let mut calls = vec![];
    while let Some((v, depth, inherited_revert)) = stack.pop() {
        if calls.len() >= max_calls || depth > 1024 {
            return Err(Error::Limit("call trace".into()));
        }
        let error = v
            .get("error")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .or_else(|| inherited_revert.then(|| "ancestor reverted".into()));
        calls.push(Call {
            kind: v["type"]
                .as_str()
                .ok_or_else(|| Error::Invalid("trace call type".into()))?
                .into(),
            from: field(v, "from")?,
            to: v
                .get("to")
                .map(|x| serde_json::from_value(x.clone()))
                .transpose()?,
            input: v
                .get("input")
                .map(|x| serde_json::from_value(x.clone()))
                .transpose()?
                .unwrap_or_else(Bytes::new),
            value: v
                .get("value")
                .map(|x| serde_json::from_value(x.clone()))
                .transpose()?
                .unwrap_or(U256::ZERO),
            error: error.clone(),
            depth,
        });
        if let Some(children) = v["calls"].as_array() {
            for child in children.iter().rev() {
                stack.push((child, depth + 1, error.is_some()));
            }
        }
    }
    Ok(calls)
}
