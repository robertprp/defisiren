use alloy_primitives::{Address, B256};
use clap::{Parser, Subcommand};
use serde_json::json;
use siren_lib::{entity::*, AppResult, Error};
use siren_service::{
    analysis,
    chain::{BlockSource, ChainAdapter, RpcAdapter},
    config::Config,
    core::app::runner,
    events::Accepted,
    services::ServiceFactory,
    simulation::{self, SimulationBackend},
    store::{publish, Store},
};
use std::{path::PathBuf, sync::Arc};

#[derive(Parser)]
#[command(
    name = "chain-siren",
    version,
    about = "Capability-aware EVM evidence monitor"
)]
struct Cli {
    #[arg(long, default_value = "config/example.toml", global = true)]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Run,
    Doctor,
    Backfill {
        #[arg(long)]
        chain: u64,
        #[arg(long)]
        from: u64,
        #[arg(long)]
        to: u64,
    },
    Replay {
        file: PathBuf,
    },
    AnalyzeTx {
        #[arg(long)]
        chain: u64,
        hash: B256,
    },
    AnalyzeContract {
        #[arg(long)]
        chain: u64,
        address: Address,
        #[arg(long)]
        block_hash: B256,
    },
    Watch {
        #[arg(long)]
        chain: u64,
        address: Address,
        #[arg(long, default_value_t = 168)]
        hours: i64,
        #[arg(long, default_value = "operator watch")]
        reason: String,
    },
    Policy {
        file: PathBuf,
    },
    ApiKey {
        #[arg(long)]
        name: String,
    },
    Benchmark {
        #[arg(long, default_value_t = 100_000)]
        iterations: usize,
    },
    Review {
        id: String,
        #[arg(long)]
        reason: String,
    },
    Demo {
        #[arg(long, default_value = "fixtures/demo.jsonl")]
        file: PathBuf,
    },
    #[command(hide = true)]
    SimulationWorker,
}
fn output(value: &impl serde::Serialize) -> AppResult<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
async fn store(cfg: &Config) -> AppResult<Arc<Store>> {
    Store::connect(&Config::secret(&cfg.database_env)?).await
}
fn adapter(cfg: &Config, chain: u64) -> AppResult<RpcAdapter> {
    let c = cfg
        .chains
        .iter()
        .find(|c| c.id == chain)
        .ok_or_else(|| Error::Invalid("chain not configured".into()))?;
    RpcAdapter::new(c.clone(), cfg)
}
pub async fn execute() -> AppResult<()> {
    let cli = Cli::parse();
    if matches!(cli.command, Command::SimulationWorker) {
        use tokio::io::AsyncReadExt;
        let mut bytes = Vec::new();
        tokio::io::stdin()
            .take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .await?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(Error::Limit("worker input".into()));
        }
        let result = simulation::execute(serde_json::from_slice(&bytes)?)?;
        return output(&result);
    }
    siren_service::telemetry::init();
    let cfg = Config::load(&cli.config).await?;
    match cli.command {
        Command::Run => {
            let s = ServiceFactory::default().initialize(cfg).await?;
            runner::run(s).await
        }
        Command::Doctor => {
            let mut reports = vec![];
            let mut verified = true;
            for c in &cfg.chains {
                match adapter(&cfg, c.id) {
                    Ok(a) => match a.capabilities().await {
                        Ok(r) => reports.push(json!(r)),
                        Err(e) => {
                            verified = false;
                            reports.push(
                                json!({"chain_id":c.id,"verified":false,"error":e.to_string()}),
                            );
                        }
                    },
                    Err(e) => {
                        verified = false;
                        reports
                            .push(json!({"chain_id":c.id,"verified":false,"error":e.to_string()}));
                    }
                }
            }
            output(
                &json!({"chains":reports,"rpc_endpoints_reachable":verified,"live_integration_verified":false,"response_execution_enabled":false}),
            )?;
            if verified {
                Ok(())
            } else {
                Err(Error::Dependency(
                    "one or more provider integrations unverified".into(),
                ))
            }
        }
        Command::Backfill { chain, from, to } => {
            if to < from || to - from > 100_000 {
                return Err(Error::Limit(
                    "backfill range (maximum 100000 blocks per invocation)".into(),
                ));
            }
            let s = ServiceFactory::default().initialize(cfg).await?;
            let a = s
                .chains
                .iter()
                .find(|a| a.config.id == chain)
                .ok_or_else(|| Error::Dependency("chain provider".into()))?;
            for number in from..=to {
                s.accept(Accepted::Block(a.block(number).await?)).await?;
            }
            Ok(())
        }
        Command::Replay { file } => {
            let s = ServiceFactory::default().initialize(cfg).await?;
            replay(&s, &file).await
        }
        Command::AnalyzeTx { chain, hash } => {
            let a = adapter(&cfg, chain)?;
            let tx = a
                .pool
                .request("eth_getTransactionByHash", json!([hash]))
                .await?;
            let receipt = a
                .pool
                .request("eth_getTransactionReceipt", json!([hash]))
                .await?;
            let included = !receipt.is_null();
            let o = siren_service::ingestion::normalize(
                chain,
                tx,
                included.then_some(receipt),
                if included {
                    Stage::Included
                } else {
                    Stage::Pending
                },
                "analyze_tx",
            )?;
            output(&a.simulate(&o).await?)
        }
        Command::AnalyzeContract {
            chain,
            address,
            block_hash,
        } => {
            let a = adapter(&cfg, chain)?;
            let code: alloy_primitives::Bytes = serde_json::from_value(
                a.pool
                    .request(
                        "eth_getCode",
                        json!([address,{"blockHash":block_hash,"requireCanonical":true}]),
                    )
                    .await?,
            )?;
            let analyzed = analysis::analyze(&code, cfg.max_input_bytes)?;
            let slot = alloy_primitives::U256::from_be_slice(
                alloy_primitives::keccak256(b"eip1967.proxy.implementation").as_slice(),
            ) - alloy_primitives::U256::from(1);
            let implementation=a.pool.request("eth_getStorageAt",json!([address,format!("0x{slot:064x}"),{"blockHash":block_hash,"requireCanonical":true}])).await;
            output(
                &json!({"analysis":analyzed,"state_hash":block_hash,"eip1967_implementation_slot":implementation.ok(),"proxy_resolution":"slot candidate; code and observed delegation require corroboration"}),
            )
        }
        Command::Watch {
            chain,
            address,
            hours,
            reason,
        } => {
            store(&cfg)
                .await?
                .watch(chain, address, hours, &reason)
                .await
        }
        Command::ApiKey { name } => {
            let key = store(&cfg).await?.new_api_key(&name).await?;
            println!("{key}");
            Ok(())
        }
        Command::Policy { file } => {
            let bytes = tokio::fs::read(file).await?;
            if bytes.len() > 64 * 1024 {
                return Err(Error::Limit("policy size".into()));
            }
            let p: ResponsePolicy = serde_json::from_slice(&bytes)?;
            if p.enabled {
                return Err(Error::Denied(
                    "production execution gated: install disabled policies for review".into(),
                ));
            }
            let db = store(&cfg).await?;
            sqlx::query("INSERT INTO response_policies(id,version,data,enabled) VALUES($1,$2,$3,false) ON CONFLICT(id) DO UPDATE SET version=EXCLUDED.version,data=EXCLUDED.data,enabled=false WHERE response_policies.version<EXCLUDED.version")
                .bind(p.id).bind(p.version as i32).bind(serde_json::to_value(p)?).execute(&db.pool).await.map_err(|_|Error::Database)?;
            Ok(())
        }
        Command::Benchmark { iterations } => siren_service::benchmark(iterations, &cfg),
        Command::Review { id, reason } => {
            let id: uuid::Uuid = id
                .parse()
                .map_err(|_| Error::Invalid("incident UUID".into()))?;
            let db = store(&cfg).await?;
            let mut tx = db.pool.begin().await.map_err(|_| Error::Database)?;
            sqlx::query("SELECT cursor FROM stream_heads WHERE stream='findings' FOR UPDATE")
                .execute(&mut *tx)
                .await
                .map_err(|_| Error::Database)?;
            let value: serde_json::Value =
                sqlx::query_scalar("SELECT data FROM incidents WHERE id=$1 FOR UPDATE")
                    .bind(id)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|_| Error::Database)?;
            let mut finding: Finding = serde_json::from_value(value)?;
            finding.reviewed = true;
            finding.evidence.push(Evidence {
                id: format!("review:{}", uuid::Uuid::new_v4()),
                kind: "operator_review".into(),
                description: reason,
                transaction: None,
                data: json!({"automatic_proof":false}),
            });
            let f = publish(&mut tx, finding).await?;
            tx.commit().await.map_err(|_| Error::Database)?;
            output(&f)
        }
        Command::Demo { file } => {
            let s = ServiceFactory::default().initialize(cfg).await?;
            let key = s.store.new_api_key("offline-demo").await?;
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
            let address = listener.local_addr()?;
            let server = tokio::spawn(
                axum::serve(listener, siren_service::api::router(s.clone())).into_future(),
            );
            use std::future::IntoFuture;
            use tokio_tungstenite::tungstenite::client::IntoClientRequest;
            let mut request = format!("ws://{address}/ws/findings?after=0")
                .into_client_request()
                .map_err(|_| Error::Invalid("demo WS request".into()))?;
            request.headers_mut().insert(
                "Authorization",
                format!("Bearer {key}")
                    .parse()
                    .map_err(|_| Error::Invalid("demo header".into()))?,
            );
            let (mut ws, _) = tokio_tungstenite::connect_async(request)
                .await
                .map_err(|_| Error::Dependency("demo WS".into()))?;
            replay(&s, &file).await?;
            let job: simulation::ReplayJob =
                serde_json::from_slice(&tokio::fs::read("fixtures/harmful.json").await?)?;
            let protected: simulation::ReplayJob =
                serde_json::from_slice(&tokio::fs::read("fixtures/protected.json").await?)?;
            let action: Action =
                serde_json::from_slice(&tokio::fs::read("fixtures/action.json").await?)?;
            let policy: ResponsePolicy =
                serde_json::from_slice(&tokio::fs::read("fixtures/policy.json").await?)?;
            let proof =
                siren_service::response::modeled_protection(&action, &job, &protected).await?;
            let now = action.state_time;
            let digest = siren_service::response::validate(
                &policy,
                &action,
                now,
                action.state_hash,
                &policy.authority_result,
                &proof,
                false,
            )?;
            use futures_util::StreamExt;
            let harmful_result =
                simulation::isolated(&job, std::time::Duration::from_secs(3)).await?;
            let mut update_tx = s.store.pool.begin().await.map_err(|_| Error::Database)?;
            let recent:serde_json::Value=sqlx::query_scalar("SELECT data FROM incidents WHERE data->>'finding_type'='unusual_outflow' ORDER BY updated_at DESC LIMIT 1")
                .fetch_one(&mut *update_tx).await.map_err(|_|Error::Database)?;
            let mut updated: Finding = serde_json::from_value(recent)?;
            updated.simulation = harmful_result;
            let updated = publish(&mut update_tx, updated).await?;
            update_tx.commit().await.map_err(|_| Error::Database)?;
            let message = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                while let Some(frame) = ws.next().await {
                    let frame = frame.map_err(|_| Error::Dependency("demo WS frame".into()))?;
                    if let Ok(text) = frame.to_text() {
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(text) {
                            if value["event_id"] == json!(updated.event_id) {
                                return Ok(value);
                            }
                        }
                    }
                }
                Err(Error::Dependency(
                    "demo WS closed before finding update".into(),
                ))
            })
            .await
            .map_err(|_| Error::Dependency("demo WS timeout".into()))??;
            server.abort();
            output(
                &json!({"websocket_message":message,"authorized_local_action_digest":digest,"protection_proof":proof,"live_submission":false}),
            )
        }
        Command::SimulationWorker => unreachable!(),
    }
}
async fn replay(s: &siren_service::services::Services, file: &std::path::Path) -> AppResult<()> {
    let bytes = tokio::fs::read(file).await?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err(Error::Limit("replay file".into()));
    }
    for line in bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        if line.len() > s.config.max_input_bytes {
            return Err(Error::Limit("replay record".into()));
        }
        let mut event: Accepted = serde_json::from_slice(line)?;
        match &mut event {
            Accepted::Block(b) => {
                for o in &mut b.observations {
                    o.source = "offline_replay".into();
                }
            }
            Accepted::Observation(o) => o.source = "offline_replay".into(),
            Accepted::Feed(f) => f.source = "offline_replay".into(),
            Accepted::Progress(p) => p.source = "offline_replay".into(),
        }
        for f in s.accept(event).await? {
            output(&f)?;
        }
    }
    Ok(())
}
