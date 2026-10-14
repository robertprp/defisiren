use alloy_primitives::{B256, Bytes, keccak256};
use async_trait::async_trait;
use chrono::{DateTime,Utc};
use serde::{Deserialize,Serialize};
use serde_json::{Value,json};
use siren_lib::{entity::*,AppResult,Error};
use std::{sync::{Arc,atomic::{AtomicBool,Ordering}},time::Duration};
use crate::{chain::provider::ProviderPool,simulation::{ReplayJob,isolated}};

#[derive(Clone,Serialize,Deserialize)]
pub struct ProtectionProof {pub action_digest:B256,pub state_hash:B256,pub attack_harm_observed:bool,pub protected_invariants_hold:bool,pub simulation:SimulationResult}
pub fn digest(action:&Action)->AppResult<B256> {
    let mut bytes=b"chain-siren/authorized-action/v1\0".to_vec();bytes.extend(serde_json::to_vec(action)?);Ok(keccak256(bytes))
}
pub fn validate(policy:&ResponsePolicy,action:&Action,now:DateTime<Utc>,current_hash:B256,authority:&[u8],proof:&ProtectionProof,kill:bool)->AppResult<B256> {
    let reject=|s:&str|Error::Denied(s.into());
    if kill{return Err(reject("emergency kill switch"));}
    if !policy.enabled || action.policy_id!=policy.id || action.policy_version!=policy.version{return Err(reject("policy inactive or version mismatch"));}
    if action.chain_id!=policy.chain_id || action.target!=policy.target || action.signer!=policy.signer{return Err(reject("chain/target/signer mismatch"));}
    if action.calldata.len()<4 || action.calldata[..4]!=policy.selector || keccak256(&action.calldata)!=policy.calldata_hash{return Err(reject("unreviewed calldata"));}
    if action.destination!=policy.recovery_destination{return Err(reject("recovery destination mismatch"));}
    if action.value>policy.max_value || action.gas>policy.max_gas || action.max_fee_per_gas>policy.max_fee_per_gas{return Err(reject("amount/gas/fee ceiling"));}
    if policy.expires_at<=now || action.expires_at<=now || action.expires_at>policy.expires_at{return Err(reject("expiration"));}
    let age=(now-action.state_time).num_seconds();
    if age<0 || age as u64>policy.max_state_age_seconds || current_hash!=action.state_hash{return Err(reject("stale state"));}
    if authority!=policy.authority_result.as_ref() || authority.is_empty(){return Err(reject("role/ownership verification"));}
    let d=digest(action)?;
    if proof.action_digest!=d || proof.state_hash!=action.state_hash || !proof.attack_harm_observed || !proof.protected_invariants_hold {
        return Err(reject("missing protection proof"));
    }
    let context=proof.simulation.state.as_ref().ok_or_else(||reject("simulation state context missing"))?;
    if context.chain_id!=action.chain_id || context.block_hash!=action.state_hash {return Err(reject("simulation state context mismatch"));}
    if !proof.simulation.unresolved.is_empty() || matches!(proof.simulation.status,SimulationStatus::Timeout|SimulationStatus::Unsupported|SimulationStatus::Inconclusive|SimulationStatus::NeedsDependency) {
        return Err(reject("simulation fidelity insufficient"));
    }
    Ok(d)
}
