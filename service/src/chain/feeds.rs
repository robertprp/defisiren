use super::{provider::quantity, RpcAdapter};
use crate::{
    config::Config,
    events::{Accepted, RawFeed},
    ingestion::normalize,
    services::Services,
};
use alloy_primitives::{keccak256, B256};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use siren_lib::{entity::*, AppResult, Error};
use std::{sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

/// Native Nitro signed-message framing only; unknown/system messages remain raw evidence.
/// Batch lengths are 8-byte big-endian. No network data is executed by this parser.
pub fn nitro_signed_hashes(bytes: &[u8], max_transactions: usize) -> AppResult<(Vec<B256>, usize)> {
    if bytes.len() > 16 * 1024 * 1024 {
        return Err(Error::Limit("Nitro message".into()));
    }
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
                    let header = body
                        .get(offset..offset + 8)
                        .ok_or_else(|| Error::Invalid("Nitro batch length".into()))?;
                    let len = u64::from_be_bytes(
                        header
                            .try_into()
                            .map_err(|_| Error::Invalid("Nitro length".into()))?,
                    );
                    offset += 8;
                    let len =
                        usize::try_from(len).map_err(|_| Error::Limit("Nitro segment".into()))?;
                    if len == 0 || len > body.len().saturating_sub(offset) {
                        return Err(Error::Invalid("Nitro truncated segment".into()));
                    }
                    children.push((&body[offset..offset + len], depth + 1));
                    if children.len() > max_transactions {
                        return Err(Error::Limit("Nitro batch count".into()));
                    }
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
    if chain != 8453 {
        return Err(Error::Unsupported(
            "Flashblocks adapter chain not validated".into(),
        ));
    }
    if value.get("chainId").is_some() && quantity(&value["chainId"])? != chain {
        return Err(Error::Provider(
            "Flashblocks transaction chain mismatch".into(),
        ));
    }
    let mut observation = normalize(
        chain,
        value.clone(),
        Some(value),
        Stage::Preconfirmed,
        "base_flashblocks",
    )?;
    observation.ordering = Ordering::Sequenced;
    // A Flashblock has a tentative position, never a canonical block hash.
    observation.position.block_hash = None;
    Ok(observation)
}

async fn persist_raw(
    s: &Services,
    chain: u64,
    source: &str,
    sequence: Option<u64>,
    payload: Value,
) -> AppResult<()> {
    s.accept(Accepted::Feed(RawFeed {
        chain_id: chain,
        source: source.into(),
        sequence,
        observed_at: chrono::Utc::now(),
        ordering: Ordering::Sequenced,
        payload,
    }))
    .await?;
    Ok(())
}

pub async fn flashblocks(
    s: Arc<Services>,
    adapter: Arc<RpcAdapter>,
    cancel: CancellationToken,
) -> AppResult<()> {
    let key = adapter
        .config
        .preconfirmed_ws_env
        .as_deref()
        .ok_or_else(|| Error::Dependency("Flashblocks endpoint".into()))?;
    let url = Config::secret(key)?;
    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|_| Error::Provider("Flashblocks connect".into()))?;
    ws.send(Message::Text(
        json!({"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]})
            .to_string()
            .into(),
    ))
    .await
    .map_err(|_| Error::Provider("Flashblocks chain probe".into()))?;
    let probe = tokio::time::timeout(Duration::from_secs(10), ws.next())
        .await
        .map_err(|_| Error::Provider("Flashblocks probe timeout".into()))?
        .ok_or_else(|| Error::Provider("Flashblocks probe closed".into()))?
        .map_err(|_| Error::Provider("Flashblocks probe transport".into()))?;
    let value: Value = serde_json::from_str(
        probe
            .to_text()
            .map_err(|_| Error::Invalid("Flashblocks probe encoding".into()))?,
    )?;
    if quantity(&value["result"])? != adapter.config.id {
        return Err(Error::Provider("Flashblocks chain ID".into()));
    }
    ws.send(Message::Text(json!({"jsonrpc":"2.0","id":2,"method":"eth_subscribe","params":["newFlashblockTransactions",true]}).to_string().into()))
        .await.map_err(|_| Error::Provider("Flashblocks subscribe".into()))?;
    let mut subscription = None;
    loop {
        let frame = tokio::select! { _ = cancel.cancelled() => return Ok(()), f = ws.next() => f };
        let frame = frame
            .ok_or_else(|| Error::Provider("Flashblocks disconnected".into()))?
            .map_err(|_| Error::Provider("Flashblocks frame".into()))?;
        if frame.len() > s.config.max_input_bytes {
            return Err(Error::Limit("Flashblocks frame".into()));
        }
        if let Message::Ping(data) = frame {
            ws.send(Message::Pong(data))
                .await
                .map_err(|_| Error::Provider("Flashblocks pong".into()))?;
            continue;
        }
        if !frame.is_text() {
            continue;
        }
        let value: Value = serde_json::from_str(
            frame
                .to_text()
                .map_err(|_| Error::Invalid("Flashblocks text".into()))?,
        )?;
        if !value["error"].is_null() {
            return Err(Error::Unsupported(
                "Flashblocks subscription rejected".into(),
            ));
        }
        if value["id"] == 2 {
            subscription = value["result"].as_str().map(str::to_owned);
            continue;
        }
        if subscription.as_deref()
            != value
                .pointer("/params/subscription")
                .and_then(Value::as_str)
        {
            continue;
        }
        let Some(payload) = value.pointer("/params/result") else {
            continue;
        };
        persist_raw(
            &s,
            adapter.config.id,
            "base_flashblocks",
            None,
            payload.clone(),
        )
        .await?;
        match flashblock_transaction(payload.clone(), adapter.config.id) {
            Ok(o) => {
                s.accept(Accepted::Observation(o)).await?;
            }
            Err(_) => {
                s.metrics
                    .unsupported_inputs
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }
}

pub async fn nitro(
    s: Arc<Services>,
    adapter: Arc<RpcAdapter>,
    cancel: CancellationToken,
) -> AppResult<()> {
    if adapter.config.id != 42161 {
        return Err(Error::Unsupported(
            "Nitro feed chain requires conformance configuration".into(),
        ));
    }
    let key = adapter
        .config
        .sequencer_ws_env
        .as_deref()
        .ok_or_else(|| Error::Dependency("Nitro relay endpoint".into()))?;
    let url = Config::secret(key)?;
    // Standard relay is a broadcast protocol, not JSON-RPC. Companion RPC proves configured chain.
    if quantity(&adapter.pool.request("eth_chainId", json!([])).await?)? != adapter.config.id {
        return Err(Error::Provider("Nitro companion chain ID".into()));
    }
    let (mut ws, _) = tokio_tungstenite::connect_async(&url)
        .await
        .map_err(|_| Error::Provider("Nitro relay connect".into()))?;
    let mut previous = None;
    loop {
        let frame = tokio::select! { _ = cancel.cancelled() => return Ok(()), f = ws.next() => f };
        let frame = frame
            .ok_or_else(|| Error::Provider("Nitro relay disconnected".into()))?
            .map_err(|_| Error::Provider("Nitro relay receive".into()))?;
        if frame.len() > s.config.max_input_bytes {
            return Err(Error::Limit("Nitro frame".into()));
        }
        if let Message::Ping(data) = frame {
            ws.send(Message::Pong(data))
                .await
                .map_err(|_| Error::Provider("Nitro pong".into()))?;
            continue;
        }
        if !frame.is_text() {
            return Err(Error::Unsupported(
                "use an uncompressed Nitro feed relay".into(),
            ));
        }
        let value: Value = serde_json::from_str(
            frame
                .to_text()
                .map_err(|_| Error::Invalid("Nitro frame encoding".into()))?,
        )?;
        if value["version"] != 1 {
            persist_raw(&s, adapter.config.id, "nitro_sequencer", None, value).await?;
            return Err(Error::Unsupported("Nitro broadcast version".into()));
        }
        let Some(messages) = value["messages"].as_array() else {
            continue;
        };
        for message in messages {
            let seq = message["sequenceNumber"]
                .as_u64()
                .ok_or_else(|| Error::Invalid("Nitro sequence number".into()))?;
            persist_raw(
                &s,
                adapter.config.id,
                "nitro_sequencer",
                Some(seq),
                message.clone(),
            )
            .await?;
            if previous.is_some_and(|n| seq > n + 1) {
                s.metrics
                    .source_gaps
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            previous = Some(seq);
            if message
                .pointer("/message/message/header/kind")
                .and_then(Value::as_u64)
                != Some(3)
            {
                s.metrics
                    .unsupported_inputs
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                continue;
            }
            let Some(encoded) = message
                .pointer("/message/message/l2Msg")
                .and_then(Value::as_str)
            else {
                continue;
            };
            let decoded = STANDARD
                .decode(encoded)
                .map_err(|_| Error::Invalid("Nitro base64".into()))?;
            let (hashes, unsupported) = match nitro_signed_hashes(&decoded, 10000) {
                Ok(result) => result,
                Err(_) => {
                    s.metrics
                        .unsupported_inputs
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    continue;
                }
            };
            s.metrics
                .unsupported_inputs
                .fetch_add(unsupported as u64, std::sync::atomic::Ordering::Relaxed);
            // Hydration trusts the chain's native RPC envelope, not the unverified relay signature.
            // Raw wire evidence is already durable if hydration has to wait for RPC availability.
            for hash in hashes {
                let mut hydrated = None;
                for attempt in 0..3 {
                    let value = adapter
                        .pool
                        .request("eth_getTransactionByHash", json!([hash]))
                        .await?;
                    if !value.is_null() {
                        hydrated = Some(value);
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(50 * (attempt + 1))).await;
                }
                let Some(raw) = hydrated else {
                    s.metrics
                        .source_gaps
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    continue;
                };
                if raw["hash"] != json!(hash) {
                    return Err(Error::Provider("Nitro hydration hash mismatch".into()));
                }
                let mut o = normalize(
                    adapter.config.id,
                    raw,
                    None,
                    Stage::Preconfirmed,
                    "nitro_sequencer",
                )?;
                o.ordering = Ordering::Sequenced;
                o.envelope["siren_sequence_number"] = json!(seq);
                o.envelope["siren_feed_signature_verified"] = json!(false);
                s.accept(Accepted::Observation(o)).await?;
            }
        }
    }
}

pub async fn supervise(
    s: Arc<Services>,
    adapter: Arc<RpcAdapter>,
    cancel: CancellationToken,
    base: bool,
) -> AppResult<()> {
    while !cancel.is_cancelled() {
        let result = if base {
            flashblocks(s.clone(), adapter.clone(), cancel.clone()).await
        } else {
            nitro(s.clone(), adapter.clone(), cancel.clone()).await
        };
        if let Err(error) = result {
            if matches!(error, Error::Durability | Error::Database) {
                return Err(error);
            }
            s.metrics
                .source_gaps
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            tracing::warn!(
                chain_id = adapter.config.id,
                "ordered feed disconnected/degraded; reconnecting"
            );
        }
        tokio::select! { _ = cancel.cancelled() => break, _ = tokio::time::sleep(Duration::from_secs(2)) => {} }
    }
    Ok(())
}
