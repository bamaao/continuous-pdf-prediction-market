//! Local fill ticket journal — frozen $S$ for claim (mirrors web `cpm.tickets.*`).
//! Not a secret. Path: `$CPM_TICKETS_PATH` or `~/.cpm/tickets.json`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TicketRow {
    pub market: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<String>,
    pub shares: i64,
    pub ts: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skellam_kind: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub a: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub b: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nonce: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sig: Option<String>,
}

pub fn path() -> PathBuf {
    if let Ok(p) = std::env::var("CPM_TICKETS_PATH") {
        return PathBuf::from(p);
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".cpm").join("tickets.json")
}

fn load_map(path: &Path) -> HashMap<String, Vec<TicketRow>> {
    let Ok(raw) = fs::read_to_string(path) else {
        return HashMap::new();
    };
    serde_json::from_str(&raw).unwrap_or_default()
}

fn save_map(path: &Path, map: &HashMap<String, Vec<TicketRow>>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("tickets dir {parent:?}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(map)?;
    fs::write(&tmp, &bytes)?;
    fs::rename(&tmp, path).or_else(|_| {
        fs::remove_file(path).ok();
        fs::rename(&tmp, path)
    })?;
    Ok(())
}

fn now_ts() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn hex32(bytes: &[u8; 32]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn remember(owner: &str, mut row: TicketRow) -> Result<PathBuf> {
    let path = path();
    row.ts = now_ts();
    let mut map = load_map(&path);
    let rows = map.entry(owner.to_string()).or_default();
    rows.push(row);
    if rows.len() > 200 {
        let drop = rows.len() - 200;
        rows.drain(0..drop);
    }
    save_map(&path, &map)?;
    Ok(path)
}

pub fn remember_mask(
    owner: &str,
    market: &str,
    mask_hex: &str,
    set_hash: [u8; 32],
    shares: i64,
    nonce: u64,
    sig: &str,
) -> Result<PathBuf> {
    remember(
        owner,
        TicketRow {
            market: market.to_string(),
            mask: Some(mask_hex.trim_start_matches("0x").to_ascii_lowercase()),
            shares,
            ts: 0,
            kind: Some("mask".into()),
            skellam_kind: None,
            a: None,
            b: None,
            set_hash: Some(hex32(&set_hash)),
            nonce: Some(nonce),
            sig: Some(sig.to_string()),
        },
    )
}

pub fn remember_skellam(
    owner: &str,
    market: &str,
    kind: u8,
    a: i16,
    b: i16,
    set_hash: [u8; 32],
    shares: i64,
    nonce: u64,
    sig: &str,
) -> Result<PathBuf> {
    remember(
        owner,
        TicketRow {
            market: market.to_string(),
            mask: None,
            shares,
            ts: 0,
            kind: Some("skellam".into()),
            skellam_kind: Some(kind),
            a: Some(i64::from(a)),
            b: Some(i64::from(b)),
            set_hash: Some(hex32(&set_hash)),
            nonce: Some(nonce),
            sig: Some(sig.to_string()),
        },
    )
}

pub fn list(owner: &str) -> Vec<TicketRow> {
    load_map(&path()).get(owner).cloned().unwrap_or_default()
}

/// Newest ticket for `(owner, market)` with a mask or typed Skellam.
pub fn last_for(owner: &str, market: &str) -> Option<TicketRow> {
    let rows = list(owner);
    rows.into_iter().rev().find(|r| {
        r.market == market
            && (r.mask.as_ref().map(|m| !m.is_empty()).unwrap_or(false) || r.skellam_kind.is_some())
    })
}

pub fn for_hash(owner: &str, market: &str, set_hash: &str) -> Option<TicketRow> {
    let want = set_hash.trim_start_matches("0x").to_ascii_lowercase();
    let rows = list(owner);
    for r in rows.into_iter().rev() {
        if r.market != market {
            continue;
        }
        let h = r
            .set_hash
            .as_deref()
            .unwrap_or("")
            .trim_start_matches("0x")
            .to_ascii_lowercase();
        if !h.is_empty() && h == want {
            return Some(r);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remember_and_lookup() {
        let dir = std::env::temp_dir().join(format!(
            "cpm-tickets-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let file = dir.join("tickets.json");
        std::env::set_var("CPM_TICKETS_PATH", &file);
        let hash = [0xabu8; 32];
        remember_mask("Own1", "Mkt1", "ff", hash, 3, 1, "sig").unwrap();
        let got = for_hash("Own1", "Mkt1", &hex32(&hash)).unwrap();
        assert_eq!(got.mask.as_deref(), Some("ff"));
        assert_eq!(last_for("Own1", "Mkt1").unwrap().shares, 3);
        let _ = fs::remove_dir_all(&dir);
        std::env::remove_var("CPM_TICKETS_PATH");
    }
}
