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
pub struct KillSwitch(pub AtomicBool);
impl Default for KillSwitch {fn default()->Self{Self(AtomicBool::new(true))}}
impl KillSwitch {pub fn active(&self)->bool{self.0.load(Ordering::SeqCst)}}
#[async_trait]
pub trait Signer:Send+Sync {async fn sign(&self,action:&Action,approved_digest:B256)->AppResult<Bytes>;}
/// Signer deployment must independently validate the digest and policy. Monitoring never holds its key.
pub struct RemoteSigner {pub endpoint:String,pub token_env:String,pub client:reqwest::Client}
#[async_trait]
impl Signer for RemoteSigner {
    async fn sign(&self,action:&Action,approved_digest:B256)->AppResult<Bytes> {
        if digest(action)?!=approved_digest{return Err(Error::Denied("digest mismatch".into()));}
        if !self.endpoint.starts_with("https://"){return Err(Error::Denied("remote signer requires TLS".into()));}
        let token=crate::config::Config::secret(&self.token_env)?;
        let response=self.client.post(&self.endpoint).bearer_auth(token).timeout(Duration::from_secs(5))
            .json(&json!({"action":action,"approved_digest":approved_digest})).send().await.map_err(|_|Error::Dependency("remote signer".into()))?;
        if !response.status().is_success(){return Err(Error::Denied("signer rejected action".into()));}
        if response.content_length().is_none_or(|n|n>32768){return Err(Error::Limit("signer response".into()));}
        let v:Value=response.json().await.map_err(|_|Error::Dependency("signer schema".into()))?;
        if v["approved_digest"]!=json!(approved_digest){return Err(Error::Denied("signer response digest".into()));}
        Ok(serde_json::from_value(v["raw_transaction"].clone())?)
    }
}
#[async_trait]
pub trait SubmissionBackend:Send+Sync {async fn submit(&self,raw:&Bytes,max_block:u64)->AppResult<B256>;}
pub struct RpcSubmission {pub pool:Arc<ProviderPool>}
#[async_trait]
impl SubmissionBackend for RpcSubmission {
    async fn submit(&self,raw:&Bytes,_max_block:u64)->AppResult<B256> {
        if self.pool.chain_id==1{return Err(Error::Denied("Ethereum private policy has no public fallback".into()));}
        Ok(serde_json::from_value(self.pool.request("eth_sendRawTransaction",json!([raw])).await?)?)
    }
}
/// Authentication is computed for each exact JSON body; a static signature is invalid.
#[async_trait]
pub trait RelayAuthenticator:Send+Sync {async fn header(&self,body:&[u8])->AppResult<String>;}
pub struct RemoteRelayAuthenticator {pub endpoint:String,pub token_env:String,pub client:reqwest::Client}
#[async_trait]
impl RelayAuthenticator for RemoteRelayAuthenticator {
    async fn header(&self,body:&[u8])->AppResult<String>{
        if !self.endpoint.starts_with("https://"){return Err(Error::Denied("relay authenticator TLS".into()));}
        let hash=keccak256(body);let token=crate::config::Config::secret(&self.token_env)?;
        let r=self.client.post(&self.endpoint).bearer_auth(token).timeout(Duration::from_secs(5))
            .json(&json!({"body_keccak256":hash,"scheme":"flashbots-eip191-hex-body-hash"})).send().await.map_err(|_|Error::Dependency("relay auth".into()))?;
        if !r.status().is_success()||r.content_length().is_none_or(|n|n>4096){return Err(Error::Dependency("relay auth response".into()));}
        let v:Value=r.json().await.map_err(|_|Error::Dependency("relay auth JSON".into()))?;
        if v["body_keccak256"]!=json!(hash){return Err(Error::Denied("relay auth hash mismatch".into()));}
        v["header"].as_str().filter(|h|h.len()<256&&h.contains(':')).map(str::to_owned).ok_or_else(||Error::Denied("relay auth header".into()))
    }
}
/// Flashbots eth_sendPrivateTransaction. No public fallback and no ordering guarantee.
pub struct FlashbotsSubmission {pub endpoint:String,pub authenticator:Arc<dyn RelayAuthenticator>,pub client:reqwest::Client}
#[async_trait]
impl SubmissionBackend for FlashbotsSubmission {
    async fn submit(&self,raw:&Bytes,max_block:u64)->AppResult<B256> {
        if !self.endpoint.starts_with("https://"){return Err(Error::Denied("private submission TLS".into()));}
        let body=serde_json::to_vec(&json!({"jsonrpc":"2.0","id":1,"method":"eth_sendPrivateTransaction","params":[{"tx":raw,"maxBlockNumber":format!("0x{max_block:x}"),"preferences":{"fast":false}}]}))?;
        let header=self.authenticator.header(&body).await?;
        let r=self.client.post(&self.endpoint).header("X-Flashbots-Signature",header).header("Content-Type","application/json")
            .timeout(Duration::from_secs(5)).body(body).send().await.map_err(|_|Error::Provider("private submission transport; do not public-fallback".into()))?;
        if !r.status().is_success()||r.content_length().is_some_and(|n|n>32768){return Err(Error::Provider("private endpoint rejected request".into()));}
        let v:Value=r.json().await.map_err(|_|Error::Provider("private response".into()))?;
        if !v["error"].is_null(){return Err(Error::Provider("private submission rejected".into()));}
        Ok(serde_json::from_value(v["result"].clone())?)
    }
}
