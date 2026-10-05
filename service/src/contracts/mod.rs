use alloy_primitives::{Address, B256, U256, keccak256};
use serde::{Deserialize, Serialize};
use siren_lib::{AppResult, Error};
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pool {
    pub chain_id: u64, pub address: Address, pub deployment_block: u64,
    pub abi_version: String, pub asset: Option<Address>, pub denomination: U256,
    pub provenance_url: String,
}
pub fn pools() -> AppResult<Vec<Pool>> { Ok(serde_json::from_str(include_str!("../../../config/tornado.v1.json"))?) }
pub fn topic(signature: &str) -> B256 { keccak256(signature.as_bytes()) }
pub fn word_address(word: &[u8]) -> AppResult<Address> {
    if word.len() != 32 || word[..12].iter().any(|x| *x != 0) { return Err(Error::Invalid("noncanonical ABI address".into())); }
    Ok(Address::from_slice(&word[12..]))
}
