pub mod feeds;
pub mod provider;
use crate::{config::{ChainConfig, Config}, ingestion::normalize};
use alloy_primitives::B256;
use async_trait::async_trait;
use chrono::Utc;
use futures_util::{stream, StreamExt, TryStreamExt, SinkExt};
use provider::{ProviderPool, field, quantity};
use serde_json::{Value, json};
use siren_lib::{entity::*, AppResult, Error};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[async_trait]
pub trait ChainAdapter: Send + Sync {
    fn chain_id(&self) -> u64;
    async fn capabilities(&self) -> AppResult<Capability>;
}
#[async_trait]
pub trait BlockSource: Send + Sync {
    async fn block(&self, number: u64) -> AppResult<BlockBatch>;
    async fn head(&self) -> AppResult<u64>;
}
#[async_trait]
pub trait PendingSource: Send + Sync {
    async fn subscribe(&self, tx: mpsc::Sender<Observation>, cancel: CancellationToken) -> AppResult<()>;
}
#[async_trait]
pub trait TraceProvider: Send + Sync {
    async fn trace(&self, hash: B256, height: u64, block_hash: B256) -> AppResult<Value>;
}
pub struct RpcAdapter { pub config: ChainConfig, pub pool: Arc<ProviderPool>, pub budget: usize, bulk_receipts: std::sync::atomic::AtomicBool }
impl RpcAdapter {
    pub fn new(config: ChainConfig, cfg: &Config) -> AppResult<Self> {
        Ok(Self { pool: Arc::new(ProviderPool::new(&config, cfg)?), config, budget: cfg.rpc_concurrency, bulk_receipts: std::sync::atomic::AtomicBool::new(true) })
    }
}
#[async_trait]
impl ChainAdapter for RpcAdapter {
    fn chain_id(&self) -> u64 { self.config.id }
    async fn capabilities(&self) -> AppResult<Capability> {
        let head = self.pool.request("eth_getBlockByNumber", json!(["latest", false])).await?;
        let mut degraded = vec![]; let mut traces = vec![];
        if self.config.trace {
            if let Some(hash) = head["transactions"].as_array().and_then(|a| a.first()) {
                match self.pool.request("debug_traceTransaction", json!([hash,{"tracer":"callTracer","timeout":"2s"}])).await {
                    Ok(_) => traces.push("debug_traceTransaction/callTracer".into()),
                    Err(e @ Error::RateLimited{..}) => return Err(e),
                    Err(e) => degraded.push(format!("native callTracer: {e}")),
                }
            } else { degraded.push("trace probe inconclusive: empty latest block".into()); }
        } else { degraded.push("internal execution coverage disabled".into()); }
        let mut tags=Vec::new();
        for tag in ["safe","finalized"] {
            match self.pool.request("eth_getBlockByNumber",json!([tag,false])).await {
                Ok(value) if value["hash"].is_string()=>tags.push(tag),
                Err(e @ Error::RateLimited{..})=>return Err(e),
                _=>degraded.push(format!("native {tag} tag unavailable")),
            }
        }
        let finality=if tags.is_empty(){"native finality unavailable; no finality inferred from depth".to_owned()}
            else{format!("provider tags: {}; L1 settlement evaluated separately",tags.join(", "))};
        let historical = match self.pool.request("eth_getBalance", json!(["0x0000000000000000000000000000000000000000",if self.config.start_at_head { "latest".to_owned() } else { format!("0x{:x}",self.config.start_block) }])).await {
            Ok(_) if self.config.start_at_head => "latest state available; historical archive range not probed",
            Ok(_) => "configured start-block probe passed; arbitrary archive range not proven",
            Err(e @ Error::RateLimited{..}) => return Err(e),
            Err(_) => "unavailable at configured start block",
        };
        let pool_count = crate::contracts::pools()?.iter().filter(|p| p.chain_id == self.config.id).count();
        if pool_count == 0 { degraded.push("no verified configured Tornado pools".into()); }
        if self.config.family=="op" && self.config.preconfirmed_ws_env.is_none() { degraded.push("Flashblocks endpoint not configured".into()); }
        if self.config.family=="nitro" { degraded.push("sequencer wire signature not independently verified; system messages retained for native-client analysis".into()); if self.config.sequencer_ws_env.is_none() { degraded.push("Nitro feed relay not configured".into()); } }
        Ok(Capability { chain_id: self.config.id, family: self.config.family.clone(), transaction_types: match self.config.family.as_str() {
            "ethereum" => vec![0,1,2,3,4], "op" => vec![0,1,2,0x7e], "nitro" => vec![0,1,2,0x64,0x65,0x66,0x68,0x69,0x6a], _ => vec![0,1,2] },
            pending_visibility: if self.config.pending { "configured subscription; acceptance and hydration must succeed; source-specific coverage" } else { "none" }.into(),
            preconfirmation_semantics: match self.config.family.as_str() { "op" => "Flashblocks already executed provisionally", "nitro" => "sequencer observations already ordered", _ => "none" }.into(),
            traces, historical_state: historical.into(), finality, system_contracts: vec![],
            response_submission: vec![], verified_at: Some(Utc::now()), degraded })
    }
}
#[async_trait]
impl BlockSource for RpcAdapter {
    async fn head(&self) -> AppResult<u64> { quantity(&self.pool.request("eth_blockNumber", json!([])).await?) }
    async fn block(&self, number: u64) -> AppResult<BlockBatch> {
        let raw = self.pool.request("eth_getBlockByNumber", json!([format!("0x{number:x}"), true])).await?;
        if quantity(&raw["number"])? != number { return Err(Error::Provider("block height mismatch".into())); }
        let hash: B256 = field(&raw, "hash")?;
        let transactions = raw["transactions"].as_array().ok_or_else(|| Error::Invalid("block transactions".into()))?;
        let observations = if transactions.is_empty() { vec![] } else {
            let bulk = if self.bulk_receipts.load(std::sync::atomic::Ordering::Relaxed) {
                match self.pool.request("eth_getBlockReceipts",json!([format!("0x{number:x}")])).await {
                    Ok(value)=>Some(value),
                    Err(Error::Unsupported(_))=>{
                        self.bulk_receipts.store(false,std::sync::atomic::Ordering::Relaxed);
                        tracing::warn!(chain_id=self.config.id,"Block-receipt RPC unavailable; using paced individual receipts");
                        None
                    }
                    Err(error)=>return Err(error),
                }
            } else { None };
            if let Some(value)=bulk {
                let receipts=value.as_array().ok_or_else(||Error::Provider("block receipts unavailable".into()))?;
                if receipts.len()!=transactions.len(){return Err(Error::Provider("block receipt count mismatch".into()));}
                let mut indexed=std::collections::BTreeMap::new();
                for receipt in receipts {
                    let index=quantity(&receipt["transactionIndex"])?;
                    if indexed.insert(index,receipt).is_some(){return Err(Error::Provider("duplicate receipt index".into()));}
                }
                transactions.iter().cloned().enumerate().map(|(index,tx)|{
                    let receipt=indexed.get(&(index as u64)).ok_or_else(||Error::Provider("missing receipt index".into()))?;
                    validate_receipt(receipt,&tx,hash,number,index)?;
                    normalize(self.config.id,tx,Some((**receipt).clone()),Stage::Included,"canonical_rpc")
                }).collect::<AppResult<Vec<_>>>()?
            } else {
                // Fail fast on rate limiting instead of completing a whole block's doomed requests.
                stream::iter(transactions.iter().cloned().enumerate()).map(|(index,tx)|async move{
                    let txhash:B256=field(&tx,"hash")?;
                    let receipt=self.pool.request("eth_getTransactionReceipt",json!([txhash])).await?;
                    validate_receipt(&receipt,&tx,hash,number,index)?;
                    normalize(self.config.id,tx,Some(receipt),Stage::Included,"canonical_rpc")
                }).buffered(self.budget).try_collect::<Vec<_>>().await?
            }
        };
        Ok(BlockBatch { chain_id: self.config.id, number, hash, parent_hash: field(&raw,"parentHash")?, timestamp: quantity(&raw["timestamp"])?, observations })
    }
}
#[async_trait]
impl TraceProvider for RpcAdapter {
    async fn trace(&self, hash: B256, height: u64, block_hash: B256) -> AppResult<Value> {
        self.pool.anchored("debug_traceTransaction", json!([hash,{"tracer":"callTracer","timeout":"5s","tracerConfig":{"withLog":true}}]), height, block_hash).await
    }
}
#[async_trait]
impl PendingSource for RpcAdapter {
    async fn subscribe(&self, tx: mpsc::Sender<Observation>, cancel: CancellationToken) -> AppResult<()> {
        if !self.config.pending { return Err(Error::Unsupported("genuine pending feed not configured".into())); }
        let key = self.config.ws_env.as_ref().ok_or_else(|| Error::Dependency("pending WS environment reference".into()))?;
        let url = Config::secret(key)?;
        let (mut socket, _) = tokio_tungstenite::connect_async(&url).await.map_err(|e| match e {
            tokio_tungstenite::tungstenite::Error::Http(response)=>Error::Provider(format!("pending WS handshake: HTTP {}",response.status().as_u16())),
            _=>Error::Provider("pending WS connection/transport failure".into()),
        })?;
        use tokio_tungstenite::tungstenite::Message;
        socket.send(Message::Text(json!({"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}).to_string().into())).await.map_err(|_| Error::Provider("WS send".into()))?;
        let reply = tokio::time::timeout(std::time::Duration::from_secs(10), socket.next()).await
            .map_err(|_| Error::Provider("WS chain probe timeout".into()))?.ok_or_else(|| Error::Provider("WS closed".into()))?
            .map_err(|_| Error::Provider("WS receive".into()))?;
        let probe: Value = serde_json::from_str(reply.to_text().map_err(|_| Error::Invalid("WS probe".into()))?)?;
        if !probe["error"].is_null(){return Err(Error::Provider(format!("pending WS chain probe: RPC code {}",probe["error"]["code"])));}
        if quantity(&probe["result"])? != self.config.id { return Err(Error::Provider("WS chain mismatch".into())); }
        socket.send(Message::Text(json!({"jsonrpc":"2.0","id":2,"method":"eth_subscribe","params":["newPendingTransactions"]}).to_string().into())).await.map_err(|_| Error::Provider("WS subscription".into()))?;
        loop {
            let message = tokio::select! { _ = cancel.cancelled() => return Ok(()), m = socket.next() => m };
            let message = message.ok_or_else(|| Error::Provider("pending disconnect; ephemeral gap".into()))?.map_err(|_| Error::Provider("pending receive".into()))?;
            if message.len() > 1024 * 1024 { return Err(Error::Limit("pending message".into())); }
            if let Message::Ping(p) = message { socket.send(Message::Pong(p)).await.map_err(|_| Error::Provider("pong".into()))?; continue; }
            if !message.is_text() { continue; }
            let v: Value = serde_json::from_str(message.to_text().map_err(|_| Error::Invalid("pending frame".into()))?)?;
            if !v["error"].is_null() { return Err(Error::Unsupported(format!("pending subscription rejected: RPC code {}",v["error"]["code"]))); }
            let Some(result) = v.pointer("/params/result") else { continue; };
            let raw = if result.is_string() { self.pool.request("eth_getTransactionByHash", json!([result])).await? } else { result.clone() };
            if raw.is_null() { tracing::debug!(chain_id=self.config.id,"pending hydration gap"); continue; }
            let o = normalize(self.config.id, raw, None, Stage::Pending, "pending_ws")?;
            tx.send(o).await.map_err(|_| Error::Dependency("pending consumer stopped".into()))?;
        }
    }
}

fn validate_receipt(receipt:&Value,tx:&Value,hash:B256,number:u64,index:usize)->AppResult<()>{
    if receipt["blockHash"]!=json!(hash)||receipt["transactionHash"]!=tx["hash"]
        || quantity(&receipt["blockNumber"])?!=number||quantity(&receipt["transactionIndex"])?!=index as u64 {
        return Err(Error::Provider("receipt canonical position mismatch".into()));
    }
    Ok(())
}
