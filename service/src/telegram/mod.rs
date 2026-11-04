use crate::{config::Config, services::Services};
use siren_lib::{AppResult, Error};
use sqlx::Row;
use std::{sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;
pub async fn run(s: Arc<Services>, cancel: CancellationToken) -> AppResult<()> {
    let token = Config::secret(
        s.config
            .telegram_token_env
            .as_deref()
            .ok_or_else(|| Error::Dependency("Telegram token reference".into()))?,
    )?;
    let chat = Config::secret(
        s.config
            .telegram_chat_env
            .as_deref()
            .ok_or_else(|| Error::Dependency("Telegram destination reference".into()))?,
    )?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|_| Error::Dependency("Telegram client".into()))?;
    loop {
        tokio::select! {_=cancel.cancelled()=>return Ok(()),_=tokio::time::sleep(Duration::from_secs(1))=>{}}
        let mut tx = s.store.pool.begin().await.map_err(|_| Error::Database)?;
        let row=sqlx::query("SELECT cursor,data FROM delivery_outbox WHERE stream='findings' AND NOT telegram_sent AND next_attempt<=now() ORDER BY cursor LIMIT 1 FOR UPDATE SKIP LOCKED")
            .fetch_optional(&mut *tx).await.map_err(|_|Error::Database)?;
        let Some(row) = row else {
            tx.rollback().await.map_err(|_| Error::Database)?;
            continue;
        };
        let cursor: i64 = row.get("cursor");
        let data: serde_json::Value = row.get("data");
        let text=format!("chain-siren {} | chain {} | {} | incident {} revision {}\nEvidence is provisional; see authenticated findings feed.",data["finding_type"],data["chain_id"],data["severity"],data["incident_id"],data["revision"]);
        let sent = match client
            .post(format!("https://api.telegram.org/bot{token}/sendMessage"))
            .json(&serde_json::json!({"chat_id":chat,"text":text}))
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => r
                .json::<serde_json::Value>()
                .await
                .ok()
                .is_some_and(|v| v["ok"] == true),
            _ => false,
        };
        sqlx::query("UPDATE delivery_outbox SET telegram_sent=$2,attempts=attempts+1,next_attempt=now()+make_interval(secs => LEAST(3600,power(2,LEAST(attempts,12))::int)) WHERE stream='findings' AND cursor=$1")
            .bind(cursor).bind(sent).execute(&mut *tx).await.map_err(|_|Error::Database)?;
        tx.commit().await.map_err(|_| Error::Database)?;
        // Telegram has no idempotency key. A crash after send/before commit may duplicate the same event ID.
    }
}
