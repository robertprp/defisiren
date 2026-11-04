pub mod journal;
use serde::{Deserialize, Serialize};
use siren_lib::entity::{BlockBatch, Observation, Ordering};
#[derive(Clone, Serialize, Deserialize)]
pub struct RawFeed {
    pub chain_id: u64,
    pub source: String,
    pub sequence: Option<u64>,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub ordering: Ordering,
    pub payload: serde_json::Value,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct FinalityPoint {
    pub number: u64,
    pub hash: alloy_primitives::B256,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ChainProgress {
    pub chain_id: u64,
    pub source: String,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub safe: Option<FinalityPoint>,
    pub finalized: Option<FinalityPoint>,
    pub pending_before: chrono::DateTime<chrono::Utc>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum Accepted {
    Block(BlockBatch),
    Observation(Observation),
    Feed(RawFeed),
    Progress(ChainProgress),
}
