use sha2::{Digest,Sha256};
use siren_lib::{AppResult,Error};
use std::{fs::{File,OpenOptions},io::Write,path::{Path,PathBuf}};
#[derive(Clone)]
pub struct EvidenceStore {pub root:PathBuf,pub max_object_bytes:usize}
impl EvidenceStore {
    pub fn new(root:&Path,max_object_bytes:usize)->AppResult<Self>{std::fs::create_dir_all(root)?;Ok(Self{root:root.to_owned(),max_object_bytes})}
    /// Content addressing makes crash retries idempotent. Checksums cover uncompressed JSON.
    pub fn put(&self,value:&serde_json::Value)->AppResult<(String,String,usize)>{
        let bytes=serde_json::to_vec(value)?;if bytes.len()>self.max_object_bytes{return Err(Error::Limit("evidence object".into()));}
        let checksum=hex::encode(Sha256::digest(&bytes));let name=format!("{checksum}.json.zst");let target=self.root.join(&name);
        if target.exists(){return Ok((checksum,name,bytes.len()));}
        let compressed=zstd::stream::encode_all(bytes.as_slice(),3)?;
        let temp=self.root.join(format!(".{}.tmp",uuid::Uuid::new_v4()));
        let mut file=OpenOptions::new().create_new(true).write(true).open(&temp)?;
        file.write_all(&compressed)?;file.sync_all()?;std::fs::rename(&temp,&target)?;File::open(&self.root)?.sync_all()?;
        Ok((checksum,name,bytes.len()))
    }
    pub fn get(&self,checksum:&str)->AppResult<serde_json::Value>{
        if checksum.len()!=64||!checksum.bytes().all(|c|c.is_ascii_hexdigit()){return Err(Error::Invalid("evidence checksum".into()));}
        let file=File::open(self.root.join(format!("{checksum}.json.zst")))?;
        use std::io::Read;
        let mut bytes=vec![];zstd::stream::read::Decoder::new(file)?.take(self.max_object_bytes as u64+1).read_to_end(&mut bytes)?;
        if bytes.len()>self.max_object_bytes||hex::encode(Sha256::digest(&bytes))!=checksum{return Err(Error::Durability);}
        Ok(serde_json::from_slice(&bytes)?)
    }
}
