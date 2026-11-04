use super::Accepted;
use sha2::{Digest, Sha256};
use siren_lib::{AppResult, Error};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};
/// Frames: u32 big-endian length | SHA256 payload | JSON payload. fsync is the acceptance boundary.
/// Single writer per process; PostgreSQL chain advisory locks serialize canonical application.
pub struct Journal {
    path: PathBuf,
    max_bytes: u64,
    max_frame: usize,
    _lock: File,
}

impl Journal {
    pub fn open(path: &Path, max_bytes: u64, max_frame: usize) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let lock = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .truncate(false)
            .open(path.with_extension("lock"))?;
        fs2::FileExt::try_lock_exclusive(&lock)
            .map_err(|_| Error::Dependency("journal writer lock held".into()))?;
        let f = OpenOptions::new().create(true).append(true).open(path)?;
        f.sync_all()?;
        if let Some(parent) = path.parent() {
            File::open(parent)?.sync_all()?;
        }
        Ok(Self {
            path: path.to_owned(),
            max_bytes,
            max_frame,
            _lock: lock,
        })
    }

    pub fn append(&self, record: &Accepted) -> AppResult<()> {
        let bytes = serde_json::to_vec(record)?;
        if bytes.len() > self.max_frame {
            return Err(Error::Limit("journal frame".into()));
        }
        let mut file = OpenOptions::new().append(true).open(&self.path)?;
        if file.metadata()?.len() + bytes.len() as u64 + 36 > self.max_bytes {
            return Err(Error::Durability);
        }
        file.write_all(&(bytes.len() as u32).to_be_bytes())
            .map_err(|_| Error::Durability)?;
        file.write_all(&Sha256::digest(&bytes))
            .map_err(|_| Error::Durability)?;
        file.write_all(&bytes).map_err(|_| Error::Durability)?;
        file.sync_data().map_err(|_| Error::Durability)?;
        Ok(())
    }

    pub fn read_at(&self, offset: u64) -> AppResult<Option<(Accepted, u64)>> {
        let mut f = OpenOptions::new().read(true).write(true).open(&self.path)?;
        let total = f.metadata()?.len();
        if total > self.max_bytes || offset > total {
            return Err(Error::Limit("journal bounds".into()));
        }
        if offset == total {
            return Ok(None);
        }
        f.seek(SeekFrom::Start(offset))?;
        let mut header = [0u8; 36];
        if let Err(e) = f.read_exact(&mut header) {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                f.set_len(offset)?;
                f.sync_data()?;
                return Ok(None);
            }
            return Err(e.into());
        }
        let len =
            u32::from_be_bytes(header[..4].try_into().map_err(|_| Error::Durability)?) as usize;
        if len > self.max_frame {
            return Err(Error::Durability);
        }
        let mut bytes = vec![0; len];
        if let Err(e) = f.read_exact(&mut bytes) {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                f.set_len(offset)?;
                f.sync_data()?;
                return Ok(None);
            }
            return Err(e.into());
        }
        if Sha256::digest(&bytes)[..] != header[4..] {
            return Err(Error::Durability);
        }
        Ok(Some((
            serde_json::from_slice(&bytes)?,
            offset + 36 + len as u64,
        )))
    }

    pub fn recover(&self) -> AppResult<Vec<Accepted>> {
        let mut f = OpenOptions::new().read(true).write(true).open(&self.path)?;
        let mut result = Vec::new();
        let mut offset = 0u64;
        if f.metadata()?.len() > self.max_bytes {
            return Err(Error::Limit("journal exceeds configured budget".into()));
        }
        loop {
            let mut header = [0u8; 36];
            match f.read_exact(&mut header) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                    f.set_len(offset)?;
                    f.sync_data()?;
                    break;
                }
                Err(e) => return Err(e.into()),
            }
            let len =
                u32::from_be_bytes(header[..4].try_into().map_err(|_| Error::Durability)?) as usize;
            if len > self.max_frame {
                return Err(Error::Durability);
            }
            let mut payload = vec![0; len];
            if let Err(e) = f.read_exact(&mut payload) {
                if e.kind() == std::io::ErrorKind::UnexpectedEof {
                    f.set_len(offset)?;
                    f.sync_data()?;
                    break;
                }
                return Err(e.into());
            }
            if Sha256::digest(&payload)[..] != header[4..] {
                return Err(Error::Durability);
            }
            result.push(serde_json::from_slice(&payload)?);
            offset = f.stream_position()?;
        }
        Ok(result)
    }

    /// Caller must hold the ingestion writer lock and prove every frame committed to PostgreSQL.
    /// Atomic empty-file replacement keeps either the old idempotently replayable journal or an empty one.
    pub fn checkpoint_committed(&self, force: bool) -> AppResult<bool> {
        if !force && std::fs::metadata(&self.path)?.len() < self.max_bytes / 2 {
            return Ok(false);
        }
        let temporary = self.path.with_extension("checkpoint.tmp");
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary)?;
        file.sync_all()?;
        std::fs::rename(&temporary, &self.path)?;
        if let Some(parent) = self.path.parent() {
            File::open(parent)?.sync_all()?;
        }
        Ok(true)
    }

    pub fn flush(&self) -> AppResult<()> {
        File::open(&self.path)?.sync_all()?;
        Ok(())
    }
}
