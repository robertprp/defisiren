use crate::{analysis, config::Config, contracts::{self, Pool}, funding::{self, WatchState}};
use alloy_primitives::{Address, B256, U256};
use chrono::Duration;
use serde::{Deserialize, Serialize};
use serde_json::json;
use siren_lib::{entity::*, AppResult};
use std::collections::{BTreeMap, HashSet};
use uuid::Uuid;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Engine {
    pub watch: WatchState,
    pub deployment_incidents: BTreeMap<Address, Uuid>,
    #[serde(default)] pub last_pruned_minute: i64,
}
pub fn candidate(o: &Observation, cfg: &Config) -> bool {
    o.to.is_none() || o.created.is_some() || o.calls.iter().any(|c| c.kind.starts_with("CREATE"))
        || cfg.protected.iter().any(|p| p.chain_id == o.chain_id && (o.to == Some(p.address)
            || o.logs.iter().any(|l| l.address == p.address) || o.calls.iter().any(|c| c.to == Some(p.address) || c.from == p.address)))
        || o.logs.iter().any(|l| l.topics.first().is_some_and(|t| privilege_topics().contains(t)))
}
fn privilege_topics() -> &'static [B256] {
    static TOPICS: std::sync::OnceLock<Vec<B256>> = std::sync::OnceLock::new();
    TOPICS.get_or_init(|| ["Upgraded(address)", "AdminChanged(address,address)", "OwnershipTransferred(address,address)",
        "RoleGranted(bytes32,address,address)", "Initialized(uint8)", "Initialized(uint64)"].iter().map(|s| contracts::topic(s)).collect())
}
fn evidence(o: &Observation, kind: &str, description: &str, data: serde_json::Value) -> Evidence {
    Evidence { id: format!("{}:{}:{}", o.chain_id, o.hash, kind), kind: kind.into(), description: description.into(), transaction: Some(o.hash), data }
}
pub fn finding(o: &Observation, kind: &str, ev: Vec<Evidence>, points: u16) -> Finding {
    let incident_id = Uuid::new_v5(&Uuid::NAMESPACE_OID, format!("{}:{}:{}", o.chain_id, o.hash, kind).as_bytes());
    Finding { schema_version: "1.0".into(), event_id: Uuid::nil(), incident_id, revision: 0, cursor: 0,
        chain_id: o.chain_id, source: o.source.clone(), observed_at: o.observed_at, position: o.position.clone(),
        ordering: o.ordering, finality: o.stage, finding_type: kind.into(),
        severity: if points >= 70 { "high" } else if points >= 40 { "medium" } else { "informational" }.into(),
        confidence: if points >= 70 { 7000 } else { 4000 }, addresses: vec![o.from], suspected_victims: vec![], observed_victims: vec![],
        components: vec![ScoreComponent { rule: kind.into(), version: "1".into(), points, evidence_ids: ev.iter().map(|e| e.id.clone()).collect() }],
        evidence: ev, detector_version: "rules/1".into(), simulation: SimulationResult::unavailable("not simulated"),
        asset_deltas: vec![], enrichment_complete: false, reviewed: false, retracted: false }
}
