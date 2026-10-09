use crate::config::{ChainConfig, Config};
use futures_util::StreamExt;
use serde_json::{Value, json};
use siren_lib::{AppResult, Error};
use std::{sync::{Arc, OnceLock, atomic::{AtomicU64, Ordering}}, time::Duration};
use tokio::{sync::{Mutex, Semaphore}, time::Instant};

/// Shared across chain pools: one API account commonly funds all four endpoints.
struct Pacing { next: Instant, blocked_until: Instant }
fn pacing() -> &'static Mutex<Pacing> {
    static SHARED: OnceLock<Mutex<Pacing>> = OnceLock::new();
    SHARED.get_or_init(|| Mutex::new(Pacing { next: Instant::now(), blocked_until: Instant::now() }))
}
pub struct ProviderPool {
    pub chain_id: u64,
    client: reqwest::Client,
    endpoints: Vec<String>,
    verified: Vec<Mutex<Option<Instant>>>,
    gate: Arc<Semaphore>,
    next: AtomicU64,
    max_bytes: usize,
    interval: Duration,
}
impl ProviderPool {
    pub fn new(c: &ChainConfig, cfg: &Config) -> AppResult<Self> {
        let endpoints = c.rpc_env.iter().map(|k| Config::secret(k)).collect::<AppResult<Vec<_>>>()?;
        let verified = endpoints.iter().map(|_| Mutex::new(None)).collect();
        let client = reqwest::Client::builder().timeout(Duration::from_secs(15))
            .connect_timeout(Duration::from_secs(5)).redirect(reqwest::redirect::Policy::none())
            .build().map_err(|_| Error::Provider("client initialization".into()))?;
        Ok(Self { chain_id: c.id, client, endpoints, verified,
            gate: Arc::new(Semaphore::new(cfg.rpc_concurrency)), next: AtomicU64::new(1),
            max_bytes: cfg.max_input_bytes,
            interval: Duration::from_secs_f64(1.0 / cfg.rpc_requests_per_second as f64) })
    }
    async fn pace(&self) -> AppResult<()> {
        loop {
            let mut state = pacing().lock().await;
            let now = Instant::now();
            if state.blocked_until > now {
                return Err(Error::RateLimited { retry_after_secs: (state.blocked_until-now).as_secs()+1 });
            }
            if state.next <= now { state.next = now+self.interval; return Ok(()); }
            let wait = state.next-now;
            drop(state);
            tokio::time::sleep(wait).await;
        }
    }
    async fn limited(&self, seconds: u64) -> Error {
        let seconds = seconds.clamp(1, 3600);
        let mut state = pacing().lock().await;
        state.blocked_until = state.blocked_until.max(Instant::now()+Duration::from_secs(seconds));
        Error::RateLimited { retry_after_secs: seconds }
    }
    async fn at(&self, url: &str, method: &str, params: Value) -> AppResult<Value> {
        self.pace().await?;
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let response = self.client.post(url).json(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .send().await.map_err(|_| Error::Provider("transport failure (connection/timeout)".into()))?;
        if response.status().as_u16()==429 {
            let seconds=response.headers().get(reqwest::header::RETRY_AFTER)
                .and_then(|v|v.to_str().ok()).and_then(|v|v.parse().ok()).unwrap_or(30);
            return Err(self.limited(seconds).await);
        }
        if !response.status().is_success() {
            return Err(Error::Provider(format!("{method}: HTTP {}", response.status().as_u16())));
        }
        if response.content_length().is_some_and(|n| n > self.max_bytes as u64) { return Err(Error::Limit("RPC response".into())); }
        let mut stream = response.bytes_stream(); let mut body = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| Error::Provider("body failure".into()))?;
            if body.len() + chunk.len() > self.max_bytes { return Err(Error::Limit("RPC body".into())); }
            body.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&body)?;
        if value["id"] != json!(id) { return Err(Error::Provider("RPC response id mismatch".into())); }
        if !value["error"].is_null() {
            let code=value["error"]["code"].as_i64();
            if matches!(code,Some(429)|Some(-32005)) { return Err(self.limited(30).await); }
            if code==Some(-32601) { return Err(Error::Unsupported(format!("{method}: method not available"))); }
            // Do not log provider messages: they may reflect credential-bearing URLs.
            return Err(Error::Provider(format!("{method}: RPC code {}",value["error"]["code"])));
        }
        value.get("result").cloned().ok_or_else(|| Error::Provider("missing result".into()))
    }
    async fn verify(&self, index: usize) -> AppResult<()> {
        let mut last = self.verified[index].lock().await;
        if last.is_some_and(|when|when.elapsed()<Duration::from_secs(60)) { return Ok(()); }
        // Single-flight probe per endpoint. Transport/rate-limit errors retain their real cause.
        let id=quantity(&self.at(&self.endpoints[index],"eth_chainId",json!([])).await?)?;
        if id!=self.chain_id {
            return Err(Error::Provider(format!("wrong chain: expected {}, received {id}",self.chain_id)));
        }
        *last=Some(Instant::now());
        Ok(())
    }
    pub async fn request(&self, method: &str, params: Value) -> AppResult<Value> {
        let _permit = self.gate.acquire().await.map_err(|_| Error::Dependency("RPC shutting down".into()))?;
        let mut last = Error::Provider("no healthy endpoint".into());
        for index in 0..self.endpoints.len().min(4) {
            let result=async {
                self.verify(index).await?;
                if method=="eth_chainId" { return Ok(json!(format!("0x{:x}",self.chain_id))); }
                self.at(&self.endpoints[index],method,params.clone()).await
            }.await;
            match result {
                Ok(value)=>return Ok(value),
                Err(e @ Error::RateLimited{..})=>return Err(e),
                Err(e)=>last=e,
            }
        }
        Err(last)
    }
    pub async fn anchored(&self, method: &str, params: Value, height: u64, hash: alloy_primitives::B256) -> AppResult<Value> {
        let _permit = self.gate.acquire().await.map_err(|_| Error::Dependency("RPC shutting down".into()))?;
        let mut last=Error::Provider("no endpoint matching anchored state".into());
        for index in 0..self.endpoints.len().min(4) {
            let url=&self.endpoints[index];
            let result: AppResult<Value> = async {
                self.verify(index).await?;
                let block = self.at(url, "eth_getBlockByNumber", json!([format!("0x{height:x}"), false])).await?;
                if block["hash"] != json!(hash) { return Err(Error::Provider("state hash mismatch".into())); }
                let result = self.at(url, method, params.clone()).await?;
                let after = self.at(url, "eth_getBlockByNumber", json!([format!("0x{height:x}"), false])).await?;
                if after["hash"] != json!(hash) { return Err(Error::Provider("state changed during request".into())); }
                Ok(result)
            }.await;
            match result {
                Ok(value)=>return Ok(value),
                Err(e @ Error::RateLimited{..})=>return Err(e),
                Err(e)=>last=e,
            }
        }
        Err(last)
    }
}
pub fn quantity(v: &Value) -> AppResult<u64> {
    let s = v.as_str().ok_or_else(|| Error::Invalid("RPC quantity is not a string".into()))?;
    u64::from_str_radix(s.strip_prefix("0x").ok_or_else(|| Error::Invalid("quantity prefix".into()))?, 16)
        .map_err(|_| Error::Invalid("quantity overflow".into()))
}
pub fn field<T: serde::de::DeserializeOwned>(v: &Value, name: &str) -> AppResult<T> {
    Ok(serde_json::from_value(v.get(name).cloned().ok_or_else(|| Error::Invalid(format!("missing {name}")))?)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{extract::State, response::{IntoResponse,Response}, routing::post, Json, Router};
    use std::sync::atomic::AtomicU8;

    #[derive(Default)]
    struct Mock { mode:AtomicU8, chain_calls:AtomicU64, method_calls:AtomicU64, requests:AtomicU64 }
    async fn rpc(State(state):State<Arc<Mock>>,Json(request):Json<Value>)->Response {
        state.requests.fetch_add(1,Ordering::SeqCst);
        if state.mode.load(Ordering::SeqCst)==1 {
            return (axum::http::StatusCode::TOO_MANY_REQUESTS,[("retry-after","3")],"limited").into_response();
        }
        let result=if request["method"]=="eth_chainId" {
            state.chain_calls.fetch_add(1,Ordering::SeqCst);
            if state.mode.load(Ordering::SeqCst)==2{"0x38"}else{"0x1"}
        }else{
            state.method_calls.fetch_add(1,Ordering::SeqCst);"0x10"
        };
        Json(json!({"jsonrpc":"2.0","id":request["id"],"result":result})).into_response()
    }
    #[tokio::test]
    async fn verification_is_cached_and_rate_limits_keep_their_cause() {
        let listener=tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address=listener.local_addr().unwrap();
        let state=Arc::new(Mock::default());
        let app=Router::new().route("/",post(rpc)).with_state(state.clone());
        let server=tokio::spawn(async move{axum::serve(listener,app).await.unwrap();});
        let pool=ProviderPool {
            chain_id:1,client:reqwest::Client::new(),endpoints:vec![format!("http://{address}/")],
            verified:vec![Mutex::new(None)],gate:Arc::new(Semaphore::new(2)),next:AtomicU64::new(1),
            max_bytes:4096,interval:Duration::from_millis(1),
        };
        for _ in 0..2{assert_eq!(pool.request("eth_blockNumber",json!([])).await.unwrap(),json!("0x10"));}
        assert_eq!(state.chain_calls.load(Ordering::SeqCst),1);
        assert_eq!(state.method_calls.load(Ordering::SeqCst),2);
        // A newly verified/failover endpoint cannot bypass chain-domain checks.
        *pool.verified[0].lock().await=None;state.mode.store(2,Ordering::SeqCst);
        let error=pool.request("eth_blockNumber",json!([])).await.unwrap_err();
        assert!(matches!(error,Error::Provider(ref message) if message.contains("wrong chain")));
        assert_eq!(state.method_calls.load(Ordering::SeqCst),2);
        state.mode.store(1,Ordering::SeqCst);
        assert!(matches!(pool.request("eth_blockNumber",json!([])).await,Err(Error::RateLimited{retry_after_secs:3})));
        let requests=state.requests.load(Ordering::SeqCst);
        assert!(matches!(pool.request("eth_blockNumber",json!([])).await,Err(Error::RateLimited{..})));
        assert_eq!(state.requests.load(Ordering::SeqCst),requests,"cooldown must prevent another wire request");
        server.abort();
    }
}
