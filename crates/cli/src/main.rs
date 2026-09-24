//! FR-CLI-01. Instruction bytes come from `crates/client` (program crates), not a second IDL.

use anyhow::{Context, Result};
use base64::Engine;
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
    #[command(subcommand)]
    Market(MarketCmd),
    #[command(subcommand)]
    Trade(TradeCmd),
    #[command(subcommand)]
    Risk(RiskCmd),
    #[command(subcommand)]
    Resolve(ResolveCmd),
    #[command(subcommand)]
    Settle(SettleCmd),
    /// Keeper is Phase 5/8. Prints the intended loop only.
    Keeper,
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
        #[arg(long, default_value_t = 0)]
        c_m: u64,
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
        #[arg(long, default_value_t = 0)]
        c_m: u64,
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
        #[arg(long, default_value_t = 0)]
        c_m: u64,
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
        #[arg(long, default_value_t = 0)]
        c_m: u64,
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
        #[arg(long, default_value_t = 0)]
        c_m: u64,
        #[arg(long, default_value_t = 300)]
        close_in: i64,
        #[arg(long)]
        close_ts: Option<i64>,
        #[arg(long, default_value_t = 20)]
        challenge_secs: i64,
    },
    /// L1 stand-in for close / undelegate (ER lands later).
    Close { market: String },
    /// Dump the trading-implied PDF $p_k$ and face $E_k$ (they are not the same).
    Pdf { market: String },
    /// On-chain θ → same $p_S$ / $C_S$ / coverage / $\hat\rho$ as Market API.
    Quote {
        market: String,
        #[arg(long, default_value = "01")]
        mask: String,
        #[arg(long, default_value_t = 1)]
        shares: i64,
    },
}

#[derive(Subcommand)]
enum TradeCmd {
    BuySet {
        market: String,
        /// Hex bit mask, e.g. 01 for cell 0 when n≤8.
        mask: String,
        /// Integer shares (Q64 integer part).
        shares: i64,
    },
    SellSet {
        market: String,
        mask: String,
        shares: i64,
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
    },
    Payout {
        market: String,
        mask: String,
        #[arg(long)]
        owner: Option<String>,
    },
    FundCm {
        market: String,
        amount: u64,
    },
}

fn pad32(s: &str) -> [u8; 32] {
    let mut o = [b'_'; 32];
    let b = s.as_bytes();
    o[..b.len().min(32)].copy_from_slice(&b[..b.len().min(32)]);
    o
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

fn load_book(rpc: &RpcClient, market: &Pubkey) -> Result<(quote::Book, u16)> {
    let mkt = client::decode_market(&rpc.get_account(market).context("market")?.data)
        .map_err(|e| anyhow::anyhow!(e))?;
    let grid = client::decode_grid(&rpc.get_account(&client::grid_pda(market)).context("grid")?.data)
        .map_err(|e| anyhow::anyhow!(e))?;
    let mut trading_revenue = mkt.trading_revenue;
    let mut c_m = mkt.c_m;
    let mut premium_payable = 0u64;
    if let Ok(bacc) = rpc.get_account(&client::board(market)) {
        if let Ok(b) = client::decode_board(&bacc.data) {
            trading_revenue = b.trading_revenue;
            if b.c_m_locked > 0 {
                c_m = b.c_m_locked;
            }
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
            c_m,
            c_r,
            slot,
        ),
        mkt.n,
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
    Ok((close, close + 60))
}

fn common(
    me: Pubkey,
    id_hash: [u8; 32],
    n: u16,
    c_m: u64,
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
        c_m,
        fee_bps: 0,
        committee: me,
        members: vec![me],
        m: 1,
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

fn send(url: &str, payer: &Keypair, ix: solana_sdk::instruction::Instruction) -> Result<String> {
    send_ixs(url, payer, vec![ix], &[])
}

fn send_ixs(
    url: &str,
    payer: &Keypair,
    ixs: Vec<solana_sdk::instruction::Instruction>,
    extra: &[&Keypair],
) -> Result<String> {
    let rpc = RpcClient::new_with_commitment(url.to_string(), CommitmentConfig::confirmed());
    let bh = rpc.get_latest_blockhash()?;
    let cu = ComputeBudgetInstruction::set_compute_unit_limit(1_400_000);
    let mut all = vec![cu];
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
        Cmd::VaultInit => {
            let sig = send(&opt.url, &kp, client::initialize_vault(me))?;
            println!("ok {sig}");
        }
        Cmd::Deposit { amount } => {
            let sig = send(&opt.url, &kp, client::deposit(me, amount))?;
            println!("ok {sig} user={}", client::user_vault(&me));
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
            c_m,
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
                common: common(me, id_hash, n, c_m, close_ts, risk_lock_ts, challenge_secs),
                topic,
                tag,
                x_min: Q64::from_int(0).raw(),
                x_max: Q64::from_int(10).raw(),
                mu: Q64::from_int(5).raw(),
                sigma: Q64::from_int(2).raw(),
            };
            let sig = send(&opt.url, &kp, client::create_gaussian(me, id_hash, n, args))?;
            print_created(&sig, market, close_ts, challenge_secs);
        }
        Cmd::Market(MarketCmd::CreateLognormal {
            topic,
            tag,
            n,
            c_m,
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
                common: common(me, id_hash, n, c_m, close_ts, risk_lock_ts, challenge_secs),
                topic,
                tag,
                x_min: Q64::from_int(1).raw(),
                x_max: Q64::from_int(16).raw(),
                mu: Q64::from_int(2).raw(),
                sigma: Q64::from_int(1).raw(),
            };
            let sig = send(&opt.url, &kp, client::create_lognormal(me, id_hash, n, args))?;
            print_created(&sig, market, close_ts, challenge_secs);
        }
        Cmd::Market(MarketCmd::CreateDirichlet {
            topic,
            n,
            c_m,
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
                common: common(me, id_hash, n, c_m, close_ts, risk_lock_ts, challenge_secs),
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
            c_m,
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
                common: common(me, id_hash, n, c_m, close_ts, risk_lock_ts, challenge_secs),
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
            c_m,
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
                common: common(me, id_hash, n, c_m, close_ts, risk_lock_ts, challenge_secs),
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
            println!("ok {sig} halted (undelegate is ER, later)");
        }
        Cmd::Market(MarketCmd::Pdf { market }) => {
            let market = Pubkey::from_str(&market)?;
            let rpc = RpcClient::new_with_commitment(opt.url.clone(), CommitmentConfig::confirmed());
            let (book, _) = load_book(&rpc, &market)?;
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
            let (book, n) = load_book(&rpc, &market)?;
            let bytes = parse_mask(&mask)?;
            let in_set = quote::decode_mask(&bytes, n as usize).map_err(|e| anyhow::anyhow!(e))?;
            let q = if shares > 0 { shares } else { 1 };
            let v = book.view(&in_set, Q64::from_int(q));
            println!(
                "market={market} slot={} n={} p_s_raw={} p_s_bps={} c_s_raw={} c_s_usdc={} coverage_bps={} rho_hat_bps={} l_max_usdc={} c_max_usdc={} r_net={}",
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
                v.r_net
            );
        }
        Cmd::Trade(TradeCmd::BuySet { market, mask, shares }) => {
            let market = Pubkey::from_str(&market)?;
            let set_mask = parse_mask(&mask)?;
            let sig = send(
                &opt.url,
                &kp,
                client::buy_set(me, market, set_mask, Q64::from_int(shares).raw()),
            )?;
            println!("ok {sig} shares={shares}");
        }
        Cmd::Trade(TradeCmd::SellSet { market, mask, shares }) => {
            let market = Pubkey::from_str(&market)?;
            let set_mask = parse_mask(&mask)?;
            let sig = send(
                &opt.url,
                &kp,
                client::sell_set(me, market, set_mask, Q64::from_int(shares).raw()),
            )?;
            println!("ok {sig} shares={shares}");
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
        Cmd::Settle(SettleCmd::FundCm { market, amount }) => {
            let market = Pubkey::from_str(&market)?;
            let sig = send(&opt.url, &kp, client::fund_cm(me, market, amount))?;
            println!("ok {sig} board={}", client::board(&market));
        }
        Cmd::Settle(SettleCmd::Begin { market, risk }) => {
            let market = Pubkey::from_str(&market)?;
            let sig = send(&opt.url, &kp, client::begin_settle(market, risk))?;
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
        Cmd::Keeper => {
            println!("stub: keeper loop is Phase 5/8 (Delegate / Commit / undelegate).");
        }
        Cmd::IndexStatus => {
            let rpc = RpcClient::new(opt.url);
            let slot = rpc.get_slot().context("rpc")?;
            println!("rpc_ok slot={slot}");
        }
    }
    Ok(())
}
