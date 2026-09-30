use alloy_primitives::{Address, Bytes, B256, U256};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage { Pending, Preconfirmed, Included, Safe, Finalized, Reorged, Replaced, Expired }
impl Stage {
    pub fn canonical(self) -> bool { matches!(self, Self::Included | Self::Safe | Self::Finalized) }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ordering { Unknown, Candidate, Sequenced, BlockOrdered }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub block_number: Option<u64>,
    pub block_hash: Option<B256>,
    pub transaction_index: Option<u64>,
    pub log_index: Option<u64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Log {
    pub address: Address,
    pub topics: Vec<B256>,
    pub data: Bytes,
    pub index: u64,
    pub removed: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Call {
    pub kind: String,
    pub from: Address,
    pub to: Option<Address>,
    pub input: Bytes,
    pub value: U256,
    pub error: Option<String>,
    pub depth: u32,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub chain_id: u64,
    pub hash: B256,
    pub source: String,
    pub observed_at: DateTime<Utc>,
    pub stage: Stage,
    pub ordering: Ordering,
    pub position: Position,
    pub from: Address,
    pub to: Option<Address>,
    pub nonce: u64,
    pub tx_type: u8,
    pub input: Bytes,
    pub value: U256,
    pub success: Option<bool>,
    pub created: Option<Address>,
    pub logs: Vec<Log>,
    pub calls: Vec<Call>,
    pub trace_complete: bool,
    /// Exact provider envelope, including unsupported fields; never synthesized signatures.
    pub envelope: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockBatch {
    pub chain_id: u64,
    pub number: u64,
    pub hash: B256,
    pub parent_hash: B256,
    pub timestamp: u64,
    pub observations: Vec<Observation>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub kind: String,
    pub description: String,
    pub transaction: Option<B256>,
    pub data: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScoreComponent { pub rule: String, pub version: String, pub points: u16, pub evidence_ids: Vec<String> }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetDelta {
    pub address: Address,
    pub asset: Option<Address>,
    /// Signed decimal integer base units, null when unavailable.
    pub net: Option<String>,
    pub basis: String,
    pub confidence: u16,
    pub price: Option<Price>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Price { pub usd_decimal: String, pub source: String, pub observed_at: DateTime<Utc>, pub decimals: u8 }
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SimulationStatus { Success, Revert, Timeout, Inconclusive, Unsupported, NeedsDependency }
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateReference {
    pub chain_id: u64,
    pub block_hash: B256,
    pub transaction_index: Option<u64>,
    pub overlay_id: String,
    pub execution_config: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub status: SimulationStatus,
    pub fidelity: String,
    pub state: Option<StateReference>,
    pub assumptions: Vec<String>,
    pub unresolved: Vec<String>,
    pub gas_used: Option<u64>,
    pub elapsed_ms: u64,
    pub evidence: Vec<Evidence>,
    pub deltas: Vec<AssetDelta>,
}
impl SimulationResult {
    pub fn unavailable(reason: impl Into<String>) -> Self {
        Self { status: SimulationStatus::Unsupported, fidelity: "none".into(), state: None,
            assumptions: vec![], unresolved: vec![reason.into()], gas_used: None, elapsed_ms: 0, evidence: vec![], deltas: vec![] }
    }
}
