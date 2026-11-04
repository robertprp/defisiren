use crate::{
    detection::finding,
    funding::bridge::{self, BridgeSource},
    store::publish,
};
use serde_json::{json, Value};
use siren_lib::{entity::*, AppResult, Error};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

pub async fn observe(
    tx: &mut Transaction<'_, Postgres>,
    o: &Observation,
) -> AppResult<Vec<Finding>> {
    let mut keys = vec![];
    for source in bridge::sources(o)? {
        sqlx::query("INSERT INTO bridge_messages(destination_chain,message_id,source_data,source_block,source_canonical) VALUES($1,$2,$3,$4,true) ON CONFLICT(destination_chain,message_id) DO UPDATE SET source_data=EXCLUDED.source_data,source_block=EXCLUDED.source_block,source_canonical=true")
            .bind(source.destination_chain as i64).bind(source.message_id.as_slice()).bind(serde_json::to_value(&source)?)
            .bind(source.source_position.block_hash.map(|b|b.to_vec())).execute(&mut **tx).await.map_err(|_|Error::Database)?;
        keys.push((source.destination_chain, source.message_id));
    }
    if let Some(id) = bridge::destination_id(o)? {
        sqlx::query("INSERT INTO bridge_messages(destination_chain,message_id,destination_data,destination_block,destination_canonical) VALUES($1,$2,$3,$4,true) ON CONFLICT(destination_chain,message_id) DO UPDATE SET destination_data=EXCLUDED.destination_data,destination_block=EXCLUDED.destination_block,destination_canonical=true")
            .bind(o.chain_id as i64).bind(id.as_slice()).bind(serde_json::to_value(o)?).bind(o.position.block_hash.map(|b|b.to_vec()))
            .execute(&mut **tx).await.map_err(|_|Error::Database)?;
        keys.push((o.chain_id, id));
    }
    let mut findings = vec![];
    for (chain, id) in keys {
        let row=sqlx::query("SELECT source_data,destination_data,correlated,source_canonical,destination_canonical FROM bridge_messages WHERE destination_chain=$1 AND message_id=$2 FOR UPDATE")
            .bind(chain as i64).bind(id.as_slice()).fetch_one(&mut **tx).await.map_err(|_|Error::Database)?;
        if row.get::<bool, _>("correlated")
            || !row.get::<bool, _>("source_canonical")
            || !row.get::<bool, _>("destination_canonical")
        {
            continue;
        }
        let (Some(source), Some(destination)) = (
            row.get::<Option<Value>, _>("source_data"),
            row.get::<Option<Value>, _>("destination_data"),
        ) else {
            continue;
        };
        let source: BridgeSource = serde_json::from_value(source)?;
        let destination: Observation = serde_json::from_value(destination)?;
        if !bridge::matches(&source, &destination)? {
            return Err(Error::Invalid(
                "bridge counterpart identity mismatch".into(),
            ));
        }
        let evidence=Evidence{id:format!("bridge:{chain}:{id}"),kind:"canonical_bridge_message".into(),
            description:"Direct native deposit correlated by protocol-derived ID; chain identities and finality remain separate.".into(),
            transaction:Some(destination.hash),data:json!({"source":source,"destination":{"chain_id":destination.chain_id,"transaction":destination.hash,"position":destination.position,"finality":destination.stage},"funding_attribution":"transfer evidence, not unique fungible units"})};
        let mut f = finding(&destination, "bridge_transfer", vec![evidence], 0);
        f.incident_id = Uuid::new_v5(
            &Uuid::NAMESPACE_OID,
            format!("bridge:{chain}:{id}").as_bytes(),
        );
        f.addresses = vec![source.destination_recipient];
        f.confidence = 10000;
        findings.push(publish(tx, f).await?);
        sqlx::query("UPDATE bridge_messages SET correlated=true WHERE destination_chain=$1 AND message_id=$2")
            .bind(chain as i64).bind(id.as_slice()).execute(&mut **tx).await.map_err(|_|Error::Database)?;
    }
    Ok(findings)
}

pub async fn refresh_watches(
    tx: &mut Transaction<'_, Postgres>,
    chain: u64,
    engine: &mut crate::detection::Engine,
    cfg: &crate::config::Config,
    now: chrono::DateTime<chrono::Utc>,
) -> AppResult<()> {
    let rows=sqlx::query("SELECT source_data,destination_data FROM bridge_messages WHERE destination_chain=$1 AND source_canonical AND destination_canonical AND correlated AND (destination_data->>'observed_at')::timestamptz >= $2 ORDER BY (destination_data->>'observed_at')::timestamptz DESC,message_id LIMIT $3")
        .bind(chain as i64).bind(now-chrono::Duration::seconds(cfg.funding.watch_seconds))
        .bind(cfg.funding.max_profiles as i64).fetch_all(&mut **tx).await.map_err(|_|Error::Database)?;
    let mut valid = std::collections::BTreeSet::new();
    let mut fanout = std::collections::BTreeMap::<
        alloy_primitives::Address,
        std::collections::BTreeSet<alloy_primitives::Address>,
    >::new();
    for row in rows {
        let source: BridgeSource = serde_json::from_value(row.get("source_data"))?;
        let destination: Observation = serde_json::from_value(row.get("destination_data"))?;
        if !bridge::matches(&source, &destination)?
            || destination.observed_at > now
            || cfg.shared_accounts.contains(&source.source_sender)
            || cfg.shared_accounts.contains(&source.destination_recipient)
        {
            continue;
        }
        let parent:Option<Value>=sqlx::query_scalar("SELECT data FROM funding_edges WHERE chain_id=1 AND recipient=$1 AND canonical AND (data->>'hop')::integer<$2 AND (data->>'observed_at')::timestamptz<=$3 AND (data->>'observed_at')::timestamptz>=$4 ORDER BY (data->>'confidence')::integer DESC,id LIMIT 1")
            .bind(source.source_sender.as_slice()).bind(cfg.funding.max_hops as i32).bind(source.observed_at)
            .bind(source.observed_at-chrono::Duration::seconds(cfg.funding.recent_seconds))
            .fetch_optional(&mut **tx).await.map_err(|_|Error::Database)?;
        let Some(parent) = parent else {
            continue;
        };
        let parent: FundingEdge = serde_json::from_value(parent)?;
        let amount = source
            .amount
            .parse::<alloy_primitives::U256>()
            .map_err(|_| Error::Invalid("bridge amount".into()))?;
        if amount < cfg.funding.minimum_native {
            continue;
        }
        let recipients = fanout.entry(source.source_sender).or_default();
        if recipients.len() >= cfg.funding.max_fanout
            && !recipients.contains(&source.destination_recipient)
        {
            continue;
        }
        recipients.insert(source.destination_recipient);
        let root = format!("bridge:{}:{}", chain, source.message_id);
        valid.insert(root.clone());
        engine.watch.add(
            FundingEdge {
                id: root.clone(),
                bridge_origin: Some(root),
                chain_id: chain,
                from: destination.from,
                to: source.destination_recipient,
                asset: None,
                amount: source.amount,
                fee: None,
                relayer: None,
                transaction: destination.hash,
                log_index: None,
                block_hash: destination.position.block_hash,
                observed_at: destination.observed_at,
                provisional: false,
                hop: parent.hop + 1,
                confidence: ((parent.confidence as u32) * 7 / 10) as u16,
            },
            &cfg.funding,
        )?;
    }
    for profile in engine.watch.profiles.values_mut() {
        profile.funding.retain(|e| {
            e.bridge_origin
                .as_ref()
                .map_or(true, |root| valid.contains(root))
        });
    }
    engine
        .watch
        .profiles
        .retain(|a, p| !p.funding.is_empty() || !engine.watch.derived_only.contains(a));
    engine
        .watch
        .derived_only
        .retain(|a| engine.watch.profiles.contains_key(a));
    // Invalidate descendants as well as direct bridge edges after either canonical half is lost.
    let roots: Vec<String> = valid.into_iter().collect();
    sqlx::query("UPDATE funding_edges SET canonical=false WHERE chain_id=$1 AND data->>'bridge_origin' IS NOT NULL AND NOT (data->>'bridge_origin'=ANY($2::text[]))")
        .bind(chain as i64).bind(roots).execute(&mut **tx).await.map_err(|_|Error::Database)?;
    Ok(())
}
