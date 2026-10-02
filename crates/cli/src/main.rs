//! FR-CLI-01. Instruction bytes come from `crates/client` (program crates), not a second IDL.

use anyhow::{Context, Result};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use clap::{Parser, Subcommand};
use client::math::{usdc, Q64};
use solana_client::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair_file, write_keypair_file, Keypair, Signer};
use solana_sdk::transaction::Transaction;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

const TOKEN_PROGRAM: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

#[derive(Parser)]
#[command(name = "cpm", about = "Continuous PDF prediction market CLI")]
struct Opt {
    #[arg(long, default_value = "http://127.0.0.1:8899")]
    url: String,
    /// ER RPC. Empty = send commit/undelegate to `--url` (L1 / program-test).
    #[arg(long, default_value = "")]
    er_url: String,
    #[arg(long)]
    keypair: Option<PathBuf>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create the vault config (Circle USDC mint must already exist on the cluster).
    VaultInit,
    Deposit {
        amount: u64,
    },
    /// Localnet only: mint fixture USDC into the payer ATA.
    Faucet {
        amount: u64,
        #[arg(long)]
        mint_authority: Option<PathBuf>,
    },
    /// Write a local validator `--account` fixture at the Circle USDC mint address.
    WriteLocalMint {
        #[arg(long)]
        authority: PathBuf,
        #[arg(long)]
        account: PathBuf,
    },
    Withdraw {
        amount: u64,
    },
    #[command(subcommand)]
    Session(SessionCmd),
    #[command(subcommand)]
    Market(MarketCmd),
    #[command(subcommand)]
    Trade(TradeCmd),
    #[command(subcommand)]
    Risk(RiskCmd),
    #[command(subcommand)]
    Resolve(ResolveCmd),
    #[command(subcommand)]
    Committee(CommitteeCmd),
    #[command(subcommand)]
    Settle(SettleCmd),
    /// R-KEEP write path (FR-UI-28): Commit, close/undelegate, heartbeat.
    Keeper {
        #[arg(long)]
        once: bool,
        #[arg(long, default_value_t = 15)]
        interval: u64,
        #[arg(long)]
        journal_replica: Option<PathBuf>,
        #[arg(long)]
        journal_object: Option<PathBuf>,
        #[arg(long)]
        heartbeat: Option<PathBuf>,
        #[arg(long)]
        notify_dir: Option<PathBuf>,
    },
    /// RPC slot, plus Market API health when `MARKET_API` is set.
    IndexStatus,
}

#[derive(Subcommand)]
enum MarketCmd {
    CreateGaussian {
        topic: String,
        tag: String,
        #[arg(long, default_value_t = 8)]
        n: u16,
        /// Seconds from now until close. Ignored when `--close-ts` is set.
        #[arg(long, default_value_t = 300)]
        close_in: i64,
        #[arg(long)]
        close_ts: Option<i64>,
        #[arg(long, default_value_t = 20)]
        challenge_secs: i64,
    },
    CreateLognormal {
        topic: String,
        tag: String,
        #[arg(long, default_value_t = 8)]
        n: u16,
        #[arg(long, default_value_t = 300)]
        close_in: i64,
        #[arg(long)]
        close_ts: Option<i64>,
        #[arg(long, default_value_t = 20)]
        challenge_secs: i64,
    },
    CreateDirichlet {
        topic: String,
        #[arg(long, default_value_t = 3)]
        n: u16,
        #[arg(long, default_value_t = 300)]
        close_in: i64,
        #[arg(long)]
        close_ts: Option<i64>,
        #[arg(long, default_value_t = 20)]
        challenge_secs: i64,
    },
    CreateBernoulli {
        topic: String,
        tag: String,
        #[arg(long, default_value_t = 300)]
        close_in: i64,
        #[arg(long)]
        close_ts: Option<i64>,
        #[arg(long, default_value_t = 20)]
        challenge_secs: i64,
    },
    CreateSkellam {
        topic: String,
        #[arg(long, default_value_t = 0)]
        score_scope: u8,
        #[arg(long, default_value_t = 300)]
        close_in: i64,
        #[arg(long)]
        close_ts: Option<i64>,
        #[arg(long, default_value_t = 20)]
        challenge_secs: i64,
    },
    /// Halt trading. If the board is delegated, halt on ER then undelegate.
    Close {
        market: String,
        #[arg(long)]
        mask: Option<String>,
        #[arg(long)]
        skellam_kind: Option<u8>,
        #[arg(long, default_value_t = 0)]
        skellam_a: i16,
        #[arg(long, default_value_t = 0)]
        skellam_b: i16,
    },
    Delegate { market: String },
    Commit {
        market: String,
        /// 64-char hex journal `trades_root`.
        root: String,
    },
    Undelegate {
        market: String,
        #[arg(long)]
        mask: Option<String>,
        #[arg(long)]
        skellam_kind: Option<u8>,
        #[arg(long, default_value_t = 0)]
        skellam_a: i16,
        #[arg(long, default_value_t = 0)]
        skellam_b: i16,
    },
    /// Dump the trading-implied PDF $p_k$ and face $E_k$ (they are not the same).
    Pdf { market: String },
    /// Public desk: traders, stake, L_max, C_R, then the implied PDF.
    Info { market: String },
    /// On-chain θ → same $p_S$ / $C_S$ / coverage / $\hat\rho$ / pre-bet ticket as Market API.
    Quote {
        market: String,
        #[arg(long, default_value = "01")]
        mask: String,
        /// Skellam / football typed line (home/draw/away/totals/…). Overrides `--mask`.
        #[arg(long)]
        kind: Option<u8>,
        #[arg(long, default_value_t = 0)]
        a: i16,
        #[arg(long, default_value_t = 0)]
        b: i16,
        #[arg(long, default_value_t = 1)]
        shares: i64,
    },
}

#[derive(Subcommand)]
enum SessionCmd {
    Open {
        #[arg(long)]
        authority: PathBuf,
        #[arg(long, default_value_t = 2)]
        hours: i64,
        #[arg(long)]
        usdc: u64,
        #[arg(long)]
        market: Option<String>,
    },
    Renew {
        #[arg(long, default_value_t = 2)]
        hours: i64,
        #[arg(long)]
        usdc: u64,
    },
    Revoke,
}

#[derive(Subcommand)]
enum TradeCmd {
    BuySet {
        market: String,
        /// Hex bit mask, e.g. 01 for cell 0 when n≤8.
        mask: String,
        /// Integer shares (Q64 integer part).
        shares: i64,
        #[arg(long, default_value_t = 0)]
        nonce: u64,
        #[arg(long)]
        session: Option<PathBuf>,
        #[arg(long)]
        gateway: Option<String>,
    },
    SellSet {
        market: String,
        mask: String,
        shares: i64,
        #[arg(long, default_value_t = 0)]
        nonce: u64,
        #[arg(long)]
        session: Option<PathBuf>,
        #[arg(long)]
        gateway: Option<String>,
    },
    BuySkellam {
        market: String,
        shares: i64,
        /// 0 Home / 1 Draw / 2 Away / 3 Over / 4 Under / 5 BTTS-Y / 6 BTTS-N / 7 Exact …
        #[arg(long, default_value_t = 0)]
        kind: u8,
        #[arg(long, default_value_t = 0)]
        a: i16,
        #[arg(long, default_value_t = 0)]
        b: i16,
        #[arg(long, default_value_t = 0)]
        nonce: u64,
        #[arg(long)]
        session: Option<PathBuf>,
        #[arg(long)]
        gateway: Option<String>,
    },
    SellSkellam {
        market: String,
        shares: i64,
        #[arg(long, default_value_t = 0)]
        kind: u8,
        #[arg(long, default_value_t = 0)]
        a: i16,
        #[arg(long, default_value_t = 0)]
        b: i16,
        #[arg(long, default_value_t = 0)]
        nonce: u64,
        #[arg(long)]
        session: Option<PathBuf>,
        #[arg(long)]
        gateway: Option<String>,
    },
}

#[derive(Subcommand)]
enum RiskCmd {
    Open { market: String },
    Bid {
        market: String,
        #[arg(long, default_value_t = 1)]
        layer: u8,
        capacity: u64,
        premium: u64,
        #[arg(long, default_value_t = 7000)]
        profit_share_bps: u16,
    },
}

#[derive(Subcommand)]
enum CommitteeCmd {
    /// Create the one protocol-wide committee PDA. Create-market only references it.
    Init {
        /// Comma-separated member pubkeys. Defaults to the payer.
        #[arg(long)]
        members: Option<String>,
        #[arg(long, default_value_t = 1)]
        m: u8,
    },
    /// Replace the live roster. In-flight votes keep the snapshot taken at resolve_open.
    Set {
        #[arg(long)]
        members: String,
        #[arg(long, default_value_t = 1)]
        m: u8,
    },
}

#[derive(Subcommand)]
enum ResolveCmd {
    Open { market: String },
    Submit {
        market: String,
        /// Integer part of $x^*$ / atom / yes-no / home score (see `--family`).
        value: i64,
        /// 0 Skellam 1 Gaussian 2 Lognormal 3 Dirichlet 4 Bernoulli.
        #[arg(long, default_value_t = 1)]
        family: u8,
        /// 0 score pair, 1 scalar, 2 dirichlet atom, 4 yes/no.
        #[arg(long, default_value_t = 1)]
        kind: u8,
        /// Away score (Skellam) or unused.
        #[arg(long, default_value_t = 0)]
        b: i64,
    },
    Finalize { market: String },
}

#[derive(Subcommand)]
enum SettleCmd {
    Begin {
        market: String,
        #[arg(long)]
        risk: bool,
        #[arg(long)]
        pool: bool,
    },
    Payout {
        market: String,
        mask: String,
        #[arg(long)]
        owner: Option<String>,
    },
    PayoutSkellam {
        market: String,
        #[arg(long, default_value_t = 0)]
        kind: u8,
        #[arg(long, default_value_t = 0)]
        a: i16,
        #[arg(long, default_value_t = 0)]
        b: i16,
        #[arg(long)]
        owner: Option<String>,
    },
    /// Open the board vault (amount is always 0).
    FundCm {
        market: String,
    },
    /// After undelegate, debit the L1 user vault for ER fills on this set.
    SyncVault {
        market: String,
        mask: String,
    },
}

fn pad32(s: &str) -> [u8; 32] {
    let mut o = [b'_'; 32];
    let b = s.as_bytes();
    o[..b.len().min(32)].copy_from_slice(&b[..b.len().min(32)]);
    o
}

fn parse_members(raw: &str, fallback: Option<Pubkey>) -> Result<Vec<Pubkey>> {
    let list = raw
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| Pubkey::from_str(s).context("committee member"))
        .collect::<Result<Vec<_>>>()?;
    if list.is_empty() {
        if let Some(one) = fallback {
            return Ok(vec![one]);
        }
        anyhow::bail!("committee members required");
    }
    Ok(list)
}

fn parse_mask(hex: &str) -> Result<Vec<u8>> {
    let h = hex.trim().trim_start_matches("0x");
    if h.len() % 2 != 0 {
        anyhow::bail!("mask hex must be even length");
    }
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).context("mask hex"))
        .collect()
}

fn mask_bytes(hex: &str, n: u16) -> Result<Vec<u8>> {
    quote::pad_mask(&parse_mask(hex)?, n as usize).map_err(|e| anyhow::anyhow!("{e}"))
}

fn parse_root(hex: &str) -> Result<[u8; 32]> {
    let h = hex.trim().trim_start_matches("0x");
    if h.len() != 64 {
        anyhow::bail!("trades_root must be 64 hex chars");
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).context("trades_root hex")?;
    }
    Ok(out)
}

fn load_grid_parts(rpc: &RpcClient, market: &Pubkey, n: u16) -> Result<Vec<client::market::state::Grid>> {
    let mut parts = Vec::new();
    for ix in 0..client::market::state::Grid::shard_count(n) {
        let acc = rpc
            .get_account(&client::grid_shard_pda(market, ix as u8))
            .with_context(|| format!("grid shard {ix}"))?;
        parts.push(client::decode_grid(&acc.data).map_err(|e| anyhow::anyhow!("{e}"))?);
    }
    Ok(parts)
}

fn load_book(rpc: &RpcClient, market: &Pubkey) -> Result<(quote::Book, u16, u16)> {
    let mkt = client::decode_market(&rpc.get_account(market).context("market")?.data)
        .map_err(|e| anyhow::anyhow!(e))?;
    let parts = load_grid_parts(rpc, market, mkt.n)?;
    let (p0, theta, exposure, _) = client::concat_grid_shards(&parts);
    let mut trading_revenue = mkt.trading_revenue;
    let mut premium_payable = 0u64;
    if let Ok(bacc) = rpc.get_account(&client::board(market)) {
        if let Ok(b) = client::decode_board(&bacc.data) {
            trading_revenue = b.trading_revenue;
            premium_payable = b.premium_payable;
        }
    }
    let c_r = rpc
        .get_account(&client::risk_book(market))
        .ok()
        .and_then(|a| client::decode_risk_book(&a.data).ok())
        .map(|b| b.c_r)
        .unwrap_or(0);
    let slot = rpc.get_slot().unwrap_or(0);
    Ok((
        quote::Book::from_grid(
            mkt.beta,
            &p0,
            &theta,
            &exposure,
            trading_revenue,
            premium_payable,
            0,
            c_r,
            slot,
        ),
        mkt.n,
        mkt.fee_bps,
    ))
}

fn default_keypair() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".config/solana/id.json");
    }
    if let Some(home) = std::env::var_os("USERPROFILE") {
        return PathBuf::from(home).join(".config").join("solana").join("id.json");
    }
    PathBuf::from("id.json")
}

fn default_mint_authority() -> PathBuf {
    PathBuf::from("fixtures/usdc-mint-authority.json")
}

fn load_kp(path: &Option<PathBuf>) -> Result<Keypair> {
    let p = path.clone().unwrap_or_else(default_keypair);
    read_keypair_file(&p).map_err(|e| anyhow::anyhow!("keypair {}: {e}", p.display()))
}

fn wall_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
}

fn plan_close(close_in: i64, close_ts: Option<i64>) -> Result<(i64, i64)> {
    let close = close_ts.unwrap_or_else(|| wall_unix() + close_in);
    if close <= wall_unix() {
        anyhow::bail!("close_ts must be in the future");
    }
    Ok((close, close))
}

fn common(
    me: Pubkey,
    id_hash: [u8; 32],
    n: u16,
    close_ts: i64,
    risk_lock_ts: i64,
    challenge_secs: i64,
) -> client::market::state::CreateCommon {
    client::market::state::CreateCommon {
        id_hash,
        n,
        close_ts,
        risk_lock_ts,
        beta: Q64::from_int(100).raw(),
        c_m: 0,
        fee_bps: 0,
        fee_timing: 0,
        authorized_reporter: Pubkey::default(),
        report_window_secs: 400,
        challenge_secs,
        n_layers: 1,
        d_unit: 10,
        gamma_bps: 1_000,
        alpha_r_bps: 7_000,
        platform: me,
    }
}

fn next_nonce(rpc: &RpcClient, owner: &Pubkey, market: &Pubkey, explicit: u64) -> Result<u64> {
    if explicit > 0 {
        return Ok(explicit);
    }
    let acc = match rpc.get_account(&client::nonce_pda(owner, market)) {
        Ok(a) => a,
        Err(_) => return Ok(1),
    };
    match client::decode_nonce(&acc.data) {
        Ok(n) => Ok(n.last.saturating_add(1)),
        Err(_) => Ok(1),
    }
}

fn send_via_gateway(
    gateway: &str,
    url: &str,
    payer: &Keypair,
    ix: solana_sdk::instruction::Instruction,
    extra: &[&Keypair],
    owner: Pubkey,
    market: Pubkey,
    nonce: u64,
) -> Result<String> {
    let rpc = RpcClient::new_with_commitment(url.to_string(), CommitmentConfig::confirmed());
    let bh = rpc.get_latest_blockhash()?;
    let heap = ComputeBudgetInstruction::request_heap_frame(256 * 1024);
    let cu = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    let mut signers: Vec<&Keypair> = vec![payer];
    for s in extra {
        if s.pubkey() != payer.pubkey() {
            signers.push(*s);
        }
    }
    let tx = Transaction::new_signed_with_payer(&[heap, cu, ix], Some(&payer.pubkey()), &signers, bh);
    let tx_b64 = B64.encode(bincode::serialize(&tx)?);
    let body = serde_json::json!({
        "tx_b64": tx_b64,
        "owner": owner.to_string(),
        "market": market.to_string(),
        "nonce": nonce,
    });
    let endpoint = format!("{}/v1/submit", gateway.trim_end_matches('/'));
    let resp: serde_json::Value = match ureq::post(&endpoint).send_json(body) {
        Ok(r) => r.into_json()?,
        Err(ureq::Error::Status(code, r)) => {
            let t = r.into_string().unwrap_or_default();
            anyhow::bail!("gateway {code}: {t}");
        }
        Err(e) => return Err(e.into()),
    };
    let status = resp.get("status").and_then(|v| v.as_str()).unwrap_or("");
    if status == "confirmed" {
        return Ok(resp.get("sig").and_then(|v| v.as_str()).unwrap_or("").to_string());
    }
    if status != "pending" {
        anyhow::bail!("gateway {resp}");
    }
    eprintln!("pending nonce={nonce}");
    let receipt_url = format!(
        "{}/v1/receipt?owner={owner}&market={market}&nonce={nonce}",
        gateway.trim_end_matches('/')
    );
    for _ in 0..150 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let rec: serde_json::Value = match ureq::get(&receipt_url).call() {
            Ok(r) => r.into_json()?,
            Err(_) => continue,
        };
        match rec.get("status").and_then(|v| v.as_str()).unwrap_or("") {
            "confirmed" => {
                return Ok(rec.get("sig").and_then(|v| v.as_str()).unwrap_or("").to_string());
            }
            "failed" => anyhow::bail!("gateway receipt failed: {rec}"),
            _ => {}
        }
    }
    anyhow::bail!("gateway receipt still pending after timeout nonce={nonce}")
}

fn account_delegated(rpc: &RpcClient, pk: &Pubkey) -> bool {
    rpc.get_account(pk)
        .ok()
        .map(|a| a.owner == client::market::DELEGATION_PROGRAM_ID)
        .unwrap_or(false)
}

fn market_is_delegated(rpc: &RpcClient, market: &Pubkey) -> bool {
    let Ok(acc) = rpc.get_account(market) else {
        return false;
    };
    if acc.owner == client::market::DELEGATION_PROGRAM_ID {
        return true;
    }
    client::decode_market(&acc.data)
        .map(|m| m.delegated)
        .unwrap_or(false)
}

fn ensure_session(url: &str, kp: &Keypair, owner: Pubkey) -> Result<()> {
    if account_delegated(
        &RpcClient::new_with_commitment(url.to_string(), CommitmentConfig::confirmed()),
        &client::session_pda(&owner),
    ) {
        return Ok(());
    }
    let sig = send(url, kp, client::delegate_session(owner))?;
    println!("delegate_session {sig}");
    Ok(())
}

fn undelegate_seat_on_er(
    l1: &str,
    er: &str,
    kp: &Keypair,
    owner: Pubkey,
    market: Pubkey,
    set_hash: [u8; 32],
) -> Result<()> {
    let er_url = er_rpc(l1, er);
    let pos = client::position_pda(&market, &owner, &set_hash);
    let nonce = client::nonce_pda(&owner, &market);
    if account_delegated(
        &RpcClient::new_with_commitment(l1.to_string(), CommitmentConfig::confirmed()),
        &pos,
    ) {
        let sig = send(er_url, kp, client::undelegate_seat(kp.pubkey(), owner, market, set_hash))?;
        println!("undelegate_seat {sig}");
        wait_owned_by_market(l1, &[pos, nonce], 80)?;
    }
    Ok(())
}

fn undelegate_seats_for_market(l1: &str, er: &str, kp: &Keypair, market: Pubkey) -> Result<()> {
    if er.is_empty() {
        return Ok(());
    }
    let er_rpc_c = RpcClient::new_with_commitment(er.to_string(), CommitmentConfig::confirmed());
    let Ok(rows) = er_rpc_c.get_program_accounts(&client::market::ID) else {
        return Ok(());
    };
    for (_, acc) in rows {
        let Ok(pos) = client::decode_position(&acc.data) else { continue };
        if pos.market != market {
            continue;
        }
        if let Err(e) = undelegate_seat_on_er(l1, er, kp, pos.owner, market, pos.set_hash) {
            eprintln!("undelegate_seat {} {market}: {e}", pos.owner);
        }
    }
    Ok(())
}

fn sync_vault_on_l1(
    l1: &str,
    kp: &Keypair,
    owner: Pubkey,
    market: Pubkey,
    set_hash: [u8; 32],
) -> Result<()> {
    let rpc = RpcClient::new_with_commitment(l1.to_string(), CommitmentConfig::confirmed());
    let pos = client::position_pda(&market, &owner, &set_hash);
    for _ in 0..40 {
        let pos_ok = rpc
            .get_account(&pos)
            .ok()
            .map(|a| a.owner == client::market::ID)
            .unwrap_or(false);
        let mkt_ok = rpc
            .get_account(&market)
            .ok()
            .map(|a| a.owner == client::market::ID)
            .unwrap_or(false);
        if pos_ok && mkt_ok {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    wait_owned_by_market(l1, &[pos, market], 80)?;
    let sig = send(l1, kp, client::sync_vault(kp.pubkey(), owner, market, set_hash))?;
    println!("sync_vault {sig}");
    Ok(())
}

fn rpc_market_n(url: &str, market: &Pubkey) -> Result<u16> {
    let rpc = RpcClient::new_with_commitment(url.to_string(), CommitmentConfig::confirmed());
    let mkt = client::decode_market(&rpc.get_account(market).context("market")?.data)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(mkt.n)
}

fn intent_already_queued(s: &str) -> bool {
    s.contains("NotDelegated")
        || s.contains("6025")
        || s.contains("0xbbf")
        || s.contains("0xBBF")
}

fn send_undelegate_shard(er: &str, kp: &Keypair, me: Pubkey, shard: Pubkey) -> Result<()> {
    match send(er, kp, client::undelegate_shard(me, shard)) {
        Ok(sig) => {
            println!("undelegate_shard {sig} shard={shard}");
            Ok(())
        }
        Err(e) => {
            if intent_already_queued(&e.to_string()) {
                Ok(())
            } else {
                Err(e)
            }
        }
    }
}

fn undelegate_shard_until_home(
    er: &str,
    l1: &str,
    kp: &Keypair,
    me: Pubkey,
    shard: Pubkey,
    label: &str,
    tries: u32,
) -> Result<()> {
    if account_owned_by_market(l1, &shard) {
        println!("home {label}={shard}");
        return Ok(());
    }
    // 16-cell shards are 1101B and need the committor writeback buffer.
    // Do not create the SDK Delegate buffer ["buffer", source]: that PDA is
    // not the committor buffer and pushed local writeback onto a crash.
    send_undelegate_shard(er, kp, me, shard)?;
    for t in 0..tries {
        if account_owned_by_market(l1, &shard) {
            println!("home {label}={shard}");
            return Ok(());
        }
        if t > 0 && t % 8 == 0 {
            if let Err(e) = send_undelegate_shard(er, kp, me, shard) {
                let s = e.to_string();
                if !intent_already_queued(&s) {
                    eprintln!("retry undelegate_shard {label}: {s}");
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    anyhow::bail!("{label} {shard} still not home after undelegate")
}

fn undelegate_extra_shards(er: &str, l1: &str, kp: &Keypair, me: Pubkey, market: Pubkey, n: u16) -> Result<()> {
    for ix in 1..client::market::state::Grid::shard_count(n) {
        let shard = client::grid_shard_pda(&market, ix as u8);
        undelegate_shard_until_home(er, l1, kp, me, shard, &format!("shard{ix}"), 80)?;
    }
    Ok(())
}

fn wait_book_home(l1: &str, er: &str, kp: &Keypair, me: Pubkey, market: Pubkey, n: u16) -> Result<()> {
    let grid = client::grid_pda(&market);
    for t in 0..240u32 {
        if account_owned_by_market(l1, &market) {
            break;
        }
        if t > 0 && t % 8 == 0 {
            match send(er, kp, client::undelegate_book(me, market)) {
                Ok(sig) => eprintln!("retry undelegate_book {sig}"),
                Err(e) => {
                    let s = e.to_string();
                    if !intent_already_queued(&s) {
                        eprintln!("retry undelegate_book: {s}");
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
        if t + 1 == 240 && !account_owned_by_market(l1, &market) {
            anyhow::bail!("L1 market still not home after undelegate");
        }
    }
    if !account_owned_by_market(l1, &grid) {
        undelegate_shard_until_home(er, l1, kp, me, grid, "shard0", 240)?;
    } else {
        println!("home shard0={grid}");
    }
    undelegate_extra_shards(er, l1, kp, me, market, n)?;
    Ok(())
}

fn settle_grid_of(rpc: &RpcClient, market: Pubkey) -> Result<Pubkey> {
    let mkt = client::decode_market(&rpc.get_account(&market).context("market")?.data)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let rec = client::decode_resolution(
        &rpc.get_account(&client::record_pda(&market))
            .context("record")?
            .data,
    )
    .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(client::winning_shard(
        market,
        mkt.n,
        rec.family,
        rec.k_max,
        mkt.extra.a,
        mkt.extra.b,
        rec.final_outcome.kind,
        rec.final_outcome.a,
        rec.final_outcome.b,
    ))
}

fn wait_owned_by_market(l1: &str, pks: &[Pubkey], tries: u32) -> Result<()> {
    let rpc = RpcClient::new_with_commitment(l1.to_string(), CommitmentConfig::confirmed());
    for _ in 0..tries {
        let all = pks.iter().all(|pk| {
            rpc.get_account(pk)
                .ok()
                .map(|a| a.owner == client::market::ID)
                .unwrap_or(false)
        });
        if all {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    anyhow::bail!("L1 owner still not market program after undelegate")
}

fn account_owned_by_market(url: &str, pk: &Pubkey) -> bool {
    let rpc = RpcClient::new_with_commitment(url.to_string(), CommitmentConfig::confirmed());
    rpc.get_account(pk)
        .ok()
        .map(|a| a.owner == client::market::ID)
        .unwrap_or(false)
}

#[allow(dead_code)]
fn settle_grid_key(l1: &str, market: Pubkey) -> Pubkey {
    let dump = client::grid_dump_pda(&market);
    let rpc = RpcClient::new_with_commitment(l1.to_string(), CommitmentConfig::confirmed());
    if rpc
        .get_account(&dump)
        .ok()
        .map(|a| a.owner == client::market::ID && a.data.len() >= 75)
        .unwrap_or(false)
    {
        dump
    } else {
        client::grid_pda(&market)
    }
}

fn rescue_grid_from_er(l1: &str, er: &str, kp: &Keypair, market: Pubkey) -> Result<()> {
    let grid = client::grid_pda(&market);
    if account_owned_by_market(l1, &grid) {
        return Ok(());
    }
    let er_rpc = if er.is_empty() { l1 } else { er };
    let er_client = RpcClient::new_with_commitment(er_rpc.to_string(), CommitmentConfig::confirmed());
    let data = er_client
        .get_account(&grid)
        .map_err(|e| anyhow::anyhow!("ER grid {grid}: {e}"))?
        .data;
    require_dump_grown(l1, kp, market, data.len())?;
    let mut off = 0usize;
    while off < data.len() {
        let end = (off + 800).min(data.len());
        let sig = send(
            l1,
            kp,
            client::write_grid_dump(kp.pubkey(), market, off as u32, data[off..end].to_vec()),
        )?;
        eprintln!("ok {sig} write-grid-dump {off}..{end}");
        off = end;
    }
    Ok(())
}

#[allow(dead_code)]
fn require_dump_grown(l1: &str, kp: &Keypair, market: Pubkey, need: usize) -> Result<()> {
    let rpc = RpcClient::new_with_commitment(l1.to_string(), CommitmentConfig::confirmed());
    let dump = client::grid_dump_pda(&market);
    loop {
        let blen = rpc.get_account(&dump).map(|a| a.data.len()).unwrap_or(0);
        if blen >= need {
            return Ok(());
        }
        let sig = send(l1, kp, client::prepare_grid_dump(kp.pubkey(), market))?;
        eprintln!("ok {sig} prepare-grid-dump {blen}->{need}");
    }
}

fn settle_seat_on_l1(
    l1: &str,
    er: &str,
    kp: &Keypair,
    owner: Pubkey,
    market: Pubkey,
    set_hash: [u8; 32],
) -> Result<()> {
    undelegate_seat_on_er(l1, er, kp, owner, market, set_hash)?;
    sync_vault_on_l1(l1, kp, owner, market, set_hash)
}

fn ensure_seat(url: &str, kp: &Keypair, owner: Pubkey, market: Pubkey, set_hash: [u8; 32]) -> Result<()> {
    let rpc = RpcClient::new_with_commitment(url.to_string(), CommitmentConfig::confirmed());
    let pos = client::position_pda(&market, &owner, &set_hash);
    let nonce = client::nonce_pda(&owner, &market);
    if account_delegated(&rpc, &pos) && account_delegated(&rpc, &nonce) {
        return Ok(());
    }
    let sig = send(url, kp, client::open_seat(kp.pubkey(), owner, market, set_hash))?;
    println!("open_seat {sig}");
    let sig = send(url, kp, client::delegate_seat(kp.pubkey(), owner, market, set_hash))?;
    println!("delegate_seat {sig}");
    Ok(())
}

fn trade_cmd(
    url: &str,
    er_url: &str,
    owner_kp: &Keypair,
    owner: Pubkey,
    market: String,
    mask: String,
    shares: i64,
    nonce: u64,
    session: Option<PathBuf>,
    gateway: Option<String>,
    is_buy: bool,
) -> Result<()> {
    let market = Pubkey::from_str(&market)?;
    let n = rpc_market_n(url, &market)?;
    let set_mask = mask_bytes(&mask, n)?;
    let l1 = RpcClient::new_with_commitment(url.to_string(), CommitmentConfig::confirmed());
    let on_er = market_is_delegated(&l1, &market);
    if on_er {
        if er_url.is_empty() {
            anyhow::bail!("market is delegated; pass --er-url");
        }
        ensure_seat(url, owner_kp, owner, market, client::market::ids::set_hash(&set_mask))?;
        if session.is_some() {
            ensure_session(url, owner_kp, owner)?;
        }
    }
    let fill_url = if on_er { er_url } else { url };
    let rpc = RpcClient::new_with_commitment(fill_url.to_string(), CommitmentConfig::confirmed());
    let nonce = next_nonce(&rpc, &owner, &market, nonce)?;
    let q = Q64::from_int(shares).raw();
    let (ixs, payer_owned): (Vec<solana_sdk::instruction::Instruction>, Option<Keypair>) =
        if let Some(path) = session {
            let sk = read_keypair_file(&path).map_err(|e| anyhow::anyhow!("session {path:?}: {e}"))?;
            let ixs = client::fill_set_ixs(
                owner,
                sk.pubkey(),
                Some(client::session_pda(&owner)),
                market,
                set_mask,
                q,
                nonce,
                is_buy,
                on_er,
            );
            (ixs, Some(sk))
        } else {
            (
                client::fill_set_ixs(owner, owner, None, market, set_mask, q, nonce, is_buy, on_er),
                None,
            )
        };
    let payer = payer_owned.as_ref().unwrap_or(owner_kp);
    let mut last = String::new();
    let single = ixs.len() == 1;
    for ix in ixs {
        last = if single {
            if let Some(gw) = gateway.as_ref() {
                send_via_gateway(gw, fill_url, payer, ix, &[], owner, market, nonce)?
            } else {
                send_ixs(fill_url, payer, vec![ix], &[])?
            }
        } else {
            send_ixs(fill_url, payer, vec![ix], &[])?
        };
    }
    println!("ok {last} shares={shares} nonce={nonce} trader={}", payer.pubkey());
    Ok(())
}

fn trade_skellam_cmd(
    url: &str,
    er_url: &str,
    owner_kp: &Keypair,
    owner: Pubkey,
    market: String,
    kind: u8,
    a: i16,
    b: i16,
    shares: i64,
    nonce: u64,
    session: Option<PathBuf>,
    gateway: Option<String>,
    is_buy: bool,
) -> Result<()> {
    let market = Pubkey::from_str(&market)?;
    let set_hash = client::market::ids::skellam_ticket(kind, a, b);
    let l1 = RpcClient::new_with_commitment(url.to_string(), CommitmentConfig::confirmed());
    let on_er = market_is_delegated(&l1, &market);
    if on_er {
        if er_url.is_empty() {
            anyhow::bail!("market is delegated; pass --er-url");
        }
        ensure_seat(url, owner_kp, owner, market, set_hash)?;
        if session.is_some() {
            ensure_session(url, owner_kp, owner)?;
        }
    }
    let fill_url = if on_er { er_url } else { url };
    let rpc = RpcClient::new_with_commitment(fill_url.to_string(), CommitmentConfig::confirmed());
    let nonce = next_nonce(&rpc, &owner, &market, nonce)?;
    let q = Q64::from_int(shares).raw();
    let (ix, payer_owned, extras): (solana_sdk::instruction::Instruction, Option<Keypair>, Vec<Keypair>) =
        if let Some(path) = session {
            let sk = read_keypair_file(&path).map_err(|e| anyhow::anyhow!("session {path:?}: {e}"))?;
            let ix = if is_buy {
                if on_er {
                    client::buy_skellam_set_session_er(owner, sk.pubkey(), market, kind, a, b, q, nonce)
                } else {
                    client::buy_skellam_set_session(owner, sk.pubkey(), market, kind, a, b, q, nonce)
                }
            } else if on_er {
                client::sell_skellam_set_session_er(owner, sk.pubkey(), market, kind, a, b, q, nonce)
            } else {
                client::sell_skellam_set_session(owner, sk.pubkey(), market, kind, a, b, q, nonce)
            };
            (ix, Some(sk), vec![])
        } else {
            let ix = if is_buy {
                if on_er {
                    client::buy_skellam_set_er(owner, market, kind, a, b, q, nonce)
                } else {
                    client::buy_skellam_set(owner, market, kind, a, b, q, nonce)
                }
            } else if on_er {
                client::sell_skellam_set_er(owner, market, kind, a, b, q, nonce)
            } else {
                client::sell_skellam_set(owner, market, kind, a, b, q, nonce)
            };
            (ix, None, vec![])
        };
    let payer = payer_owned.as_ref().unwrap_or(owner_kp);
    let extra_refs: Vec<&Keypair> = extras.iter().collect();
    let sig = if let Some(gw) = gateway {
        send_via_gateway(&gw, fill_url, payer, ix, &extra_refs, owner, market, nonce)?
    } else {
        send_ixs(fill_url, payer, vec![ix], &extra_refs)?
    };
    println!("ok {sig} skellam kind={kind} a={a} b={b} shares={shares} nonce={nonce} trader={}", payer.pubkey());
    Ok(())
}

fn send(url: &str, payer: &Keypair, ix: solana_sdk::instruction::Instruction) -> Result<String> {
    send_ixs(url, payer, vec![ix], &[])
}

fn er_rpc<'a>(l1: &'a str, er: &'a str) -> &'a str {
    if er.is_empty() {
        l1
    } else {
        er
    }
}

fn already_in_use(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("already in use")
        || m.contains("already initialized")
        || m.contains("custom program error: 0x0")
}

fn record_open(rpc: &RpcClient, market: &Pubkey) -> bool {
    rpc.get_account(&client::record_pda(market))
        .map(|a| a.data.len() > 8)
        .unwrap_or(false)
}

fn fetch_program_markets(url: &str) -> Vec<(Pubkey, Vec<u8>)> {
    let rpc = RpcClient::new_with_commitment(url.to_string(), CommitmentConfig::confirmed());
    match rpc.get_program_accounts(&client::market::ID) {
        Ok(rows) => rows.into_iter().map(|(k, a)| (k, a.data)).collect(),
        Err(e) => {
            eprintln!("get_program_accounts {url}: {e}");
            Vec::new()
        }
    }
}

/// L1 Market-owned accounts miss delegated books (owner = DELeGG). Merge ER
/// copies and any journal market still sitting on ER.
fn merge_market_accounts(url: &str, er_url: &str, journal: &journal::Journal) -> Vec<(Pubkey, Vec<u8>)> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<Pubkey, Vec<u8>> = BTreeMap::new();
    for (k, d) in fetch_program_markets(url) {
        map.insert(k, d);
    }
    if !er_url.is_empty() {
        for (k, d) in fetch_program_markets(er_url) {
            map.insert(k, d);
        }
        let er = RpcClient::new_with_commitment(er_url.to_string(), CommitmentConfig::confirmed());
        for name in journal.listed_markets() {
            let Ok(pk) = Pubkey::from_str(&name) else { continue };
            if map.contains_key(&pk) {
                continue;
            }
            if let Ok(acc) = er.get_account(&pk) {
                map.insert(pk, acc.data);
            }
        }
    }
    map.into_iter().collect()
}

fn session_rpc<'a>(l1: &'a str, er: &'a str, owner: &Pubkey) -> &'a str {
    if account_delegated(
        &RpcClient::new_with_commitment(l1.to_string(), CommitmentConfig::confirmed()),
        &client::session_pda(owner),
    ) {
        er_rpc(l1, er)
    } else {
        l1
    }
}

fn keeper_once(
    url: &str,
    er_url: &str,
    kp: &Keypair,
    journal: &journal::Journal,
    heartbeat: &PathBuf,
    notify_dir: &PathBuf,
) -> Result<()> {
    let rpc = RpcClient::new_with_commitment(url.to_string(), CommitmentConfig::confirmed());
    let slot = rpc.get_slot().unwrap_or(0);
    let now = wall_unix();
    let me = kp.pubkey();
    let mut last = "scan".to_string();
    let mut last_market = String::new();
    let mut ok = true;
    let accounts = merge_market_accounts(url, er_url, journal);
    if accounts.is_empty() {
        notify::write_heartbeat(
            heartbeat,
            &notify::Heartbeat {
                slot,
                ts: now,
                last: "rpc empty".into(),
                market: String::new(),
                ok: false,
            },
        )?;
        return Ok(());
    }
    for (key, data) in accounts {
        let Ok(mkt) = client::decode_market(&data) else { continue };
        let market = key;
        if mkt.delegated {
            let root = journal.trades_root(&market.to_string()).unwrap_or([0u8; 32]);
            if root != mkt.trades_root {
                match send(if er_url.is_empty() { url } else { er_url }, kp, client::commit_book(me, market, root)) {
                    Ok(sig) => {
                        last = "commit".into();
                        last_market = market.to_string();
                        let _ = notify::append_event(
                            notify_dir,
                            &notify::Event {
                                ts: now,
                                kind: "commit".into(),
                                market: last_market.clone(),
                            },
                        );
                        println!("commit {sig} market={market}");
                    }
                    Err(e) => {
                        ok = false;
                        eprintln!("commit {market}: {e}");
                    }
                }
            }
        }
        let trading = mkt.status == 1;
        let halted = mkt.status == 2;
        if trading && now >= mkt.close_ts {
            let halt_url = if mkt.delegated || account_delegated(&rpc, &market) {
                er_rpc(url, er_url)
            } else {
                url
            };
            match send(halt_url, kp, client::halt(me, market)) {
                Ok(sig) => {
                    last = "close".into();
                    last_market = market.to_string();
                    let _ = notify::append_event(
                        notify_dir,
                        &notify::Event {
                            ts: now,
                            kind: "close".into(),
                            market: last_market.clone(),
                        },
                    );
                    println!("close {sig} market={market}");
                    if mkt.delegated {
                        match send(if er_url.is_empty() { url } else { er_url }, kp, client::undelegate_book(me, market)) {
                            Ok(s2) => {
                                last = "undelegate".into();
                                println!("undelegate {s2} market={market}");
                            }
                            Err(e) => eprintln!("undelegate {market}: {e}"),
                        }
                        if let Err(e) = wait_book_home(
                            url,
                            if er_url.is_empty() { url } else { er_url },
                            kp,
                            me,
                            market,
                            mkt.n,
                        ) {
                            ok = false;
                            eprintln!("undelegate wait {market}: {e}");
                        }
                    }
                    if let Err(e) = undelegate_seats_for_market(url, er_url, kp, market) {
                        eprintln!("seats {market}: {e}");
                    }
                    if !record_open(&rpc, &market) {
                        if let Err(e) = send(url, kp, client::resolve_open(me, market)) {
                            let msg = e.to_string();
                            if !already_in_use(&msg) {
                                eprintln!("resolve_open {market}: {e}");
                            }
                        }
                    }
                }
                Err(e) => {
                    let msg = e.to_string();
                    if !msg.contains("NotTrading") && !msg.contains("6010") {
                        ok = false;
                        eprintln!("close {market}: {e}");
                    }
                }
            }
        } else if mkt.delegated && (halted || now >= mkt.close_ts) {
            match send(if er_url.is_empty() { url } else { er_url }, kp, client::undelegate_book(me, market)) {
                Ok(sig) => {
                    last = "undelegate".into();
                    last_market = market.to_string();
                    println!("undelegate {sig} market={market}");
                }
                Err(e) => {
                    let msg = e.to_string();
                    if !msg.contains("NotDelegated") && !msg.contains("6025") {
                        eprintln!("undelegate {market}: {e}");
                    }
                }
            }
            if let Err(e) = wait_book_home(
                url,
                if er_url.is_empty() { url } else { er_url },
                kp,
                me,
                market,
                mkt.n,
            ) {
                ok = false;
                eprintln!("undelegate wait {market}: {e}");
            }
            if let Err(e) = undelegate_seats_for_market(url, er_url, kp, market) {
                eprintln!("seats {market}: {e}");
            }
        }
    }
    notify::write_heartbeat(
        heartbeat,
        &notify::Heartbeat {
            slot,
            ts: now,
            last,
            market: last_market,
            ok,
        },
    )?;
    println!("ok heartbeat {} slot={slot}", heartbeat.display());
    Ok(())
}

fn send_ixs(
    url: &str,
    payer: &Keypair,
    ixs: Vec<solana_sdk::instruction::Instruction>,
    extra: &[&Keypair],
) -> Result<String> {
    let rpc = RpcClient::new_with_commitment(url.to_string(), CommitmentConfig::confirmed());
    let bh = rpc.get_latest_blockhash()?;
    let heap = ComputeBudgetInstruction::request_heap_frame(256 * 1024);
    let cu = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    let mut all = vec![heap, cu];
    all.extend(ixs);
    let mut signers: Vec<&Keypair> = vec![payer];
    for s in extra {
        if s.pubkey() != payer.pubkey() {
            signers.push(s);
        }
    }
    let tx = Transaction::new_signed_with_payer(&all, Some(&payer.pubkey()), &signers, bh);
    let sig = rpc.send_and_confirm_transaction(&tx)?;
    Ok(sig.to_string())
}

fn finish_grid(url: &str, payer: &Keypair, market: Pubkey, n: u16) -> Result<()> {
    let rpc = RpcClient::new_with_commitment(url.to_string(), CommitmentConfig::confirmed());
    let n = rpc
        .get_account(&market)
        .ok()
        .and_then(|a| client::decode_market(&a.data).ok())
        .map(|m| m.n)
        .unwrap_or(n);
    let count = client::market::state::Grid::shard_count(n);
    for ix in 1..count {
        let shard = client::grid_shard_pda(&market, ix as u8);
        if rpc.get_account(&shard).is_err() {
            send(url, payer, client::create_grid_shard(payer.pubkey(), market, ix as u8))?;
        }
        send(url, payer, client::write_grid_shard(payer.pubkey(), market, ix as u8))?;
    }
    let grid = client::grid_pda(&market);
    let local0 = client::market::state::Grid::shard_len(n, 0) as usize;
    let need = client::market::state::Grid::space(local0);
    send(url, payer, client::write_grid_mass(payer.pubkey(), market))?;
    let extra = count.saturating_sub(1);
    if extra > 16 {
        let extras: Vec<u8> = (1..count).map(|i| i as u8).collect();
        for chunk in extras.chunks(16) {
            send(url, payer, client::accum_seal(payer.pubkey(), market, chunk))?;
        }
        for chunk in extras.chunks(16) {
            send(url, payer, client::apply_seal(payer.pubkey(), market, chunk))?;
        }
    } else {
        send(url, payer, client::seal_grid_n(payer.pubkey(), market, n))?;
    }
    let acc = rpc.get_account(&grid).context("grid after finish")?;
    let g = client::decode_grid(&acc.data).map_err(|e| anyhow::anyhow!("{e}"))?;
    anyhow::ensure!(
        g.p0.len() == local0 && g.weights.len() == local0 && g.z != 0 && acc.data.len() >= need,
        "grid not sealed n={n} p0={} z={} bytes={}",
        g.p0.len(),
        g.z,
        acc.data.len()
    );
    Ok(())
}

fn print_created(sig: &str, market: Pubkey, close_ts: i64, challenge_secs: i64) {
    println!("ok {sig} market={market} close_ts={close_ts} challenge_secs={challenge_secs}");
}

fn main() -> Result<()> {
    let opt = Opt::parse();
    let kp = match &opt.cmd {
        Cmd::WriteLocalMint { .. } => Keypair::new(),
        _ => load_kp(&opt.keypair)?,
    };
    let me = kp.pubkey();

    match opt.cmd {
        Cmd::Committee(CommitteeCmd::Init { members, m }) => {
            let roster = parse_members(members.as_deref().unwrap_or(""), Some(me))?;
            let sig = send(&opt.url, &kp, client::init_committee(me, roster.clone(), m))?;
            println!("ok {sig} committee={} n={} m={m}", client::committee_pda(), roster.len());
        }
        Cmd::Committee(CommitteeCmd::Set { members, m }) => {
            let roster = parse_members(&members, None)?;
            let sig = send(&opt.url, &kp, client::set_roster(me, roster.clone(), m))?;
            println!("ok {sig} committee={} n={} m={m}", client::committee_pda(), roster.len());
        }
        Cmd::VaultInit => {
            let sig = send(&opt.url, &kp, client::initialize_vault(me))?;
            println!("ok {sig}");
        }
        Cmd::Deposit { amount } => {
            let sig = send(&opt.url, &kp, client::deposit(me, amount))?;
            println!("ok {sig} user={}", client::user_vault(&me));
        }
        Cmd::Withdraw { amount } => {
            let sig = send(&opt.url, &kp, client::withdraw(me, amount))?;
            println!("ok {sig} user={}", client::user_vault(&me));
        }
        Cmd::Session(SessionCmd::Open { authority, hours, usdc, market }) => {
            let auth = read_keypair_file(&authority).map_err(|e| anyhow::anyhow!("session key {authority:?}: {e}"))?;
            let expires = wall_unix() + hours.saturating_mul(3600).max(60);
            let whitelist = market
                .as_deref()
                .map(Pubkey::from_str)
                .transpose()?
                .unwrap_or_default();
            let session = client::session_pda(&me);
            let l1 = RpcClient::new_with_commitment(opt.url.clone(), CommitmentConfig::confirmed());
            let url = if account_delegated(&l1, &session) {
                session_rpc(&opt.url, &opt.er_url, &me)
            } else {
                opt.url.as_str()
            };
            let sig = send(
                url,
                &kp,
                client::open_session(
                    me,
                    auth.pubkey(),
                    expires,
                    usdc,
                    client::market::session::IX_ALL_TRADES,
                    whitelist,
                ),
            )?;
            println!(
                "ok {sig} session={session} authority={} expires_ts={expires} remaining={usdc}",
                auth.pubkey()
            );
        }
        Cmd::Session(SessionCmd::Renew { hours, usdc }) => {
            let expires = wall_unix() + hours.saturating_mul(3600).max(60);
            let rpc = session_rpc(&opt.url, &opt.er_url, &me);
            let sig = send(rpc, &kp, client::renew_session(me, expires, usdc))?;
            println!("ok {sig} session={} expires_ts={expires} remaining={usdc}", client::session_pda(&me));
        }
        Cmd::Session(SessionCmd::Revoke) => {
            let rpc = session_rpc(&opt.url, &opt.er_url, &me);
            let sig = send(rpc, &kp, client::revoke_session(me))?;
            println!("ok {sig} revoked session={}", client::session_pda(&me));
        }
        Cmd::Faucet { amount, mint_authority } => {
            let auth_path = mint_authority.unwrap_or_else(default_mint_authority);
            let mint_kp = read_keypair_file(&auth_path)
                .map_err(|e| anyhow::anyhow!("mint authority {}: {e}", auth_path.display()))?;
            let sig = send_ixs(
                &opt.url,
                &kp,
                vec![client::create_ata(me, me), client::mint_usdc(mint_kp.pubkey(), me, amount)],
                &[&mint_kp],
            )?;
            println!("ok {sig} ata={} amount={amount}", client::ata(&me));
        }
        Cmd::WriteLocalMint { authority, account } => {
            let mint_kp = if authority.exists() {
                read_keypair_file(&authority)
                    .map_err(|e| anyhow::anyhow!("authority {}: {e}", authority.display()))?
            } else {
                if let Some(parent) = authority.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let fresh = Keypair::new();
                write_keypair_file(&fresh, &authority)
                    .map_err(|e| anyhow::anyhow!("write {}: {e}", authority.display()))?;
                fresh
            };
            if let Some(parent) = account.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let data = client::pack_local_usdc_mint(&mint_kp.pubkey());
            let b64 = base64::engine::general_purpose::STANDARD.encode(data);
            let body = format!(
                "{{\n  \"pubkey\": \"{}\",\n  \"account\": {{\n    \"lamports\": 1000000000,\n    \"data\": [\"{}\", \"base64\"],\n    \"owner\": \"{TOKEN_PROGRAM}\",\n    \"executable\": false,\n    \"rentEpoch\": 0\n  }}\n}}\n",
                client::vault::USDC_MINT,
                b64
            );
            std::fs::write(&account, body)?;
            println!(
                "ok mint={} authority={} account={}",
                client::vault::USDC_MINT,
                mint_kp.pubkey(),
                account.display()
            );
        }
        Cmd::Market(MarketCmd::CreateGaussian {
            topic,
            tag,
            n,
            close_in,
            close_ts,
            challenge_secs,
        }) => {
            let (close_ts, risk_lock_ts) = plan_close(close_in, close_ts)?;
            let topic = pad32(&topic);
            let tag = pad32(&tag);
            let id_hash = client::market::ids::interval(client::market::state::Family::Gaussian as u8, &topic, &tag);
            let market = client::market_pda(&id_hash);
            let args = client::market::state::IntervalArgs {
                common: common(me, id_hash, n, close_ts, risk_lock_ts, challenge_secs),
                topic,
                tag,
                x_min: Q64::from_int(0).raw(),
                x_max: Q64::from_int(10).raw(),
                mu: Q64::from_int(5).raw(),
                sigma: Q64::from_int(2).raw(),
            };
            let sig = send(&opt.url, &kp, client::create_gaussian(me, id_hash, n, args))?;
            finish_grid(&opt.url, &kp, market, n)?;
            print_created(&sig, market, close_ts, challenge_secs);
        }
        Cmd::Market(MarketCmd::CreateLognormal {
            topic,
            tag,
            n,
            close_in,
            close_ts,
            challenge_secs,
        }) => {
            let (close_ts, risk_lock_ts) = plan_close(close_in, close_ts)?;
            let topic = pad32(&topic);
            let tag = pad32(&tag);
            let id_hash = client::market::ids::interval(client::market::state::Family::Lognormal as u8, &topic, &tag);
            let market = client::market_pda(&id_hash);
            let args = client::market::state::IntervalArgs {
                common: common(me, id_hash, n, close_ts, risk_lock_ts, challenge_secs),
                topic,
                tag,
                x_min: Q64::from_int(1).raw(),
                x_max: Q64::from_int(16).raw(),
                mu: Q64::from_int(2).raw(),
                sigma: Q64::from_int(1).raw(),
            };
            let sig = send(&opt.url, &kp, client::create_lognormal(me, id_hash, n, args))?;
            finish_grid(&opt.url, &kp, market, n)?;
            print_created(&sig, market, close_ts, challenge_secs);
        }
        Cmd::Market(MarketCmd::CreateDirichlet {
            topic,
            n,
            close_in,
            close_ts,
            challenge_secs,
        }) => {
            let (close_ts, risk_lock_ts) = plan_close(close_in, close_ts)?;
            let topic = pad32(&topic);
            let id_hash = client::market::ids::dirichlet(&topic, client::market::state::DIRICHLET_ATOMS, 0, 0);
            let market = client::market_pda(&id_hash);
            let alpha = vec![Q64::from_int(1).raw(); n as usize];
            let args = client::market::state::DirichletArgs {
                common: common(me, id_hash, n, close_ts, risk_lock_ts, challenge_secs),
                topic,
                layout: client::market::state::DIRICHLET_ATOMS,
                top_n: 0,
                bins: 0,
                alpha,
            };
            let sig = send(&opt.url, &kp, client::create_dirichlet(me, id_hash, n, args))?;
            finish_grid(&opt.url, &kp, market, n)?;
            print_created(&sig, market, close_ts, challenge_secs);
        }
        Cmd::Market(MarketCmd::CreateBernoulli {
            topic,
            tag,
            close_in,
            close_ts,
            challenge_secs,
        }) => {
            let (close_ts, risk_lock_ts) = plan_close(close_in, close_ts)?;
            let topic = pad32(&topic);
            let tag = pad32(&tag);
            let id_hash = client::market::ids::bernoulli(&topic, &tag);
            let market = client::market_pda(&id_hash);
            let n = 2;
            let args = client::market::state::BernoulliArgs {
                common: common(me, id_hash, n, close_ts, risk_lock_ts, challenge_secs),
                topic,
                tag,
                deadline_ts: close_ts,
                early_resolve: false,
                alpha_yes: Q64::from_int(1).raw(),
                alpha_no: Q64::from_int(1).raw(),
            };
            let sig = send(&opt.url, &kp, client::create_bernoulli(me, id_hash, n, args))?;
            print_created(&sig, market, close_ts, challenge_secs);
        }
        Cmd::Market(MarketCmd::CreateSkellam {
            topic,
            score_scope,
            close_in,
            close_ts,
            challenge_secs,
        }) => {
            let (close_ts, risk_lock_ts) = plan_close(close_in, close_ts)?;
            let topic = pad32(&topic);
            let id_hash = client::market::ids::skellam(&topic, score_scope);
            let market = client::market_pda(&id_hash);
            let n = client::market::state::FOOTBALL_N;
            let args = client::market::state::SkellamArgs {
                common: common(me, id_hash, n, close_ts, risk_lock_ts, challenge_secs),
                topic,
                score_scope,
                kickoff_ts: close_ts,
                prior_kind: 0,
                lambda_home: Q64::from_int(1).raw(),
                lambda_away: Q64::from_int(1).raw(),
                dc_rho: 0,
            };
            let sig = send(&opt.url, &kp, client::create_skellam(me, id_hash, n, args))?;
            finish_grid(&opt.url, &kp, market, n)?;
            print_created(&sig, market, close_ts, challenge_secs);
        }
        Cmd::Market(MarketCmd::Close { market, mask, skellam_kind, skellam_a, skellam_b }) => {
            let market = Pubkey::from_str(&market)?;
            let rpc = RpcClient::new_with_commitment(opt.url.clone(), CommitmentConfig::confirmed());
            let delegated = market_is_delegated(&rpc, &market);
            let halt_url = if delegated {
                er_rpc(&opt.url, &opt.er_url)
            } else {
                opt.url.as_str()
            };
            let halt = send(halt_url, &kp, client::halt(me, market));
            let already = halt
                .as_ref()
                .err()
                .map(|e| e.to_string())
                .map(|s| s.contains("NotTrading") || s.contains("6010"))
                .unwrap_or(false);
            if let Err(e) = &halt {
                if !already {
                    return Err(anyhow::anyhow!("{e}"));
                }
                eprintln!("halt already stopped (ok)");
            }
            let n = rpc_market_n(&opt.url, &market).or_else(|_| {
                if delegated {
                    rpc_market_n(er_rpc(&opt.url, &opt.er_url), &market)
                } else {
                    Err(anyhow::anyhow!("market n"))
                }
            })?;
            let set_hash = if let Some(kind) = skellam_kind {
                Some(client::market::ids::skellam_ticket(kind, skellam_a, skellam_b))
            } else {
                mask
                    .as_deref()
                    .map(|hex| mask_bytes(hex, n))
                    .transpose()?
                    .map(|m| client::market::ids::set_hash(&m))
            };
            if let Some(set_hash) = set_hash {
                undelegate_seat_on_er(&opt.url, &opt.er_url, &kp, me, market, set_hash)?;
            }
            undelegate_seats_for_market(&opt.url, &opt.er_url, &kp, market)?;
            if delegated {
                let und = send(er_rpc(&opt.url, &opt.er_url), &kp, client::undelegate_book(me, market));
                match &und {
                    Ok(sig2) => println!("ok halted; undelegate {sig2}"),
                    Err(e) => {
                        let s = e.to_string();
                        if s.contains("NotDelegated") || s.contains("6025") {
                            eprintln!("undelegate already done (ok)");
                        } else {
                            return Err(anyhow::anyhow!("{e}"));
                        }
                    }
                }
                wait_book_home(&opt.url, er_rpc(&opt.url, &opt.er_url), &kp, me, market, n)?;
                if let Some(set_hash) = set_hash {
                    sync_vault_on_l1(&opt.url, &kp, me, market, set_hash)?;
                }
            } else {
                let grid = client::grid_pda(&market);
                if account_owned_by_market(&opt.url, &grid) {
                    println!("ok halted");
                    println!("home shard0={grid}");
                } else {
                    println!("ok halted");
                    wait_book_home(&opt.url, er_rpc(&opt.url, &opt.er_url), &kp, me, market, n)?;
                }
                if let Some(set_hash) = set_hash {
                    sync_vault_on_l1(&opt.url, &kp, me, market, set_hash)?;
                }
            }
        }
        Cmd::Market(MarketCmd::Delegate { market }) => {
            let market = Pubkey::from_str(&market)?;
            let n = rpc_market_n(&opt.url, &market)?;
            let sig = send(&opt.url, &kp, client::delegate_book(me, market))?;
            println!("ok {sig} delegated");
            for ix in 1..client::market::state::Grid::shard_count(n) {
                let s = send(&opt.url, &kp, client::delegate_grid_shard(me, market, ix as u8))?;
                println!("ok {s} delegated shard={ix}");
            }
        }
        Cmd::Market(MarketCmd::Commit { market, root }) => {
            let market = Pubkey::from_str(&market)?;
            let sig = send(er_rpc(&opt.url, &opt.er_url), &kp, client::commit_book(me, market, parse_root(&root)?))?;
            println!("ok {sig} commit");
        }
        Cmd::Market(MarketCmd::Undelegate { market, mask, skellam_kind, skellam_a, skellam_b }) => {
            let market = Pubkey::from_str(&market)?;
            let n = rpc_market_n(&opt.url, &market)?;
            let set_hash = if let Some(kind) = skellam_kind {
                Some(client::market::ids::skellam_ticket(kind, skellam_a, skellam_b))
            } else {
                mask
                    .as_deref()
                    .map(|hex| mask_bytes(hex, n))
                    .transpose()?
                    .map(|m| client::market::ids::set_hash(&m))
            };
            if let Some(set_hash) = set_hash {
                undelegate_seat_on_er(&opt.url, &opt.er_url, &kp, me, market, set_hash)?;
            }
            let und = send(er_rpc(&opt.url, &opt.er_url), &kp, client::undelegate_book(me, market));
            match und {
                Ok(sig) => println!("ok {sig} undelegated"),
                Err(e) => {
                    let s = e.to_string();
                    if s.contains("NotDelegated") || s.contains("6025") {
                        eprintln!("undelegate already done (ok)");
                    } else {
                        return Err(e);
                    }
                }
            }
            wait_book_home(&opt.url, er_rpc(&opt.url, &opt.er_url), &kp, me, market, n)?;
            if let Some(set_hash) = set_hash {
                sync_vault_on_l1(&opt.url, &kp, me, market, set_hash)?;
            }
        }
        Cmd::Market(MarketCmd::Info { market }) => {
            let market = Pubkey::from_str(&market)?;
            let rpc = RpcClient::new_with_commitment(opt.url.clone(), CommitmentConfig::confirmed());
            let (book, _, fee_bps) = load_book(&rpc, &market)?;
            let accounts = rpc.get_program_accounts(&client::market::ID)?;
            let mut owners = std::collections::HashSet::new();
            let mut tickets = 0u64;
            let mut stake = 0u64;
            for (_, acc) in accounts {
                let Ok(pos) = client::decode_position(&acc.data) else { continue };
                if pos.market != market || pos.q <= 0 {
                    continue;
                }
                owners.insert(pos.owner);
                tickets += 1;
                stake += pos.cost_paid;
            }
            let mkt = client::decode_market(&rpc.get_account(&market).context("market")?.data)
                .map_err(|e| anyhow::anyhow!(e))?;
            let root: String = mkt.trades_root.iter().map(|b| format!("{b:02x}")).collect();
            println!(
                "market={market} traders={} tickets={} stake_usdc={} trading_revenue={} l_max_usdc={} c_r={} r_net={} c_max_usdc={} coverage_bps={} fee_bps={} delegated={} trades_root={} commit_ts={}  # p is implied PDF, not E",
                owners.len(),
                tickets,
                stake,
                book.trading_revenue,
                book.l_max_usdc(),
                book.c_r,
                book.r_net(),
                book.c_max_usdc(),
                quote::q_bps(book.coverage()),
                fee_bps,
                mkt.delegated,
                root,
                mkt.commit_ts,
            );
        }
        Cmd::Market(MarketCmd::Pdf { market }) => {
            let market = Pubkey::from_str(&market)?;
            let rpc = RpcClient::new_with_commitment(opt.url.clone(), CommitmentConfig::confirmed());
            let (book, _, _) = load_book(&rpc, &market)?;
            let p = book.pdf();
            println!(
                "market={market} n={} beta_int={}  # p_bps is implied PDF; e is face E(x), not p",
                book.n(),
                usdc(book.state.beta)
            );
            for i in 0..p.len() {
                println!(
                    "cell={i} p_bps={} e={}",
                    quote::q_bps(p[i]),
                    usdc(book.state.exposure[i])
                );
            }
        }
        Cmd::Market(MarketCmd::Quote { market, mask, kind, a, b, shares }) => {
            let market = Pubkey::from_str(&market)?;
            let rpc = RpcClient::new_with_commitment(opt.url.clone(), CommitmentConfig::confirmed());
            let (book, n, fee_bps) = load_book(&rpc, &market)?;
            let in_set = if let Some(kind) = kind {
                let mkt = client::decode_market(&rpc.get_account(&market).context("market")?.data)
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                if mkt.family != client::market::state::Family::Skellam as u8 {
                    anyhow::bail!("--kind is a Skellam / football line");
                }
                let k_max = if mkt.extra.u2 == 0 {
                    client::market::state::FOOTBALL_K_MAX as u32
                } else {
                    mkt.extra.u2 as u32
                };
                let mut masks = client::math::football::skellam_masks(kind, a, b, k_max)
                    .ok_or_else(|| anyhow::anyhow!("bad skellam line kind={kind} a={a} b={b}"))?;
                if masks.len() != 1 {
                    anyhow::bail!("quarter line is two half-lines; quote each half");
                }
                masks.remove(0)
            } else {
                quote::decode_mask(&mask_bytes(&mask, n)?, n as usize)
                    .map_err(|e| anyhow::anyhow!("{e}"))?
            };
            let q = if shares > 0 { shares } else { 1 };
            let v = book.view(&in_set, Q64::from_int(q));
            let t = quote::ticket_from_view(&v, q, fee_bps);
            println!(
                "market={market} slot={} n={} p_s_raw={} p_s_bps={} c_s_raw={} c_s_usdc={} coverage_bps={} rho_hat_bps={} l_max_usdc={} c_max_usdc={} r_net={} fee_bps={} fee_usdc={} pay_usdc={} face_usdc={} payout_if_hit_usdc={} payout_if_miss_usdc={} net_if_hit={} ev_if_p_s={}",
                v.slot,
                v.n,
                v.p_s.raw(),
                quote::q_bps(v.p_s),
                v.c_s.raw(),
                usdc(v.c_s),
                quote::q_bps(v.coverage),
                quote::q_bps(v.rho_hat),
                v.l_max_usdc,
                v.c_max_usdc,
                v.r_net,
                t.fee_bps,
                t.fee_usdc,
                t.pay_usdc,
                t.face_usdc,
                t.payout_if_hit_usdc,
                t.payout_if_miss_usdc,
                t.net_if_hit,
                t.ev_if_p_s
            );
        }
        Cmd::Trade(TradeCmd::BuySet { market, mask, shares, nonce, session, gateway }) => {
            trade_cmd(&opt.url, &opt.er_url, &kp, me, market, mask, shares, nonce, session, gateway, true)?;
        }
        Cmd::Trade(TradeCmd::SellSet { market, mask, shares, nonce, session, gateway }) => {
            trade_cmd(&opt.url, &opt.er_url, &kp, me, market, mask, shares, nonce, session, gateway, false)?;
        }
        Cmd::Trade(TradeCmd::BuySkellam { market, shares, kind, a, b, nonce, session, gateway }) => {
            trade_skellam_cmd(&opt.url, &opt.er_url, &kp, me, market, kind, a, b, shares, nonce, session, gateway, true)?;
        }
        Cmd::Trade(TradeCmd::SellSkellam { market, shares, kind, a, b, nonce, session, gateway }) => {
            trade_skellam_cmd(&opt.url, &opt.er_url, &kp, me, market, kind, a, b, shares, nonce, session, gateway, false)?;
        }
        Cmd::Risk(RiskCmd::Open { market }) => {
            let market = Pubkey::from_str(&market)?;
            let sig = send(&opt.url, &kp, client::risk_open_book(me, market))?;
            println!("ok {sig} book={}", client::risk_book(&market));
        }
        Cmd::Risk(RiskCmd::Bid {
            market,
            layer,
            capacity,
            premium,
            profit_share_bps,
        }) => {
            let market = Pubkey::from_str(&market)?;
            let sig = send(
                &opt.url,
                &kp,
                client::risk_quote(me, market, layer, capacity, premium, profit_share_bps),
            )?;
            println!("ok {sig}");
        }
        Cmd::Resolve(ResolveCmd::Open { market }) => {
            let market = Pubkey::from_str(&market)?;
            let sig = send(&opt.url, &kp, client::resolve_open(me, market))?;
            println!("ok {sig}");
        }
        Cmd::Resolve(ResolveCmd::Submit { market, value, family, kind, b }) => {
            let market = Pubkey::from_str(&market)?;
            let a = match family {
                0 | 3 | 4 => value as i128,
                _ => Q64::from_int(value).raw(),
            };
            let b = match family {
                0 => b as i128,
                _ => 0,
            };
            let outcome = client::resolution::Outcome {
                family,
                kind,
                a,
                b,
                shares: [0; 8],
            };
            let sig = send(&opt.url, &kp, client::submit_result(me, market, outcome, [0; 32]))?;
            println!("ok {sig} family={family} kind={kind} a={value} b={b}");
        }
        Cmd::Resolve(ResolveCmd::Finalize { market }) => {
            let market = Pubkey::from_str(&market)?;
            let sig = send(&opt.url, &kp, client::finalize(me, market))?;
            println!("ok {sig}");
        }
        Cmd::Settle(SettleCmd::FundCm { market }) => {
            let market = Pubkey::from_str(&market)?;
            let sig = send(&opt.url, &kp, client::fund_cm(me, market, 0))?;
            println!("ok {sig} board={}", client::board(&market));
        }
        Cmd::Settle(SettleCmd::SyncVault { market, mask }) => {
            let market = Pubkey::from_str(&market)?;
            let n = rpc_market_n(&opt.url, &market)?;
            let set_hash = client::market::ids::set_hash(&mask_bytes(&mask, n)?);
            let sig = send(&opt.url, &kp, client::sync_vault(me, me, market, set_hash))?;
            println!("ok {sig} synced vault for mask={mask}");
        }
        Cmd::Settle(SettleCmd::Begin { market, risk, pool }) => {
            let market = Pubkey::from_str(&market)?;
            let rpc = RpcClient::new_with_commitment(opt.url.clone(), CommitmentConfig::confirmed());
            let grid = settle_grid_of(&rpc, market)?;
            let sig = send(&opt.url, &kp, client::begin_settle_on(market, risk, pool, grid))?;
            println!("ok {sig}");
        }
        Cmd::Settle(SettleCmd::Payout { market, mask, owner }) => {
            let market = Pubkey::from_str(&market)?;
            let owner = owner
                .map(|s| Pubkey::from_str(&s))
                .transpose()?
                .unwrap_or(me);
            let rpc = RpcClient::new_with_commitment(opt.url.clone(), CommitmentConfig::confirmed());
            let n = rpc_market_n(&opt.url, &market)?;
            let set_mask = mask_bytes(&mask, n)?;
            let set_hash = client::market::ids::set_hash(&set_mask);
            undelegate_seat_on_er(&opt.url, &opt.er_url, &kp, owner, market, set_hash)?;
            sync_vault_on_l1(&opt.url, &kp, owner, market, set_hash)?;
            let grid = settle_grid_of(&rpc, market)?;
            let sig = send(&opt.url, &kp, client::payout_on(me, market, owner, set_mask, grid))?;
            println!("ok {sig}");
        }
        Cmd::Settle(SettleCmd::PayoutSkellam { market, kind, a, b, owner }) => {
            let market = Pubkey::from_str(&market)?;
            let owner = owner
                .map(|s| Pubkey::from_str(&s))
                .transpose()?
                .unwrap_or(me);
            let rpc = RpcClient::new_with_commitment(opt.url.clone(), CommitmentConfig::confirmed());
            let set_hash = client::market::ids::skellam_ticket(kind, a, b);
            undelegate_seat_on_er(&opt.url, &opt.er_url, &kp, owner, market, set_hash)?;
            sync_vault_on_l1(&opt.url, &kp, owner, market, set_hash)?;
            let grid = settle_grid_of(&rpc, market)?;
            let sig = send(
                &opt.url,
                &kp,
                client::payout_skellam_on(me, market, owner, kind, a, b, grid),
            )?;
            println!("ok {sig} payout-skellam kind={kind} a={a} b={b}");
        }
        Cmd::Keeper {
            once,
            interval,
            journal_replica,
            journal_object,
            heartbeat,
            notify_dir,
        } => {
            let replica = journal_replica.unwrap_or_else(|| {
                PathBuf::from(std::env::var("JOURNAL_REPLICA_DIR").unwrap_or_else(|_| "journal-replica".into()))
            });
            let object = journal_object.unwrap_or_else(|| {
                PathBuf::from(std::env::var("JOURNAL_OBJECT_DIR").unwrap_or_else(|_| "journal-object".into()))
            });
            let heartbeat = heartbeat.unwrap_or_else(|| {
                PathBuf::from(std::env::var("KEEPER_HEARTBEAT_PATH").unwrap_or_else(|_| "tmp/keeper-heartbeat.json".into()))
            });
            let notify_dir = notify_dir.unwrap_or_else(|| {
                PathBuf::from(std::env::var("NOTIFY_DIR").unwrap_or_else(|_| "tmp/notify".into()))
            });
            let journal = journal::Journal::open(&replica, &object)?;
            loop {
                keeper_once(&opt.url, &opt.er_url, &kp, &journal, &heartbeat, &notify_dir)?;
                if once {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_secs(interval.max(1)));
            }
        }
        Cmd::IndexStatus => {
            let rpc = RpcClient::new(opt.url);
            let slot = rpc.get_slot().context("rpc")?;
            println!("rpc_ok slot={slot}");
        }
    }
    Ok(())
}
