use siren_lib::{AppResult,Error};
use std::{future::Future,time::Duration};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
pub struct TaskManager {pub cancel:CancellationToken,pub tasks:JoinSet<AppResult<()>>}
impl Default for TaskManager {fn default()->Self{Self{cancel:CancellationToken::new(),tasks:JoinSet::new()}}}
impl TaskManager {
    pub fn spawn(&mut self,future:impl Future<Output=AppResult<()>>+Send+'static){self.tasks.spawn(future);}
    pub async fn shutdown(&mut self,deadline:Duration)->AppResult<()> {
        self.cancel.cancel();
        let drain=async{while let Some(result)=self.tasks.join_next().await {result.map_err(|_|Error::Dependency("worker join".into()))??;}Ok::<_,Error>(())};
        match tokio::time::timeout(deadline,drain).await {
            Ok(r)=>r,Err(_)=>{self.tasks.abort_all();while self.tasks.join_next().await.is_some(){}Err(Error::Limit("shutdown deadline; durable journal will replay".into()))}
        }
    }
}
