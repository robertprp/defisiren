use alloy_primitives::{Address, B256, Bytes, U256, keccak256};
use async_trait::async_trait;
use revm::{Database, DatabaseCommit, Evm, primitives::{Account, AccountInfo, Bytecode, SpecId, TxKind}};
use serde::{Deserialize, Serialize};
use serde_json::json;
use siren_lib::{entity::*, AppResult, Error};
use std::{collections::BTreeMap, time::{Duration, Instant}};
use crate::chain::{RpcAdapter, TraceProvider};

#[async_trait]
pub trait SimulationBackend: Send + Sync {
    async fn simulate(&self, observation: &Observation) -> AppResult<SimulationResult>;
}
#[async_trait]
impl SimulationBackend for RpcAdapter {
    async fn simulate(&self, o: &Observation) -> AppResult<SimulationResult> {
        let (Some(height), Some(hash), Some(index)) = (o.position.block_number,o.position.block_hash,o.position.transaction_index) else {
            return Ok(SimulationResult::unavailable("speculative chain-native execution unavailable; no future-state claim"));
        };
        if !o.stage.canonical() { return Ok(SimulationResult::unavailable("exact included context required")); }
        let start = Instant::now();
        let trace = self.trace(o.hash,height,hash).await?;
        let diff = self.pool.anchored("debug_traceTransaction",json!([o.hash,{"tracer":"prestateTracer","timeout":"5s","tracerConfig":{"diffMode":true}}]),height,hash).await;
        let mut result = SimulationResult {
            status: if trace["error"].is_null() { SimulationStatus::Success } else { SimulationStatus::Revert },
            fidelity: format!("{}/native-client-included-trace; not independent differential validation",self.config.family),
            state: Some(StateReference { chain_id:o.chain_id,block_hash:hash,transaction_index:Some(index),overlay_id:"native-exact-transaction".into(),execution_config:self.config.family.clone() }),
            assumptions: vec!["Native client reconstructs exact pre-transaction state and chain fork rules.".into()],
            unresolved: vec![], gas_used: trace.get("gasUsed").and_then(|v| crate::chain::provider::quantity(v).ok()),
            elapsed_ms:start.elapsed().as_millis() as u64, evidence:vec![],deltas:vec![],
        };
        if let Ok(diff) = diff {
            if let (Some(pre),Some(post)) = (diff["pre"].as_object(),diff["post"].as_object()) {
                for (address,before) in pre {
                    let Some(after) = post.get(address) else { continue; };
                    let a = before.get("balance").and_then(|x| serde_json::from_value::<U256>(x.clone()).ok());
                    let b = after.get("balance").and_then(|x| serde_json::from_value::<U256>(x.clone()).ok());
                    if let (Some(a),Some(b),Ok(address)) = (a,b,address.parse::<Address>()) {
                        let net = if b>=a { (b-a).to_string() } else { format!("-{}",a-b) };
                        result.deltas.push(AssetDelta { address,asset:None,net:Some(net),basis:"native_prestate_diff_including_fees".into(),confidence:9000,price:None });
                    }
                }
            }
            result.evidence.push(Evidence {id:format!("{}:state-diff",o.hash),kind:"state_diff".into(),description:"Native pre/post state; omitted accounts/fields are not synthesized.".into(),transaction:Some(o.hash),data:diff});
        } else { result.unresolved.push("state diff unavailable; token and native reconciliation incomplete".into()); }
        result.evidence.push(Evidence { id:format!("{}:native-trace",o.hash),kind:"call_tree".into(),description:"Observed native execution, not a predicted ordering.".into(),transaction:Some(o.hash),data:trace });
        Ok(result)
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct SnapshotAccount {
    pub balance: U256, pub nonce:u64, pub code:Bytes,
    pub storage:BTreeMap<U256,U256>, pub storage_complete:bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub state:StateReference, pub number:u64, pub timestamp:u64, pub basefee:U256,
    pub coinbase:Address, pub gas_limit:u64,
    pub accounts:BTreeMap<Address,Option<SnapshotAccount>>, pub block_hashes:BTreeMap<u64,B256>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ReplayTx { pub caller:Address,pub to:Option<Address>,pub data:Bytes,pub value:U256,pub nonce:u64,pub gas:u64,pub gas_price:U256 }
#[derive(Clone, Serialize, Deserialize)]
pub struct ReplayJob { pub snapshot:Snapshot, pub transactions:Vec<ReplayTx>, pub synthetic_fixture:bool }
struct StrictDb { snapshot:Snapshot, reads:usize, max_reads:usize }
impl StrictDb {
    fn read(&mut self) -> AppResult<()> { self.reads+=1; if self.reads>self.max_reads { return Err(Error::Limit("state reads".into())); } Ok(()) }
}
impl Database for StrictDb {
    type Error=Error;
    fn basic(&mut self,address:Address)->AppResult<Option<AccountInfo>> {
        self.read()?;
        let a=self.snapshot.accounts.get(&address).ok_or_else(||Error::Dependency(format!("account {address}")))?;
        Ok(a.as_ref().map(|a|AccountInfo { balance:a.balance,nonce:a.nonce,code_hash:keccak256(&a.code),code:Some(Bytecode::new_raw(a.code.clone())) }))
    }
    fn code_by_hash(&mut self,hash:B256)->AppResult<Bytecode> {
        self.read()?;
        self.snapshot.accounts.values().flatten().find(|a|keccak256(&a.code)==hash).map(|a|Bytecode::new_raw(a.code.clone()))
            .ok_or_else(||Error::Dependency(format!("code {hash}")))
    }
    fn storage(&mut self,address:Address,index:U256)->AppResult<U256> {
        self.read()?;
        let a=self.snapshot.accounts.get(&address).ok_or_else(||Error::Dependency("storage account absent from snapshot".into()))?;
        match a { None=>Ok(U256::ZERO),Some(a)=>match a.storage.get(&index) {
            Some(value)=>Ok(*value),None if a.storage_complete=>Ok(U256::ZERO),_=>Err(Error::Dependency(format!("storage {address}:{index}"))) } }
    }
    fn block_hash(&mut self,number:u64)->AppResult<B256> {
        self.read()?; self.snapshot.block_hashes.get(&number).copied().ok_or_else(||Error::Dependency(format!("block hash {number}")))
    }
}
impl DatabaseCommit for StrictDb {
    fn commit(&mut self,changes:revm::primitives::HashMap<Address,Account>) {
        for (address,account) in changes {
            if !account.is_touched() {continue;}
            if account.is_selfdestructed() {self.snapshot.accounts.insert(address,None);continue;}
            let created=account.is_created();
            let entry=self.snapshot.accounts.entry(address).or_insert(None);
            let fresh=entry.is_none();
            let a=entry.get_or_insert_with(||SnapshotAccount {balance:U256::ZERO,nonce:0,code:Bytes::new(),storage:BTreeMap::new(),storage_complete:true});
            a.balance=account.info.balance;a.nonce=account.info.nonce;
            if let Some(code)=account.info.code {a.code=code.original_bytes();}
            if created || fresh {a.storage.clear();a.storage_complete=true;}
            for (key,value) in account.storage {a.storage.insert(key,value.present_value());}
        }
    }
}
