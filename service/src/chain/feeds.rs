use super::{RpcAdapter, provider::quantity};
use crate::{config::Config, events::{Accepted, RawFeed}, ingestion::normalize, services::Services};
use alloy_primitives::{B256, keccak256};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use siren_lib::{entity::*, AppResult, Error};
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

/// Native Nitro signed-message framing only; unknown/system messages remain raw evidence.
/// Batch lengths are 8-byte big-endian. No network data is executed by this parser.
pub fn nitro_signed_hashes(bytes: &[u8], max_transactions: usize) -> AppResult<(Vec<B256>, usize)> {
    if bytes.len() > 16 * 1024 * 1024 { return Err(Error::Limit("Nitro message".into())); }
    let mut stack = vec![(bytes, 0usize)];
    let mut hashes = vec![];
    let mut unsupported = 0usize;
    while let Some((message, depth)) = stack.pop() {
        if depth > 16 || hashes.len() + unsupported >= max_transactions {
            return Err(Error::Limit("Nitro batch complexity".into()));
        }
        let Some((&kind, body)) = message.split_first() else {
            return Err(Error::Invalid("empty Nitro message".into()));
        };
        match kind {
            4 if !body.is_empty() => hashes.push(keccak256(body)),
            3 => {
                let mut offset = 0usize;
                let mut children = vec![];
                while offset < body.len() {
                    let header = body.get(offset..offset + 8).ok_or_else(|| Error::Invalid("Nitro batch length".into()))?;
                    let len = u64::from_be_bytes(header.try_into().map_err(|_| Error::Invalid("Nitro length".into()))?);
                    offset += 8;
                    let len = usize::try_from(len).map_err(|_| Error::Limit("Nitro segment".into()))?;
                    if len == 0 || len > body.len().saturating_sub(offset) {
                        return Err(Error::Invalid("Nitro truncated segment".into()));
                    }
                    children.push((&body[offset..offset + len], depth + 1));
                    if children.len() > max_transactions { return Err(Error::Limit("Nitro batch count".into())); }
                    offset += len;
                }
                stack.extend(children.into_iter().rev());
            }
            _ => unsupported += 1,
        }
    }
    Ok((hashes, unsupported))
}

pub fn flashblock_transaction(value: Value, chain: u64) -> AppResult<Observation> {
    if chain != 8453 { return Err(Error::Unsupported("Flashblocks adapter chain not validated".into())); }
    if value.get("chainId").is_some() && quantity(&value["chainId"])? != chain {
        return Err(Error::Provider("Flashblocks transaction chain mismatch".into()));
    }
    let mut observation = normalize(chain, value.clone(), Some(value), Stage::Preconfirmed, "base_flashblocks")?;
    observation.ordering = Ordering::Sequenced;
    // A Flashblock has a tentative position, never a canonical block hash.
    observation.position.block_hash = None;
    Ok(observation)
}

