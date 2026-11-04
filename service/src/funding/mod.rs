use crate::{
    config::FundingConfig,
    contracts::{topic, word_address, Pool},
};
use alloy_primitives::{Address, U256};
use chrono::Duration;
use serde::{Deserialize, Serialize};
use siren_lib::{entity::*, AppResult, Error};
use std::collections::{BTreeMap, BTreeSet, HashSet};

/// No recipient inference from transaction sender or a call to a mixer.
pub fn withdrawals(o: &Observation, pools: &[Pool]) -> AppResult<Vec<FundingEdge>> {
    let mut edges = Vec::new();
    if o.success != Some(true) || !o.stage.canonical() {
        return Ok(edges);
    }
    for log in &o.logs {
        let Some(pool) = pools.iter().find(|p| {
            p.chain_id == o.chain_id
                && p.address == log.address
                && o.position
                    .block_number
                    .is_some_and(|b| b >= p.deployment_block)
        }) else {
            continue;
        };
        if log.removed
            || log.topics.first() != Some(&topic("Withdrawal(address,bytes32,address,uint256)"))
        {
            continue;
        }
        if log.topics.len() != 2 || log.data.len() != 96 {
            return Err(Error::Invalid("Withdrawal ABI".into()));
        }
        let recipient = word_address(&log.data[..32])?;
        let relayer = word_address(log.topics[1].as_slice())?;
        let fee = U256::from_be_slice(&log.data[64..]);
        let amount = pool
            .denomination
            .checked_sub(fee)
            .ok_or_else(|| Error::Invalid("withdrawal fee".into()))?;
        edges.push(FundingEdge {
            id: format!("{}:{}:{}", o.chain_id, o.hash, log.index),
            chain_id: o.chain_id,
            from: pool.address,
            to: recipient,
            asset: pool.asset,
            amount: amount.to_string(),
            fee: Some(fee.to_string()),
            relayer: Some(relayer),
            transaction: o.hash,
            log_index: Some(log.index),
            block_hash: o.position.block_hash,
            observed_at: o.observed_at,
            provisional: false,
            bridge_origin: None,
            hop: 0,
            confidence: 10000,
        });
    }
    Ok(edges)
}

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct WatchState {
    pub profiles: BTreeMap<Address, Profile>,
    pub fanout: BTreeMap<Address, Vec<Address>>,
    #[serde(default)]
    pub derived_only: BTreeSet<Address>,
}

impl WatchState {
    pub fn expire(&mut self, now: chrono::DateTime<chrono::Utc>) {
        self.profiles.retain(|_, p| p.watch_until >= now);
        self.derived_only.retain(|a| self.profiles.contains_key(a));
        self.fanout.retain(|a, _| self.profiles.contains_key(a));
    }

    pub fn add(&mut self, edge: FundingEdge, cfg: &FundingConfig) -> AppResult<()> {
        if !self.profiles.contains_key(&edge.to) && self.profiles.len() >= cfg.max_profiles {
            return Err(Error::Limit(
                "watch profiles; evidence retained in journal".into(),
            ));
        }
        if edge.bridge_origin.is_some() && !self.profiles.contains_key(&edge.to) {
            self.derived_only.insert(edge.to);
        }
        if edge.bridge_origin.is_none() {
            self.derived_only.remove(&edge.to);
        }
        let profile = self.profiles.entry(edge.to).or_insert_with(|| Profile {
            chain_id: edge.chain_id,
            address: edge.to,
            first_observed: edge.observed_at,
            last_activity: edge.observed_at,
            history_start: None,
            history_complete: false,
            nonce: None,
            nonce_state: None,
            code_status: "unknown".into(),
            freshness_confidence: 0,
            watch_until: edge.observed_at + Duration::seconds(cfg.watch_seconds),
            funding: vec![],
            deployments: vec![],
        });
        if profile.chain_id != edge.chain_id {
            return Err(Error::Invalid("cross-chain watch state".into()));
        }
        profile.watch_until = profile
            .watch_until
            .max(edge.observed_at + Duration::seconds(cfg.watch_seconds));
        if !profile.funding.iter().any(|f| f.id == edge.id) {
            if profile.funding.len() >= 64 {
                profile.funding.remove(0);
            }
            profile.funding.push(edge);
        }
        Ok(())
    }

    pub fn propagate(
        &mut self,
        o: &Observation,
        cfg: &FundingConfig,
        shared: &[Address],
    ) -> AppResult<Vec<FundingEdge>> {
        if o.success != Some(true) || !o.stage.canonical() {
            return Ok(vec![]);
        }
        let shared: HashSet<_> = shared.iter().copied().collect();
        let mut transfers = Vec::new();
        if let Some(to) = o.to {
            transfers.push((o.from, to, o.value, None));
        }
        // Native call values only; ERC20 event-only transfers need state reconciliation before propagation.
        for c in &o.calls {
            if c.error.is_none() && c.depth > 0 && c.kind == "CALL" {
                if let Some(to) = c.to {
                    transfers.push((c.from, to, c.value, None));
                }
            }
        }
        let mut edges = Vec::new();
        for (index, (from, to, amount, asset)) in transfers.into_iter().enumerate() {
            if amount < cfg.minimum_native
                || from == to
                || shared.contains(&from)
                || shared.contains(&to)
            {
                continue;
            }
            let Some(profile) = self.profiles.get(&from) else {
                continue;
            };
            let Some(parent) = profile
                .funding
                .iter()
                .filter(|f| {
                    !f.provisional
                        && f.hop < cfg.max_hops
                        && (o.observed_at - f.observed_at).num_seconds() >= 0
                        && (o.observed_at - f.observed_at).num_seconds() <= cfg.recent_seconds
                })
                .max_by_key(|f| f.confidence)
            else {
                continue;
            };
            let fan = self.fanout.entry(from).or_default();
            if fan.len() >= cfg.max_fanout && !fan.contains(&to) {
                continue;
            }
            if !fan.contains(&to) {
                fan.push(to);
            }
            edges.push(FundingEdge {
                id: format!("{}:{}:native:{}", o.chain_id, o.hash, index),
                chain_id: o.chain_id,
                from,
                to,
                asset,
                amount: amount.to_string(),
                fee: None,
                relayer: None,
                transaction: o.hash,
                log_index: None,
                block_hash: o.position.block_hash,
                observed_at: o.observed_at,
                provisional: false,
                bridge_origin: parent.bridge_origin.clone(),
                hop: parent.hop + 1,
                confidence: ((parent.confidence as u32) * 7 / 10) as u16,
            });
        }
        for e in &edges {
            self.add(e.clone(), cfg)?;
        }
        Ok(edges)
    }
}

pub mod bridge;
