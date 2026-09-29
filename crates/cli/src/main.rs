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
    /// Halt trading. If the board is delegated, also undelegate.
    Close { market: String },
    Delegate { market: String },
    Commit {
        market: String,
        /// 64-char hex journal `trades_root`.
        root: String,
    },
    Undelegate { market: String },
    /// Dump the trading-implied PDF $p_k$ and face $E_k$ (they are not the same).
    Pdf { market: String },
    /// Public desk: traders, stake, L_max, C_R, then the implied PDF.
    Info { market: String },
    /// On-chain θ → same $p_S$ / $C_S$ / coverage / $\hat\rho$ / pre-bet ticket as Market API.
    Quote {
        market: String,
        #[arg(long, default_value = "01")]
        mask: String,
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
        /// Integer outcome for Gaussian kind=1 (cell mapper uses Q64).
        value: i64,
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
    /// Open the board vault (amount is always 0).
    FundCm {
        market: String,
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

fn load_book(rpc: &RpcClient, market: &Pubkey) -> Result<(quote::Book, u16, u16)> {
    let mkt = client::decode_market(&rpc.get_account(market).context("market")?.data)
        .map_err(|e| anyhow::anyhow!(e))?;
    let grid = client::decode_grid(&rpc.get_account(&client::grid_pda(market)).context("grid")?.data)
        .map_err(|e| anyhow::anyhow!(e))?;
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
            &grid.p0,
            &grid.theta,
            &grid.exposure,
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

fn trade_cmd(
    url: &str,
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
    let set_mask = parse_mask(&mask)?;
    let rpc = RpcClient::new_with_commitment(url.to_string(), CommitmentConfig::confirmed());
    let nonce = next_nonce(&rpc, &owner, &market, nonce)?;
    let q = Q64::from_int(shares).raw();
    let (ix, payer_owned, extras): (solana_sdk::instruction::Instruction, Option<Keypair>, Vec<Keypair>) =
        if let Some(path) = session {
            let sk = read_keypair_file(&path).map_err(|e| anyhow::anyhow!("session {path:?}: {e}"))?;
            let ix = if is_buy {
                client::buy_set_session(owner, sk.pubkey(), market, set_mask, q, nonce)
            } else {
                client::sell_set_session(owner, sk.pubkey(), market, set_mask, q, nonce)
            };
            (ix, Some(sk), vec![])
        } else {
            let ix = if is_buy {
                client::buy_set(owner, market, set_mask, q, nonce)
            } else {
                client::sell_set(owner, market, set_mask, q, nonce)
            };
            (ix, None, vec![])
        };
    let payer = payer_owned.as_ref().unwrap_or(owner_kp);
    let extra_refs: Vec<&Keypair> = extras.iter().collect();
    let sig = if let Some(gw) = gateway {
        send_via_gateway(&gw, url, payer, ix, &extra_refs, owner, market, nonce)?
    } else {
        send_ixs(url, payer, vec![ix], &extra_refs)?
    };
    println!("ok {sig} shares={shares} nonce={nonce} trader={}", payer.pubkey());
    Ok(())
}

fn send(url: &str, payer: &Keypair, ix: solana_sdk::instruction::Instruction) -> Result<String> {
    send_ixs(url, payer, vec![ix], &[])
}

fn keeper_once(
    url: &str,
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
    let accounts = match rpc.get_program_accounts(&client::market::ID) {
        Ok(a) => a,
        Err(e) => {
            notify::write_heartbeat(
                heartbeat,
                &notify::Heartbeat {
                    slot,
                    ts: now,
                    last: format!("rpc {e}"),
                    market: String::new(),
                    ok: false,
                },
            )?;
            return Ok(());
        }
    };
    for (key, acc) in accounts {
        let Ok(mkt) = client::decode_market(&acc.data) else { continue };
        let market = key;
        if mkt.delegated {
            let root = journal.trades_root(&market.to_string()).unwrap_or([0u8; 32]);
            if root != mkt.trades_root {
                match send(url, kp, client::commit_book(me, market, root)) {
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
            match send(url, kp, client::halt(me, market)) {
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
                        match send(url, kp, client::undelegate_book(me, market)) {
                            Ok(s2) => {
                                last = "undelegate".into();
                                println!("undelegate {s2} market={market}");
                            }
                            Err(e) => eprintln!("undelegate {market}: {e}"),
                        }
                    }
                    if let Err(e) = send(url, kp, client::resolve_open(me, market)) {
                        eprintln!("resolve_open {market}: {e}");
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
            match send(url, kp, client::undelegate_book(me, market)) {
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
    let grid = client::grid_pda(&market);
    let need = client::grid_space(n);
    let tries = client::grid_grow_steps(n)
        .saturating_add(client::grid_mass_steps(n))
        .saturating_add(5)
        .max(1);
    for _ in 0..tries {
        if let Ok(acc) = rpc.get_account(&grid) {
            if let Ok(g) = client::decode_grid(&acc.data) {
                if g.p0.len() == n as usize && g.weights.len() == n as usize && g.z != 0 && acc.data.len() >= need {
                    return Ok(());
                }
                if acc.data.len() < need {
                    send(url, payer, client::grow_grid(payer.pubkey(), market))?;
                    continue;
                }
                if g.p0.len() < n as usize {
                    send(url, payer, client::write_grid_mass(payer.pubkey(), market))?;
                    continue;
                }
                if g.z == 0 {
                    send(url, payer, client::seal_grid(payer.pubkey(), market))?;
                    continue;
                }
            }
        } else {
            send(url, payer, client::grow_grid(payer.pubkey(), market))?;
        }
    }
    let acc = rpc.get_account(&grid).context("grid after finish")?;
    let g = client::decode_grid(&acc.data).map_err(|e| anyhow::anyhow!("{e}"))?;
    anyhow::ensure!(
        g.p0.len() == n as usize && g.weights.len() == n as usize && g.z != 0 && acc.data.len() >= need,
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
            let sig = send(
                &opt.url,
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
                "ok {sig} session={} authority={} expires_ts={expires} remaining={usdc}",
                client::session_pda(&me),
                auth.pubkey()
            );
        }
        Cmd::Session(SessionCmd::Renew { hours, usdc }) => {
            let expires = wall_unix() + hours.saturating_mul(3600).max(60);
            let sig = send(&opt.url, &kp, client::renew_session(me, expires, usdc))?;
            println!("ok {sig} session={} expires_ts={expires} remaining={usdc}", client::session_pda(&me));
        }
        Cmd::Session(SessionCmd::Revoke) => {
            let sig = send(&opt.url, &kp, client::revoke_session(me))?;
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
            print_created(&sig, market, close_ts, challenge_secs);
        }
        Cmd::Market(MarketCmd::Close { market }) => {
            let market = Pubkey::from_str(&market)?;
            let sig = send(&opt.url, &kp, client::halt(me, market))?;
            let rpc = RpcClient::new_with_commitment(opt.url.clone(), CommitmentConfig::confirmed());
            let delegated = rpc
                .get_account(&market)
                .ok()
                .and_then(|a| client::decode_market(&a.data).ok())
                .map(|m| m.delegated)
                .unwrap_or(false);
            if delegated {
                let sig2 = send(&opt.url, &kp, client::undelegate_book(me, market))?;
                println!("ok {sig} halted; undelegate {sig2}");
            } else {
                println!("ok {sig} halted");
            }
        }
        Cmd::Market(MarketCmd::Delegate { market }) => {
            let market = Pubkey::from_str(&market)?;
            let sig = send(&opt.url, &kp, client::delegate_book(me, market))?;
            println!("ok {sig} delegated");
        }
        Cmd::Market(MarketCmd::Commit { market, root }) => {
            let market = Pubkey::from_str(&market)?;
            let sig = send(&opt.url, &kp, client::commit_book(me, market, parse_root(&root)?))?;
            println!("ok {sig} commit");
        }
        Cmd::Market(MarketCmd::Undelegate { market }) => {
            let market = Pubkey::from_str(&market)?;
            let sig = send(&opt.url, &kp, client::undelegate_book(me, market))?;
            println!("ok {sig} undelegated");
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
        Cmd::Market(MarketCmd::Quote { market, mask, shares }) => {
            let market = Pubkey::from_str(&market)?;
            let rpc = RpcClient::new_with_commitment(opt.url.clone(), CommitmentConfig::confirmed());
            let (book, n, fee_bps) = load_book(&rpc, &market)?;
            let bytes = parse_mask(&mask)?;
            let in_set = quote::decode_mask(&bytes, n as usize).map_err(|e| anyhow::anyhow!(e))?;
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
            trade_cmd(&opt.url, &kp, me, market, mask, shares, nonce, session, gateway, true)?;
        }
        Cmd::Trade(TradeCmd::SellSet { market, mask, shares, nonce, session, gateway }) => {
            trade_cmd(&opt.url, &kp, me, market, mask, shares, nonce, session, gateway, false)?;
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
        Cmd::Resolve(ResolveCmd::Submit { market, value }) => {
            let market = Pubkey::from_str(&market)?;
            let outcome = client::resolution::Outcome {
                family: 1,
                kind: 1,
                a: Q64::from_int(value).raw(),
                b: 0,
                shares: [0; 8],
            };
            let sig = send(&opt.url, &kp, client::submit_result(me, market, outcome, [0; 32]))?;
            println!("ok {sig}");
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
        Cmd::Settle(SettleCmd::Begin { market, risk, pool }) => {
            let market = Pubkey::from_str(&market)?;
            let sig = send(&opt.url, &kp, client::begin_settle_ex(market, risk, pool))?;
            println!("ok {sig}");
        }
        Cmd::Settle(SettleCmd::Payout { market, mask, owner }) => {
            let market = Pubkey::from_str(&market)?;
            let owner = owner
                .map(|s| Pubkey::from_str(&s))
                .transpose()?
                .unwrap_or(me);
            let set_mask = parse_mask(&mask)?;
            let sig = send(&opt.url, &kp, client::payout(me, market, owner, set_mask))?;
            println!("ok {sig}");
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
                keeper_once(&opt.url, &kp, &journal, &heartbeat, &notify_dir)?;
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
