//! Keeper heartbeat and notify log. Payload is `market_id` only (FR-NTF-01).

use serde::{Deserialize, Serialize};
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Heartbeat {
    pub slot: u64,
    pub ts: i64,
    pub last: String,
    #[serde(default)]
    pub market: String,
    pub ok: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Event {
    pub ts: i64,
    pub kind: String,
    pub market: String,
}

pub fn write_heartbeat(path: impl AsRef<Path>, hb: &Heartbeat) -> Result<()> {
    let path = path.as_ref();
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(hb)?)?;
    fs::rename(&tmp, path).or_else(|_| {
        fs::remove_file(path).ok();
        fs::rename(&tmp, path)
    })?;
    Ok(())
}

pub fn read_heartbeat(path: impl AsRef<Path>) -> Result<Option<Heartbeat>> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&fs::read_to_string(path)?)?))
}

pub fn notify_path(dir: impl AsRef<Path>) -> PathBuf {
    dir.as_ref().join("events.jsonl")
}

pub fn append_event(dir: impl AsRef<Path>, ev: &Event) -> Result<()> {
    let dir = dir.as_ref();
    fs::create_dir_all(dir)?;
    let path = notify_path(dir);
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(f, "{}", serde_json::to_string(ev)?)?;
    f.flush()?;
    Ok(())
}

pub fn last_events(dir: impl AsRef<Path>, limit: usize) -> Result<Vec<Event>> {
    let path = notify_path(dir);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for line in BufReader::new(fs::File::open(path)?).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(&line)?);
    }
    if rows.len() > limit {
        rows.drain(0..rows.len() - limit);
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("cpm-notify-{n}"))
    }

    #[test]
    fn heartbeat_roundtrip() {
        let dir = tmp();
        let path = dir.join("hb.json");
        let hb = Heartbeat {
            slot: 9,
            ts: 100,
            last: "commit".into(),
            market: "Mkt1".into(),
            ok: true,
        };
        write_heartbeat(&path, &hb).unwrap();
        assert_eq!(read_heartbeat(&path).unwrap(), Some(hb));
    }

    #[test]
    fn notify_is_market_id_only() {
        let dir = tmp();
        append_event(
            &dir,
            &Event {
                ts: 1,
                kind: "close".into(),
                market: "MktAAA".into(),
            },
        )
        .unwrap();
        let rows = last_events(&dir, 10).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].market, "MktAAA");
        let raw = serde_json::to_value(&rows[0]).unwrap();
        assert!(raw.get("email").is_none());
        assert!(raw.get("owner").is_none());
    }
}
