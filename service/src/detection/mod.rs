use crate::{
    analysis,
    config::Config,
    contracts::{self, Pool},
    funding::{self, WatchState},
};
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
    #[serde(default)]
    pub last_pruned_minute: i64,
}

pub fn candidate(o: &Observation, cfg: &Config) -> bool {
    o.to.is_none()
        || o.created.is_some()
        || o.calls.iter().any(|c| c.kind.starts_with("CREATE"))
        || cfg.protected.iter().any(|p| {
            p.chain_id == o.chain_id
                && (o.to == Some(p.address)
                    || o.logs.iter().any(|l| l.address == p.address)
                    || o.calls
                        .iter()
                        .any(|c| c.to == Some(p.address) || c.from == p.address))
        })
        || o.logs.iter().any(|l| {
            l.topics
                .first()
                .is_some_and(|t| privilege_topics().contains(t))
        })
}

/// Recognized wire formats, not a claim of local EVM execution support.
/// Sources: OP deposit spec, Arbitrum RPC types, and bnb-chain/bsc core/types.
pub fn known_envelope(chain_id: u64, tx_type: u8) -> bool {
    match chain_id {
        1 | 56 => tx_type <= 4,
        8453 => matches!(tx_type, 0 | 1 | 2 | 4 | 126),
        42161 => matches!(tx_type, 0 | 1 | 2 | 4 | 100 | 101 | 102 | 104 | 105 | 106),
        _ => tx_type <= 2,
    }
}

fn privilege_topics() -> &'static [B256] {
    static TOPICS: std::sync::OnceLock<Vec<B256>> = std::sync::OnceLock::new();
    TOPICS.get_or_init(|| {
        [
            "Upgraded(address)",
            "AdminChanged(address,address)",
            "OwnershipTransferred(address,address)",
            "RoleGranted(bytes32,address,address)",
            "Initialized(uint8)",
            "Initialized(uint64)",
        ]
        .iter()
        .map(|s| contracts::topic(s))
        .collect()
    })
}

fn evidence(o: &Observation, kind: &str, description: &str, data: serde_json::Value) -> Evidence {
    Evidence {
        id: format!("{}:{}:{}", o.chain_id, o.hash, kind),
        kind: kind.into(),
        description: description.into(),
        transaction: Some(o.hash),
        data,
    }
}

pub fn finding(o: &Observation, kind: &str, ev: Vec<Evidence>, points: u16) -> Finding {
    let incident_id = Uuid::new_v5(
        &Uuid::NAMESPACE_OID,
        format!("{}:{}:{}", o.chain_id, o.hash, kind).as_bytes(),
    );
    Finding {
        schema_version: "1.0".into(),
        event_id: Uuid::nil(),
        incident_id,
        revision: 0,
        cursor: 0,
        chain_id: o.chain_id,
        source: o.source.clone(),
        observed_at: o.observed_at,
        position: o.position.clone(),
        ordering: o.ordering,
        finality: o.stage,
        finding_type: kind.into(),
        severity: if points >= 70 {
            "high"
        } else if points >= 40 {
            "medium"
        } else {
            "informational"
        }
        .into(),
        confidence: if points >= 70 { 7000 } else { 4000 },
        addresses: vec![o.from],
        suspected_victims: vec![],
        observed_victims: vec![],
        components: vec![ScoreComponent {
            rule: kind.into(),
            version: "1".into(),
            points,
            evidence_ids: ev.iter().map(|e| e.id.clone()).collect(),
        }],
        evidence: ev,
        detector_version: "rules/1".into(),
        simulation: SimulationResult::unavailable("not simulated"),
        asset_deltas: vec![],
        enrichment_complete: false,
        reviewed: false,
        retracted: false,
    }
}

impl Engine {
    pub fn process(
        &mut self,
        o: &Observation,
        cfg: &Config,
        pools: &[Pool],
    ) -> AppResult<Vec<Finding>> {
        if self.last_pruned_minute != o.observed_at.timestamp() / 60 {
            self.watch.expire(o.observed_at);
            let active: HashSet<_> = self
                .watch
                .profiles
                .values()
                .flat_map(|p| p.deployments.iter().copied())
                .chain(self.watch.profiles.keys().copied())
                .collect();
            self.deployment_incidents.retain(|a, _| active.contains(a));
            self.last_pruned_minute = o.observed_at.timestamp() / 60;
        }
        let mut findings = vec![];
        if self
            .watch
            .profiles
            .get(&o.from)
            .is_some_and(|p| p.funding.is_empty())
        {
            findings.push(finding(
                o,
                "watched_activity",
                vec![evidence(
                    o,
                    "operator_watch",
                    "Operator-enrolled wallet activity; no maliciousness inferred.",
                    json!({"sender":o.from}),
                )],
                0,
            ));
        }
        for edge in funding::withdrawals(o, pools)? {
            self.watch.add(edge.clone(), &cfg.funding)?;
            let ev = evidence(o, "withdrawal", "Successful registered pool Withdrawal; privacy use is not proof of abuse. Refund is unresolved unless traced.", json!(edge));
            let mut f = finding(o, "funding_watch", vec![ev], 0);
            f.addresses = vec![edge.to, edge.from];
            f.confidence = 10000;
            findings.push(f);
        }
        for edge in self
            .watch
            .propagate(o, &cfg.funding, &cfg.shared_accounts)?
        {
            let ev = evidence(
                o,
                "transfer_provenance",
                "Verified native transfer; fungible units cannot be uniquely attributed.",
                json!(edge),
            );
            let mut f = finding(o, "funding_watch", vec![ev], 0);
            f.addresses = vec![edge.to, edge.from];
            findings.push(f);
        }
        let mut created = Vec::new();
        if let Some(a) = o.created {
            created.push(a);
        }
        for c in &o.calls {
            if c.kind.starts_with("CREATE") && c.error.is_none() {
                if let Some(a) = c.to {
                    created.push(a);
                }
            }
        }
        let recently_funded = self.watch.profiles.get(&o.from).is_some_and(|p| {
            p.funding.iter().any(|f| {
                !f.provisional
                    && (o.observed_at - f.observed_at).num_seconds() >= 0
                    && o.observed_at - f.observed_at
                        <= Duration::seconds(cfg.funding.recent_seconds)
            })
        });
        if let Some(p) = self.watch.profiles.get_mut(&o.from) {
            p.last_activity = o.observed_at;
            p.nonce = Some(o.nonce);
            p.nonce_state = o.position.block_hash;
            // Partial history supports only weak freshness inference, never a proof of wallet creation.
            p.freshness_confidence = if o.nonce <= cfg.funding.max_pre_nonce {
                3000
            } else {
                0
            };
            for a in &created {
                if !p.deployments.contains(a) && p.deployments.len() < 128 {
                    p.deployments.push(*a);
                }
            }
        }
        if o.to.is_none() || !created.is_empty() {
            let a = analysis::analyze(&o.input, cfg.max_input_bytes)?;
            let fingerprint = cfg.known_bad_hashes.contains(&a.exact_hash);
            if recently_funded || fingerprint {
                let points = if fingerprint { 70 } else { 30 };
                let ev = evidence(o, "creation", "Initcode heuristic analysis; embedded addresses are candidates until execution corroborates them.", json!({"analysis":a,"created":created,"recent_funding":recently_funded,"known_exact_fingerprint":fingerprint}));
                let f = finding(o, "suspicious_deployment", vec![ev], points);
                for address in &created {
                    self.deployment_incidents.insert(*address, f.incident_id);
                }
                if recently_funded {
                    self.deployment_incidents.insert(o.from, f.incident_id);
                }
                findings.push(f);
            }
        }
        let privilege = privilege_topics();
        for p in cfg.protected.iter().filter(|p| p.chain_id == o.chain_id) {
            let interacts =
                o.to == Some(p.address) || o.calls.iter().any(|c| c.to == Some(p.address));
            let related =
                o.to.and_then(|a| self.deployment_incidents.get(&a))
                    .or_else(|| self.deployment_incidents.get(&o.from))
                    .copied();
            if interacts && (recently_funded || related.is_some()) {
                let ev = evidence(o, "sequence", "Watched deployment/funding followed by protected-protocol interaction; impact requires reconstruction.", json!({"protocol":p.name,"address":p.address}));
                let mut f = finding(o, "sequence_continuation", vec![ev], 45);
                if let Some(id) = related {
                    f.incident_id = id;
                }
                f.suspected_victims.push(p.address);
                findings.push(f);
            }
            for l in o
                .logs
                .iter()
                .filter(|l| !l.removed && l.address == p.address && o.success == Some(true))
            {
                if l.topics.first().is_some_and(|t| privilege.contains(t)) {
                    let allowed = p.authorized_admins.contains(&o.from);
                    let ev = evidence(o, "privilege_event", "Observed privilege event; emitted event is not independent proof of changed storage.", json!({"log":l,"known_admin_sender":allowed}));
                    let mut f = finding(
                        o,
                        "privilege_change",
                        vec![ev],
                        if allowed { 20 } else { 50 },
                    );
                    f.addresses.push(p.address);
                    findings.push(f);
                }
                if p.governance
                    && l.topics.first().is_some_and(|t| {
                        *t == contracts::topic("ProposalExecuted(uint256)")
                            || *t
                                == contracts::topic(
                                    "CallExecuted(bytes32,uint256,address,uint256,bytes)",
                                )
                    })
                {
                    findings.push(finding(o, "governance_execution", vec![evidence(o, "governance", "Supported Governor/Timelock execution event; authorization and proposal intent unresolved.", json!(l))], 20));
                }
            }
            if o.success == Some(true) && o.trace_complete {
                let mut sent = U256::ZERO;
                let mut received = U256::ZERO;
                let mut visits = 0;
                for c in &o.calls {
                    if c.error.is_some()
                        || !matches!(
                            c.kind.as_str(),
                            "CALL" | "CREATE" | "CREATE2" | "SELFDESTRUCT"
                        )
                    {
                        continue;
                    }
                    if c.from == p.address {
                        sent = sent.saturating_add(c.value);
                    }
                    if c.to == Some(p.address) {
                        received = received.saturating_add(c.value);
                        visits += 1;
                    }
                }
                let net_out = sent.saturating_sub(received);
                if net_out > p.native_outflow_limit {
                    let ev = evidence(o, "net_native_outflow", "Net successful call-value outflow exceeds configured budget. Fees/state reconciliation remain separate; this is not proof of theft.", json!({"sent":sent.to_string(),"received":received.to_string(),"net_out":net_out.to_string(),"limit":p.native_outflow_limit.to_string(),"callbacks":visits}));
                    let mut f = finding(
                        o,
                        "unusual_outflow",
                        vec![ev],
                        if visits > 1 { 75 } else { 65 },
                    );
                    f.suspected_victims.push(p.address);
                    f.asset_deltas.push(AssetDelta {
                        address: p.address,
                        asset: None,
                        net: Some(format!("-{net_out}")),
                        basis: "successful_call_values".into(),
                        confidence: 6000,
                        price: None,
                    });
                    findings.push(f);
                }
            }
        }
        let known = known_envelope(o.chain_id, o.tx_type);
        if !known {
            findings.push(finding(
                o,
                "unsupported_envelope",
                vec![evidence(
                    o,
                    "envelope",
                    "Envelope retained; execution-family support is not assumed.",
                    json!({"type":o.tx_type}),
                )],
                0,
            ));
        }
        Ok(findings)
    }
}
