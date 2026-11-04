use crate::{
    chain::RpcAdapter,
    config::Config,
    contracts::{self, Pool},
    events::{journal::Journal, Accepted},
    store::Store,
    telemetry::Metrics,
};
use siren_lib::{entity::Finding, AppResult, Error};
use sqlx::{Connection, PgConnection};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
use tokio::sync::{Mutex, OnceCell};

pub struct Services {
    pub config: Arc<Config>,
    pub store: Arc<Store>,
    pub chains: Vec<Arc<RpcAdapter>>,
    pub pools: Vec<Pool>,
    pub metrics: Arc<Metrics>,
    pub healthy: AtomicBool,
    pub journal: Arc<Journal>,
    writer: Mutex<()>,
    /// Session lock prevents simultaneous use of the same single-writer journal.
    _journal_lease: Mutex<PgConnection>,
}
#[derive(Default)]
pub struct ServiceFactory {
    cell: OnceCell<Arc<Services>>,
}
impl ServiceFactory {
    pub async fn initialize(&self, cfg: Config) -> AppResult<Arc<Services>> {
        self.cell
            .get_or_try_init(|| async {
                let database = Config::secret(&cfg.database_env)?;
                let store = Store::connect(&database).await?;
                let mut lease = PgConnection::connect(&database)
                    .await
                    .map_err(|_| Error::Database)?;
                let locked: bool =
                    sqlx::query_scalar("SELECT pg_try_advisory_lock(746219035112::bigint)")
                        .fetch_one(&mut lease)
                        .await
                        .map_err(|_| Error::Database)?;
                if !locked {
                    return Err(Error::Dependency(
                        "single-writer ingestion lease is held; API readers may run separately"
                            .into(),
                    ));
                }
                let journal = Arc::new(Journal::open(
                    &Path::new(&cfg.data_dir).join("accepted.wal"),
                    cfg.journal_max_bytes,
                    cfg.max_input_bytes,
                )?);
                let mut chains = vec![];
                for c in &cfg.chains {
                    match RpcAdapter::new(c.clone(), &cfg) {
                        Ok(a) => chains.push(Arc::new(a)),
                        Err(_) => {
                            tracing::warn!(chain_id = c.id, "provider missing; chain degraded")
                        }
                    }
                }
                let services = Arc::new(Services {
                    config: Arc::new(cfg),
                    store,
                    chains,
                    pools: contracts::pools()?,
                    metrics: Arc::new(Metrics::default()),
                    healthy: AtomicBool::new(true),
                    journal,
                    writer: Mutex::new(()),
                    _journal_lease: Mutex::new(lease),
                });
                services.recover().await?;
                Ok(services)
            })
            .await
            .cloned()
    }
}
impl Services {
    pub async fn accept(&self, event: Accepted) -> AppResult<Vec<Finding>> {
        let _guard = self.writer.lock().await;
        if !self.healthy.load(Ordering::SeqCst) {
            return Err(Error::Durability);
        }
        let j = self.journal.clone();
        let copy = event.clone();
        if !matches!(
            tokio::task::spawn_blocking(move || j.append(&copy)).await,
            Ok(Ok(()))
        ) {
            self.healthy.store(false, Ordering::SeqCst);
            self.metrics
                .durability_errors
                .fetch_add(1, Ordering::Relaxed);
            return Err(Error::Durability);
        }
        let count = match &event {
            Accepted::Block(b) => b.observations.len(),
            Accepted::Observation(_) => 1,
            Accepted::Feed(_) | Accepted::Progress(_) => 0,
        };
        let result = self.store.accept(&event, &self.config, &self.pools).await;
        match result {
            Ok(findings) => {
                let j = self.journal.clone();
                if !matches!(
                    tokio::task::spawn_blocking(move || j.checkpoint_committed(false)).await,
                    Ok(Ok(_))
                ) {
                    self.healthy.store(false, Ordering::SeqCst);
                    self.metrics
                        .durability_errors
                        .fetch_add(1, Ordering::Relaxed);
                    return Err(Error::Durability);
                }
                self.metrics
                    .observations
                    .fetch_add(count as u64, Ordering::Relaxed);
                self.metrics
                    .findings
                    .fetch_add(findings.len() as u64, Ordering::Relaxed);
                Ok(findings)
            }
            Err(e) => {
                self.healthy.store(false, Ordering::SeqCst);
                Err(e)
            }
        }
    }
    pub async fn recover(&self) -> AppResult<usize> {
        let _guard = self.writer.lock().await;
        self.healthy.store(false, Ordering::SeqCst);
        let mut offset = 0;
        let mut count = 0;
        loop {
            let j = self.journal.clone();
            let record = tokio::task::spawn_blocking(move || j.read_at(offset))
                .await
                .map_err(|_| Error::Durability)??;
            let Some((record, next)) = record else {
                break;
            };
            self.store
                .accept(&record, &self.config, &self.pools)
                .await?;
            offset = next;
            count += 1;
        }
        let j = self.journal.clone();
        tokio::task::spawn_blocking(move || j.checkpoint_committed(true))
            .await
            .map_err(|_| Error::Durability)??;
        self.healthy.store(true, Ordering::SeqCst);
        Ok(count)
    }
}
