//! Append-only fill journal. Two directories that must not share a process
//! (FR-DUR-03): replica log + object-store append. L1 `trades_root` is a
//! checkpoint of this chain, not whether a fill exists (FR-DUR-02).

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("journal fork: expected prev_root {expected} got {got}")]
    Fork { expected: String, got: String },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct FillRecord {
    pub seq: u64,
    pub market: String,
    pub owner: String,
    pub nonce: u64,
    pub sig: String,
    pub prev_root: String,
    pub root: String,
}

#[derive(Clone)]
pub struct Journal {
    replica: PathBuf,
    object: PathBuf,
}

impl Journal {
    pub fn open(replica: impl AsRef<Path>, object: impl AsRef<Path>) -> Result<Self> {
        let replica = replica.as_ref().to_path_buf();
        let object = object.as_ref().to_path_buf();
        fs::create_dir_all(&replica)?;
        fs::create_dir_all(&object)?;
        Ok(Self { replica, object })
    }

    pub fn path_for(dir: &Path, market: &str) -> PathBuf {
        let safe: String = market
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        dir.join(format!("{safe}.jsonl"))
    }

    pub fn replay_file(path: &Path) -> Result<(Vec<FillRecord>, [u8; 32])> {
        if !path.exists() {
            return Ok((Vec::new(), [0u8; 32]));
        }
        let f = fs::File::open(path)?;
        let mut rows = Vec::new();
        let mut prev = [0u8; 32];
        for line in BufReader::new(f).lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let rec: FillRecord = serde_json::from_str(&line)?;
            let want = hex(&prev);
            if rec.prev_root != want {
                return Err(Error::Fork {
                    expected: want,
                    got: rec.prev_root,
                });
            }
            let body = canonical_body(&rec);
            let root = chain(prev, &body);
            if rec.root != hex(&root) {
                return Err(Error::Fork {
                    expected: hex(&root),
                    got: rec.root,
                });
            }
            prev = root;
            rows.push(rec);
        }
        Ok((rows, prev))
    }

    pub fn replay(&self, market: &str) -> Result<(Vec<FillRecord>, [u8; 32])> {
        let a = Self::replay_file(&Self::path_for(&self.replica, market));
        let b = Self::replay_file(&Self::path_for(&self.object, market));
        match (a, b) {
            (Ok(ra), Ok(rb)) => Ok(if ra.0.len() >= rb.0.len() { ra } else { rb }),
            (Ok(ra), Err(_)) => Ok(ra),
            (Err(_), Ok(rb)) => Ok(rb),
            (Err(e), Err(_)) => Err(e),
        }
    }

    pub fn trades_root(&self, market: &str) -> Result<[u8; 32]> {
        Ok(self.replay(market)?.1)
    }

    /// Base58 market ids that have a replica or object-store log.
    pub fn listed_markets(&self) -> Vec<String> {
        let mut names = std::collections::BTreeSet::new();
        for dir in [&self.replica, &self.object] {
            let Ok(rd) = fs::read_dir(dir) else { continue };
            for ent in rd.flatten() {
                let p = ent.path();
                if p.extension().and_then(|s| s.to_str()) != Some("jsonl") {
                    continue;
                }
                if let Some(stem) = p.file_stem().and_then(|s| s.to_str()) {
                    names.insert(stem.to_string());
                }
            }
        }
        names.into_iter().collect()
    }

    /// Append to both copies. Returns the new `trades_root`.
    pub fn append(
        &self,
        market: &str,
        owner: &str,
        nonce: u64,
        sig: &str,
    ) -> Result<[u8; 32]> {
        let (rows, prev) = self.replay(market)?;
        let seq = rows.len() as u64 + 1;
        let mut rec = FillRecord {
            seq,
            market: market.to_string(),
            owner: owner.to_string(),
            nonce,
            sig: sig.to_string(),
            prev_root: hex(&prev),
            root: String::new(),
        };
        let root = chain(prev, &canonical_body(&rec));
        rec.root = hex(&root);
        let line = serde_json::to_string(&rec)?;
        for dir in [&self.replica, &self.object] {
            let path = Self::path_for(dir, market);
            let mut f = OpenOptions::new().create(true).append(true).open(&path)?;
            writeln!(f, "{line}")?;
            f.flush()?;
        }
        Ok(root)
    }
}

fn canonical_body(rec: &FillRecord) -> Vec<u8> {
    format!("{}|{}|{}|{}|{}", rec.seq, rec.market, rec.owner, rec.nonce, rec.sig).into_bytes()
}

fn chain(prev: [u8; 32], body: &[u8]) -> [u8; 32] {
    let mut h = Sha256::new();
    h.update(prev);
    h.update(body);
    h.finalize().into()
}

pub fn hex(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn parse_hex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp() -> (PathBuf, PathBuf) {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let base = std::env::temp_dir().join(format!("cpm-journal-{n}"));
        (base.join("replica"), base.join("object"))
    }

    #[test]
    fn append_replay_same_root() {
        let (r, o) = tmp();
        let j = Journal::open(&r, &o).unwrap();
        let a = j.append("Mkt111", "Own111", 1, "sigA").unwrap();
        let b = j.append("Mkt111", "Own111", 2, "sigB").unwrap();
        assert_ne!(a, b);
        assert_ne!(a, [0u8; 32]);
        let (rows, root) = j.replay("Mkt111").unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(root, b);
        assert_eq!(j.trades_root("Mkt111").unwrap(), b);
    }

    #[test]
    fn kill_replica_replays_from_object_store() {
        let (r, o) = tmp();
        let j = Journal::open(&r, &o).unwrap();
        let want = j.append("Mkt222", "Own222", 7, "sigC").unwrap();
        fs::remove_dir_all(&r).unwrap();
        let j2 = Journal::open(&r, &o).unwrap();
        let (rows, root) = j2.replay("Mkt222").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(root, want);
        assert_eq!(rows[0].nonce, 7);
    }
}
