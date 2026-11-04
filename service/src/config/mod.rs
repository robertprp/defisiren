use alloy_primitives::{Address, B256, U256};
use serde::{Deserialize, Serialize};
use siren_lib::{entity::Mode, AppResult, Error};
use std::{collections::HashSet, path::Path};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub name: String,
    pub listen: String,
    pub database_env: String,
    pub data_dir: String,
    #[serde(default)]
    pub mode: Mode,
    pub journal_max_bytes: u64,
    pub max_input_bytes: usize,
    pub rpc_concurrency: usize,
    #[serde(default = "rpc_rate")]
    pub rpc_requests_per_second: u32,
    pub trace_candidates_per_block: usize,
    pub poll_ms: u64,
    #[serde(default = "pending_ttl")]
    pub pending_ttl_seconds: i64,
    pub shutdown_seconds: u64,
    pub max_reorg_depth: u64,
    pub api_requests_per_minute: u32,
    pub chains: Vec<ChainConfig>,
    #[serde(default)]
    pub protected: Vec<ProtectedProtocol>,
    #[serde(default)]
    pub known_bad_hashes: Vec<B256>,
    #[serde(default)]
    pub shared_accounts: Vec<Address>,
    #[serde(default)]
    pub funding: FundingConfig,
    pub telegram_token_env: Option<String>,
    pub telegram_chat_env: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ChainConfig {
    pub id: u64,
    pub family: String,
    pub rpc_env: Vec<String>,
    pub ws_env: Option<String>,
    pub preconfirmed_ws_env: Option<String>,
    pub sequencer_ws_env: Option<String>,
    pub start_block: u64,
    #[serde(default)]
    pub start_at_head: bool,
    pub pending: bool,
    pub trace: bool,
    pub finality_depth: u64,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProtectedProtocol {
    pub chain_id: u64,
    pub address: Address,
    pub name: String,
    pub native_outflow_limit: U256,
    #[serde(default)]
    pub authorized_admins: Vec<Address>,
    #[serde(default)]
    pub governance: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct FundingConfig {
    pub recent_seconds: i64,
    pub max_pre_nonce: u64,
    pub watch_seconds: i64,
    pub max_hops: u8,
    pub max_fanout: usize,
    pub minimum_native: U256,
    pub max_profiles: usize,
}
impl Default for FundingConfig {
    fn default() -> Self {
        Self {
            recent_seconds: 86400,
            max_pre_nonce: 3,
            watch_seconds: 604800,
            max_hops: 2,
            max_fanout: 16,
            minimum_native: U256::from(10_000_000_000_000_000u64),
            max_profiles: 100_000,
        }
    }
}
fn rpc_rate() -> u32 {
    5
}
fn pending_ttl() -> i64 {
    900
}
impl Config {
    pub async fn load(path: &Path) -> AppResult<Self> {
        let text = tokio::fs::read_to_string(path).await?;
        let cfg: Self =
            toml::from_str(&text).map_err(|_| Error::Invalid("configuration TOML".into()))?;
        cfg.validate()?;
        Ok(cfg)
    }
    pub fn validate(&self) -> AppResult<()> {
        if self.name.is_empty()
            || self.rpc_concurrency == 0
            || self.rpc_requests_per_second == 0
            || self.rpc_requests_per_second > 10000
            || self.max_input_bytes < 1024
            || self.max_input_bytes > 64 * 1024 * 1024
            || self.journal_max_bytes < 2 * self.max_input_bytes as u64 + 72
            || self.poll_ms < 10
            || self.shutdown_seconds == 0
            || self.pending_ttl_seconds < 60
            || self.pending_ttl_seconds > 86400
            || self.api_requests_per_minute == 0
            || self.funding.max_hops > 2
            || self.funding.max_profiles == 0
        {
            return Err(Error::Invalid("configuration budgets".into()));
        }
        let mut ids = HashSet::new();
        for c in &self.chains {
            if !ids.insert(c.id)
                || c.rpc_env.is_empty()
                || !matches!(c.family.as_str(), "ethereum" | "op" | "bsc" | "nitro")
            {
                return Err(Error::Invalid("chain configuration".into()));
            }
            if c.pending && matches!(c.family.as_str(), "op" | "nitro") {
                return Err(Error::Invalid(
                    "sequencer feeds cannot be configured as pending".into(),
                ));
            }
        }
        // Live executor deliberately remains gated until chain differential and signer conformance pass.
        if self.mode == Mode::ExecuteAuthorized {
            return Err(Error::Unsupported(
                "production execution is gated; use isolated response replay".into(),
            ));
        }
        Ok(())
    }
    pub fn secret(name: &str) -> AppResult<String> {
        std::env::var(name)
            .map_err(|_| Error::Dependency(format!("environment variable {name} is not set")))
    }
}
