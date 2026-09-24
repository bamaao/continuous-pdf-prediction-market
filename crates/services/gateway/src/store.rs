//! Durable receipt files. Not the ledger (FR-DUR-04). No private keys.

use crate::Receipt;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stored {
    pub receipt: Receipt,
    /// Signed tx only, so the same nonce can be re-forwarded. Never a secret key.
    pub tx_b64: String,
    pub tx_sha: String,
}

#[derive(Clone)]
pub struct FileStore {
    dir: PathBuf,
    inner: Arc<Mutex<HashMap<String, Stored>>>,
}

impl FileStore {
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir).with_context(|| format!("receipt dir {dir:?}"))?;
        let mut map = HashMap::new();
        if let Ok(rd) = fs::read_dir(&dir) {
            for ent in rd.flatten() {
                let path = ent.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    continue;
                }
                let Ok(raw) = fs::read_to_string(&path) else { continue };
                if let Ok(row) = serde_json::from_str::<Stored>(&raw) {
                    if let Some(k) = row.receipt.key() {
                        map.insert(k, row);
                    }
                }
            }
        }
        Ok(Self {
            dir,
            inner: Arc::new(Mutex::new(map)),
        })
    }

    pub fn get(&self, key: &str) -> Option<Stored> {
        self.inner.lock().expect("store").get(key).cloned()
    }

    pub fn put(&self, key: &str, row: Stored) -> Result<()> {
        let path = self.path_for(key);
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(&row)?;
        fs::write(&tmp, &bytes)?;
        fs::rename(&tmp, &path).or_else(|_| {
            fs::remove_file(&path).ok();
            fs::rename(&tmp, &path)
        })?;
        self.inner.lock().expect("store").insert(key.to_string(), row);
        Ok(())
    }

    fn path_for(&self, key: &str) -> PathBuf {
        let safe: String = key
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        self.dir.join(format!("{safe}.json"))
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let d = Sha256::digest(bytes);
    d.iter().map(|b| format!("{b:02x}")).collect()
}
