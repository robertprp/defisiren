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
