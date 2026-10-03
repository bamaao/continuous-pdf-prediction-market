//! Instruction bytes from `crates/client` only (IR-10). The web app SHALL NOT
//! invent a second discriminator table.

use axum::http::StatusCode;
use axum::Json;
use base64::Engine;
use client::math::Q64;
use serde::{Deserialize, Serialize};
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

#[derive(Deserialize, Default)]
pub struct ComposeBody {
    pub op: String,
    pub owner: String,
    #[serde(default)]
    pub trader: Option<String>,
    #[serde(default)]
    pub market: Option<String>,
    #[serde(default)]
    pub amount: Option<u64>,
    #[serde(default)]
    pub mask: Option<String>,
    #[serde(default)]
    pub shares: Option<i64>,
    #[serde(default)]
    pub nonce: Option<u64>,
    #[serde(default)]
    pub authority: Option<String>,
    #[serde(default)]
    pub expires_ts: Option<i64>,
    #[serde(default)]
    pub remaining_usdc: Option<u64>,
    /// Top up session signer lamports when creating SessionTokenV2.
    #[serde(default)]
    pub top_up: Option<bool>,
    #[serde(default)]
    pub whitelist: Option<String>,
    #[serde(default)]
    pub value: Option<i64>,
    #[serde(default)]
    pub value_b: Option<i64>,
    #[serde(default)]
    pub family: Option<u8>,
    #[serde(default)]
    pub kind: Option<u8>,
    #[serde(default)]
    pub evidence_hex: Option<String>,
    #[serde(default)]
    pub layer: Option<u8>,
    #[serde(default)]
    pub capacity: Option<u64>,
    #[serde(default)]
    pub premium: Option<u64>,
    #[serde(default)]
    pub profit_share_bps: Option<u16>,
    #[serde(default)]
    pub topic: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub n: Option<u16>,
    #[serde(default)]
    pub close_ts: Option<i64>,
    #[serde(default)]
    pub risk_lock_ts: Option<i64>,
    #[serde(default)]
    pub c_m: Option<u64>,
    #[serde(default)]
    pub beta: Option<i64>,
    #[serde(default)]
    pub challenge_secs: Option<i64>,
    #[serde(default)]
    pub report_window_secs: Option<i64>,
    #[serde(default)]
    pub n_layers: Option<u8>,
    #[serde(default)]
    pub d_unit: Option<u64>,
    #[serde(default)]
    pub score_scope: Option<u8>,
    #[serde(default)]
    pub layout: Option<u8>,
    #[serde(default)]
    pub top_n: Option<u8>,
    #[serde(default)]
    pub bins: Option<u16>,
    /// Dirichlet α count for top-n / simplex (atoms use `n`).
    #[serde(default)]
    pub k: Option<u8>,
    #[serde(default)]
    pub early_resolve: Option<bool>,
    #[serde(default)]
    pub mu: Option<i64>,
    #[serde(default)]
    pub sigma: Option<i64>,
    #[serde(default)]
    pub x_min: Option<i64>,
    #[serde(default)]
    pub x_max: Option<i64>,
    /// When true, `mu` / `sigma` / `x_min` / `x_max` are thousandths (2.4 → 2400).
    #[serde(default)]
    pub milli: Option<bool>,
    #[serde(default)]
    pub lambda_home: Option<i64>,
    #[serde(default)]
    pub lambda_away: Option<i64>,
    #[serde(default)]
    pub for_challenge: Option<bool>,
    #[serde(default)]
    pub extensions: Option<u8>,
    #[serde(default)]
    pub position: Option<String>,
    #[serde(default)]
    pub set_hash: Option<String>,
    #[serde(default)]
    pub committee: Option<String>,
    #[serde(default)]
    pub members: Option<Vec<String>>,
    #[serde(default)]
    pub m: Option<u8>,
    #[serde(default)]
    pub weight_sum: Option<u64>,
    #[serde(default)]
    pub include_pool: Option<bool>,
    #[serde(default)]
    pub fee_bps: Option<u16>,
    #[serde(default)]
    pub fee_timing: Option<u8>,
    /// Skellam prior: 0=independent Poisson, 1=Dixon–Coles, 2=uniform.
    #[serde(default)]
    pub prior_kind: Option<u8>,
    /// Dixon–Coles ρ (milli when `milli=true`, e.g. 100 → 0.1).
    #[serde(default)]
    pub dc_rho: Option<i64>,
    /// Protocol fee claimant. If omitted, `PLATFORM_PUBKEY` or the create signer (local only).
    #[serde(default)]
    pub platform: Option<String>,
    /// When true, compose an ER fill (readonly vaults + remaining MAGIC program).
    #[serde(default)]
    pub er: Option<bool>,
    /// Grid shard index for `create_grid_shard` / `write_grid_shard`.
    #[serde(default)]
    pub ix: Option<u8>,
    /// Extra shard indices for `accum_seal` / `apply_seal`.
    #[serde(default)]
    pub extras: Option<Vec<u8>>,
}

#[derive(Serialize, Clone, Debug)]
pub struct CompKey {
    pub pubkey: String,
    pub is_signer: bool,
    pub is_writable: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct ComposeOut {
    pub program_id: String,
    pub keys: Vec<CompKey>,
    pub data_b64: String,
    /// Present when `buy_set` / `sell_set` must split into wide fill ixs (n large).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ixs: Option<Vec<ComposeOut>>,
}

pub fn pack(ix: Instruction) -> ComposeOut {
    ComposeOut {
        program_id: ix.program_id.to_string(),
        keys: ix
            .accounts
            .into_iter()
            .map(|m| CompKey {
                pubkey: m.pubkey.to_string(),
                is_signer: m.is_signer,
                is_writable: m.is_writable,
            })
            .collect(),
        data_b64: base64::engine::general_purpose::STANDARD.encode(ix.data),
        ixs: None,
    }
}

fn pk(s: &str) -> Result<Pubkey, StatusCode> {
    Pubkey::from_str(s.trim()).map_err(|_| StatusCode::BAD_REQUEST)
}

fn default_kind(family: u8) -> u8 {
    match family {
        0 => 0,
        1 | 2 => 1,
        3 => 2,
        4 => 4,
        _ => 1,
    }
}

fn hash32(hex: Option<&str>) -> Result<[u8; 32], StatusCode> {
    let Some(h) = hex.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok([0u8; 32]);
    };
    let h = h.trim_start_matches("0x");
    if h.len() != 64 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).map_err(|_| StatusCode::BAD_REQUEST)?;
    }
    Ok(out)
}

fn pad32(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    let b = s.as_bytes();
    let n = b.len().min(32);
    out[..n].copy_from_slice(&b[..n]);
    out
}

fn q_int(v: i64) -> i128 {
    Q64::from_int(v).raw()
}

fn q_scaled(v: i64, milli: bool) -> i128 {
    if milli {
        Q64::from_ratio(v, 1000).raw()
    } else {
        q_int(v)
    }
}

fn roster_from(body: &ComposeBody) -> Result<Vec<Pubkey>, StatusCode> {
    let list = body.members.as_ref().ok_or(StatusCode::BAD_REQUEST)?;
    if list.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    list.iter().map(|s| pk(s)).collect()
}

fn create_common(owner: Pubkey, body: &ComposeBody, id_hash: [u8; 32], n: u16) -> Result<client::market::state::CreateCommon, StatusCode> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let close_ts = body.close_ts.unwrap_or(now + 86_400);
    let risk_lock_ts = body.risk_lock_ts.unwrap_or(close_ts);
    Ok(client::market::state::CreateCommon {
        id_hash,
        n,
        close_ts,
        risk_lock_ts,
        beta: q_int(body.beta.unwrap_or(100)),
        c_m: 0,
        fee_bps: body.fee_bps.unwrap_or(0).min(10_000),
        fee_timing: if body.fee_timing.unwrap_or(0) == 1 { 1 } else { 0 },
        authorized_reporter: Pubkey::default(),
        report_window_secs: body.report_window_secs.unwrap_or(400).max(1),
        challenge_secs: body.challenge_secs.unwrap_or(3_600).max(1),
        n_layers: body.n_layers.unwrap_or(1).clamp(1, 8),
        d_unit: body.d_unit.unwrap_or(10).max(1),
        gamma_bps: 1_000,
        alpha_r_bps: 7_000,
        platform: platform_pubkey(owner, body)?,
    })
}

fn platform_pubkey(owner: Pubkey, body: &ComposeBody) -> Result<Pubkey, StatusCode> {
    if let Some(p) = body.platform.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return pk(p);
    }
    if let Ok(p) = std::env::var("PLATFORM_PUBKEY") {
        let t = p.trim();
        if !t.is_empty() {
            return pk(t);
        }
    }
    Ok(owner)
}

fn interval_bounds(body: &ComposeBody, family: u8) -> (i128, i128) {
    let milli = body.milli.unwrap_or(false);
    let n = body.n.unwrap_or(if milli { 256 } else { 8 });
    let (def_min, def_max) = if family == 2 && milli {
        (10_000_000, 250_000_000)
    } else if milli {
        (-2_000, 12_000)
    } else {
        (0, i64::from(n))
    };
    (
        q_scaled(body.x_min.unwrap_or(def_min), milli),
        q_scaled(body.x_max.unwrap_or(def_max), milli),
    )
}

fn settle_grid(market: Pubkey, body: &ComposeBody) -> Pubkey {
    if let Some(ix) = body.ix {
        return client::grid_shard_pda(&market, ix);
    }
    let n = body.n.unwrap_or(0);
    if n < 2 {
        return client::grid_pda(&market);
    }
    let o = outcome_from(body);
    let (extra_a, extra_b) = interval_bounds(body, o.family);
    client::winning_shard(
        market,
        n,
        o.family,
        10,
        extra_a,
        extra_b,
        o.kind,
        o.a,
        o.b,
    )
}

fn outcome_from(body: &ComposeBody) -> client::resolution::Outcome {
    let family = body.family.unwrap_or(1);
    let kind = body.kind.unwrap_or(default_kind(family));
    let value = body.value.unwrap_or(0);
    let a = if kind == 1 {
        q_scaled(value, body.milli.unwrap_or(false))
    } else {
        i128::from(value)
    };
    client::resolution::Outcome {
        family,
        kind,
        a,
        b: i128::from(body.value_b.unwrap_or(0)),
        shares: [0; 8],
    }
}

pub fn mask_bytes(s: &str) -> Result<Vec<u8>, StatusCode> {
    mask_hex(s)
}

fn mask_hex(s: &str) -> Result<Vec<u8>, StatusCode> {
    let h = s.trim().trim_start_matches("0x");
    if h.len() % 2 != 0 {
        return Err(StatusCode::BAD_REQUEST);
    }
    (0..h.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&h[i..i + 2], 16).map_err(|_| StatusCode::BAD_REQUEST))
        .collect()
}

pub fn build(body: &ComposeBody) -> Result<Instruction, StatusCode> {
    let owner = pk(&body.owner)?;
    match body.op.as_str() {
        "deposit" => Ok(client::deposit(owner, body.amount.ok_or(StatusCode::BAD_REQUEST)?)),
        "withdraw" => Ok(client::withdraw(owner, body.amount.ok_or(StatusCode::BAD_REQUEST)?)),
        "create_ata" => Ok(client::create_ata(owner, owner)),
        "vault_init" | "initialize_vault" => Ok(client::initialize_vault(owner)),
        "init_committee" => {
            let members = roster_from(body)?;
            Ok(client::init_committee(owner, members, body.m.unwrap_or(1).max(1)))
        }
        "set_roster" => {
            let members = roster_from(body)?;
            Ok(client::set_roster(owner, members, body.m.unwrap_or(1).max(1)))
        }
        "open_session" => {
            let authority = pk(body.authority.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            let expires = body.expires_ts.ok_or(StatusCode::BAD_REQUEST)?;
            let usdc = body.remaining_usdc.unwrap_or(20_000);
            let white = body
                .whitelist
                .as_deref()
                .map(pk)
                .transpose()?
                .unwrap_or_default();
            Ok(client::open_session(
                owner,
                authority,
                expires,
                usdc,
                client::market::session::IX_ALL_TRADES,
                white,
            ))
        }
        "create_session_token" => {
            let authority = pk(body.authority.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            let expires = body.expires_ts.ok_or(StatusCode::BAD_REQUEST)?;
            // fee_payer defaults to owner (compose payer).
            Ok(client::create_session_token_v2(
                owner,
                authority,
                owner,
                expires,
                body.top_up.unwrap_or(true),
            ))
        }
        "revoke_session" => Ok(client::revoke_session(owner)),
        "revoke_session_token" => {
            let authority = pk(body.authority.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::revoke_session_token_v2(owner, authority, owner))
        }
        "renew_session" => {
            let expires = body.expires_ts.ok_or(StatusCode::BAD_REQUEST)?;
            let usdc = body.remaining_usdc.unwrap_or(20_000);
            Ok(client::renew_session(owner, expires, usdc))
        }
        "buy_skellam_set" | "sell_skellam_set" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            let kind = body.kind.unwrap_or(0);
            let a = i16::try_from(body.value.unwrap_or(0)).map_err(|_| StatusCode::BAD_REQUEST)?;
            let b = i16::try_from(body.value_b.unwrap_or(0)).map_err(|_| StatusCode::BAD_REQUEST)?;
            let q = Q64::from_int(body.shares.unwrap_or(1)).raw();
            let nonce = body.nonce.unwrap_or(1);
            let sell = body.op == "sell_skellam_set";
            let on_er = body.er.unwrap_or(false);
            if let Some(trader) = body.trader.as_deref() {
                let t = pk(trader)?;
                if t != owner {
                    return Ok(if sell {
                        if on_er {
                            client::sell_skellam_set_session_er(owner, t, market, kind, a, b, q, nonce)
                        } else {
                            client::sell_skellam_set_session(owner, t, market, kind, a, b, q, nonce)
                        }
                    } else if on_er {
                        client::buy_skellam_set_session_er(owner, t, market, kind, a, b, q, nonce)
                    } else {
                        client::buy_skellam_set_session(owner, t, market, kind, a, b, q, nonce)
                    });
                }
            }
            Ok(if sell {
                if on_er {
                    client::sell_skellam_set_er(owner, market, kind, a, b, q, nonce)
                } else {
                    client::sell_skellam_set(owner, market, kind, a, b, q, nonce)
                }
            } else if on_er {
                client::buy_skellam_set_er(owner, market, kind, a, b, q, nonce)
            } else {
                client::buy_skellam_set(owner, market, kind, a, b, q, nonce)
            })
        }
        "payout_skellam" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            let kind = body.kind.unwrap_or(0);
            let a = i16::try_from(body.value.unwrap_or(0)).map_err(|_| StatusCode::BAD_REQUEST)?;
            let b = i16::try_from(body.value_b.unwrap_or(0)).map_err(|_| StatusCode::BAD_REQUEST)?;
            Ok(client::payout_skellam_on(
                owner,
                market,
                owner,
                kind,
                a,
                b,
                settle_grid(market, body),
            ))
        }
        "draw_lp" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::draw_lp(market, owner, body.layer.unwrap_or(1)))
        }
        "pay_premium" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::pay_premium(market, owner, body.layer.unwrap_or(1)))
        }
        "release_lp" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::release_lp(market, owner, body.layer.unwrap_or(1)))
        }
        "pay_surplus_lp" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::pay_surplus_lp(
                market,
                owner,
                body.layer.unwrap_or(1),
                body.weight_sum.unwrap_or(1),
            ))
        }
        "init_pool" => Ok(client::init_pool(owner)),
        "fund_pool" => Ok(client::fund_pool(owner, body.amount.ok_or(StatusCode::BAD_REQUEST)?)),
        "set_tap" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::set_tap(owner, market, body.amount.unwrap_or(0)))
        }
        "claim_fees" | "sweep_fees" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::claim_fees(owner, market))
        }
        "begin_refund" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::begin_refund(market))
        }
        "begin_settle" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::begin_settle_on(
                market,
                true,
                body.include_pool.unwrap_or(false),
                settle_grid(market, body),
            ))
        }
        "buy_set" | "sell_set" => fill_set_from_body(owner, body).map(|mut ixs| ixs.remove(0)),
        "payout" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            let mask = mask_hex(body.mask.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::payout_on(
                owner,
                market,
                owner,
                mask,
                settle_grid(market, body),
            ))
        }
        "refund" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            let position = pk(body.position.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::refund_position(owner, market, owner, position))
        }
        "grow_grid" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::grow_grid(owner, market))
        }
        "write_grid_mass" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::write_grid_mass(owner, market))
        }
        "create_grid_shard" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            let ix = body.ix.ok_or(StatusCode::BAD_REQUEST)?;
            Ok(client::create_grid_shard(owner, market, ix))
        }
        "write_grid_shard" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            let ix = body.ix.ok_or(StatusCode::BAD_REQUEST)?;
            Ok(client::write_grid_shard(owner, market, ix))
        }
        "accum_seal" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            let extras = body.extras.clone().unwrap_or_default();
            Ok(client::accum_seal(owner, market, &extras))
        }
        "apply_seal" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            let extras = body.extras.clone().unwrap_or_default();
            Ok(client::apply_seal(owner, market, &extras))
        }
        "seal_grid" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            let n = body.n.unwrap_or(8);
            Ok(client::seal_grid_n(owner, market, n))
        }
        "halt" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::halt(owner, market))
        }
        "delegate_book" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::delegate_book(owner, market))
        }
        "prepare_delegate_buffer" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::prepare_delegate_buffer(owner, client::grid_pda(&market)))
        }
        "commit_book" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::commit_book(
                owner,
                market,
                hash32(body.evidence_hex.as_deref())?,
            ))
        }
        "undelegate_book" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::undelegate_book(owner, market))
        }
        "fund_cm" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::fund_cm(owner, market, body.amount.unwrap_or(0)))
        }
        "create_skellam" | "create_gaussian" | "create_lognormal" | "create_dirichlet" | "create_bernoulli" => {
            if body.members.is_some() || body.committee.is_some() || body.m.is_some() {
                return Err(StatusCode::BAD_REQUEST);
            }
            let topic = pad32(body.topic.as_deref().unwrap_or("board"));
            let tag = pad32(body.tag.as_deref().unwrap_or("default"));
            match body.op.as_str() {
                "create_skellam" => {
                    let scope = body.score_scope.unwrap_or(0);
                    let id_hash = client::market::ids::skellam(&topic, scope);
                    let n = client::market::state::FOOTBALL_N;
                    let milli = body.milli.unwrap_or(false);
                    let dc = body.dc_rho.unwrap_or(0);
                    let prior_kind = body.prior_kind.unwrap_or(if dc != 0 { 1 } else { 0 });
                    let args = client::market::state::SkellamArgs {
                        common: create_common(owner, body, id_hash, n)?,
                        topic,
                        score_scope: scope,
                        kickoff_ts: body.close_ts.unwrap_or(0),
                        prior_kind,
                        lambda_home: q_scaled(body.lambda_home.unwrap_or(if milli { 1_400 } else { 1 }), milli),
                        lambda_away: q_scaled(body.lambda_away.unwrap_or(if milli { 1_100 } else { 1 }), milli),
                        dc_rho: q_scaled(dc, milli),
                    };
                    Ok(client::create_skellam(owner, id_hash, n, args))
                }
                "create_gaussian" | "create_lognormal" => {
                    let family = if body.op == "create_lognormal" { 2 } else { 1 };
                    let id_hash = client::market::ids::interval(family, &topic, &tag);
                    let milli = body.milli.unwrap_or(false);
                    let n = body.n.unwrap_or(if milli { 256 } else { 8 });
                    let (def_min, def_max, def_mu, def_sigma) = if family == 2 && milli {
                        (10_000_000, 250_000_000, 11_082, 250)
                    } else if milli {
                        (-2_000, 12_000, 2_400, 350)
                    } else {
                        (0, i64::from(n), i64::from(n) / 2, 2)
                    };
                    let args = client::market::state::IntervalArgs {
                        common: create_common(owner, body, id_hash, n)?,
                        topic,
                        tag,
                        x_min: q_scaled(body.x_min.unwrap_or(def_min), milli),
                        x_max: q_scaled(body.x_max.unwrap_or(def_max), milli),
                        mu: q_scaled(body.mu.unwrap_or(def_mu), milli),
                        sigma: q_scaled(body.sigma.unwrap_or(def_sigma), milli),
                    };
                    if body.op == "create_lognormal" {
                        Ok(client::create_lognormal(owner, id_hash, n, args))
                    } else {
                        Ok(client::create_gaussian(owner, id_hash, n, args))
                    }
                }
                "create_dirichlet" => {
                    let layout = body.layout.unwrap_or(client::market::state::DIRICHLET_ATOMS);
                    let top_n = body.top_n.unwrap_or(0);
                    let bins = body.bins.unwrap_or(0);
                    let id_hash = client::market::ids::dirichlet(&topic, layout, top_n, bins);
                    let n = body.n.unwrap_or(4);
                    let over = client::grid_space(n) > client::market::state::Grid::CREATE_CAP;
                    let k = match layout {
                        client::market::state::DIRICHLET_ATOMS => n as usize,
                        _ => {
                            let k = body.k.unwrap_or(2) as usize;
                            if !(2..=16).contains(&k) {
                                return Err(StatusCode::BAD_REQUEST);
                            }
                            k
                        }
                    };
                    if over && k > 4 {
                        return Err(StatusCode::BAD_REQUEST);
                    }
                    if layout == client::market::state::DIRICHLET_TOP_N {
                        let cells = client::math::prior::binom(k as u32, top_n as u32)
                            .ok_or(StatusCode::BAD_REQUEST)?;
                        if cells != n as u32 {
                            return Err(StatusCode::BAD_REQUEST);
                        }
                    }
                    if layout == client::market::state::DIRICHLET_SIMPLEX {
                        let cells = client::math::prior::simplex_cell_count(k as u32, bins as u32)
                            .ok_or(StatusCode::BAD_REQUEST)?;
                        if cells != n as u32 {
                            return Err(StatusCode::BAD_REQUEST);
                        }
                    }
                    let args = client::market::state::DirichletArgs {
                        common: create_common(owner, body, id_hash, n)?,
                        topic,
                        layout,
                        top_n,
                        bins,
                        alpha: vec![q_int(1); k],
                    };
                    Ok(client::create_dirichlet(owner, id_hash, n, args))
                }
                _ => {
                    let id_hash = client::market::ids::bernoulli(&topic, &tag);
                    let n = 2;
                    let args = client::market::state::BernoulliArgs {
                        common: create_common(owner, body, id_hash, n)?,
                        topic,
                        tag,
                        deadline_ts: body.close_ts.unwrap_or(0),
                        early_resolve: body.early_resolve.unwrap_or(false),
                        alpha_yes: q_int(1),
                        alpha_no: q_int(1),
                    };
                    Ok(client::create_bernoulli(owner, id_hash, n, args))
                }
            }
        }
        "submit_result" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::submit_result(
                owner,
                market,
                outcome_from(body),
                hash32(body.evidence_hex.as_deref())?,
            ))
        }
        "challenge" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::challenge(owner, market, outcome_from(body)))
        }
        "vote" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::vote(
                owner,
                market,
                body.for_challenge.unwrap_or(false),
                body.extensions.unwrap_or(0),
            ))
        }
        "resolve_open" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::resolve_open(owner, market))
        }
        "finalize" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::finalize(owner, market))
        }
        "void_resolution" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::void_resolution(owner, market))
        }
        "risk_open_book" | "open_book" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::risk_open_book(owner, market))
        }
        "risk_quote" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::risk_quote(
                owner,
                market,
                body.layer.unwrap_or(1),
                body.capacity.unwrap_or(100),
                body.premium.unwrap_or(1),
                body.profit_share_bps.unwrap_or(0),
            ))
        }
        "fill_next" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::fill_next(market, owner, body.layer.unwrap_or(1)))
        }
        "cancel_unfilled" => {
            let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
            Ok(client::cancel_unfilled(owner, market, body.layer.unwrap_or(1)))
        }
        _ => Err(StatusCode::BAD_REQUEST),
    }
}

fn fill_set_from_body(owner: Pubkey, body: &ComposeBody) -> Result<Vec<Instruction>, StatusCode> {
    let market = pk(body.market.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
    let mask = mask_hex(body.mask.as_deref().ok_or(StatusCode::BAD_REQUEST)?)?;
    let q = Q64::from_int(body.shares.unwrap_or(1)).raw();
    let nonce = body.nonce.unwrap_or(1);
    let is_buy = body.op == "buy_set";
    let on_er = body.er.unwrap_or(false);
    let (trader, session) = match body.trader.as_deref() {
        Some(t) => {
            let t = pk(t)?;
            if t != owner {
                (t, Some(client::session_pda(&owner)))
            } else {
                (owner, None)
            }
        }
        None => (owner, None),
    };
    Ok(client::fill_set_ixs(
        owner, trader, session, market, mask, q, nonce, is_buy, on_er,
    ))
}

pub fn build_ixs(body: &ComposeBody) -> Result<Vec<Instruction>, StatusCode> {
    if body.op == "buy_set" || body.op == "sell_set" {
        return fill_set_from_body(pk(&body.owner)?, body);
    }
    Ok(vec![build(body)?])
}

pub fn compose_out(body: &ComposeBody) -> Result<ComposeOut, StatusCode> {
    let ixs = build_ixs(body)?;
    let mut out = pack(ixs[0].clone());
    if ixs.len() > 1 {
        out.ixs = Some(ixs.into_iter().map(pack).collect());
    }
    Ok(out)
}

pub async fn compose(Json(body): Json<ComposeBody>) -> Result<Json<ComposeOut>, StatusCode> {
    Ok(Json(compose_out(&body)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deposit_bytes_match_client_crate() {
        let owner = Pubkey::new_unique();
        let want = client::deposit(owner, 42);
        let got = build(&ComposeBody {
            op: "deposit".into(),
            owner: owner.to_string(),
            amount: Some(42),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(want.program_id, got.program_id);
        assert_eq!(want.data, got.data);
        assert_eq!(want.accounts.len(), got.accounts.len());
    }

    #[test]
    fn session_buy_is_not_owner_buy() {
        let owner = Pubkey::new_unique();
        let trader = Pubkey::new_unique();
        let market = Pubkey::new_unique();
        let a = client::buy_set_session(owner, trader, market, vec![1], Q64::from_int(1).raw(), 1);
        let b = build(&ComposeBody {
            op: "buy_set".into(),
            owner: owner.to_string(),
            trader: Some(trader.to_string()),
            market: Some(market.to_string()),
            mask: Some("01".into()),
            shares: Some(1),
            nonce: Some(1),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(a.data, b.data);
        assert!(a.accounts.iter().any(|m| m.pubkey == client::session_pda(&owner)));
        assert!(a.accounts.iter().any(|m| m.pubkey == client::session_token_v2_pda(&owner, &trader)));
    }

    #[test]
    fn wide_session_buy_compose_splits_and_carries_token() {
        let owner = Pubkey::new_unique();
        let trader = Pubkey::new_unique();
        let market = Pubkey::new_unique();
        let token = client::session_token_v2_pda(&owner, &trader);
        let mask = "ff".repeat(128);
        let out = compose_out(&ComposeBody {
            op: "buy_set".into(),
            owner: owner.to_string(),
            trader: Some(trader.to_string()),
            market: Some(market.to_string()),
            mask: Some(mask),
            shares: Some(1),
            nonce: Some(1),
            ..Default::default()
        })
        .unwrap();
        let ixs = out.ixs.expect("n=1024 full mask must split");
        assert!(ixs.len() > 1);
        for ix in &ixs {
            assert!(ix.keys.iter().any(|k| k.pubkey == token.to_string()));
        }
    }

    #[test]
    fn grow_grid_bytes_match_client_crate() {
        let owner = Pubkey::new_unique();
        let market = Pubkey::new_unique();
        let want = client::grow_grid(owner, market);
        let got = build(&ComposeBody {
            op: "grow_grid".into(),
            owner: owner.to_string(),
            market: Some(market.to_string()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(want.program_id, got.program_id);
        assert_eq!(want.data, got.data);
        assert_eq!(client::grid_space(16), 1_101);
        assert_eq!(client::grid_space(128), 8_269);
        assert_eq!(client::grid_space(256), 16_461);
        assert_eq!(client::grid_grow_steps(256), 1);
        assert_eq!(client::grid_grow_steps(1024), 6);
        assert_eq!(client::grid_mass_steps(256), 4);
        assert_eq!(client::grid_mass_steps(1024), 16);
        let mass = client::write_grid_mass(owner, market);
        let seal = client::seal_grid(owner, market);
        let seal256 = client::seal_grid_n(owner, market, 256);
        assert_eq!(
            build(&ComposeBody {
                op: "write_grid_mass".into(),
                owner: owner.to_string(),
                market: Some(market.to_string()),
                ..Default::default()
            })
            .unwrap()
            .data,
            mass.data
        );
        assert_eq!(
            build(&ComposeBody {
                op: "seal_grid".into(),
                owner: owner.to_string(),
                market: Some(market.to_string()),
                ..Default::default()
            })
            .unwrap()
            .data,
            seal.data
        );
        let got256 = build(&ComposeBody {
            op: "seal_grid".into(),
            owner: owner.to_string(),
            market: Some(market.to_string()),
            n: Some(256),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(got256.data, seal256.data);
        assert!(got256.accounts.len() > seal.accounts.len());
    }

    #[test]
    fn begin_settle_points_at_winning_shard() {
        let market = Pubkey::new_unique();
        let shard0 = client::begin_settle_on(market, true, false, client::grid_pda(&market));
        let got = build(&ComposeBody {
            op: "begin_settle".into(),
            owner: Pubkey::new_unique().to_string(),
            market: Some(market.to_string()),
            family: Some(1),
            kind: Some(1),
            milli: Some(true),
            n: Some(256),
            value: Some(2_400),
            x_min: Some(-2_000),
            x_max: Some(12_000),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(got.program_id, shard0.program_id);
        assert_ne!(got.accounts, shard0.accounts);
    }

    #[test]
    fn halt_and_delegate_bytes_match_client_crate() {
        let owner = Pubkey::new_unique();
        let market = Pubkey::new_unique();
        let root = [7u8; 32];
        let hex: String = root.iter().map(|b| format!("{b:02x}")).collect();
        for (op, want) in [
            ("halt", client::halt(owner, market)),
            ("delegate_book", client::delegate_book(owner, market)),
            ("undelegate_book", client::undelegate_book(owner, market)),
            ("commit_book", client::commit_book(owner, market, root)),
        ] {
            let got = build(&ComposeBody {
                op: op.into(),
                owner: owner.to_string(),
                market: Some(market.to_string()),
                evidence_hex: Some(hex.clone()),
                ..Default::default()
            })
            .unwrap();
            assert_eq!(want.program_id, got.program_id, "{op}");
            assert_eq!(want.data, got.data, "{op}");
        }
    }

    #[test]
    fn renew_bytes_match_client_crate() {
        let owner = Pubkey::new_unique();
        let want = client::renew_session(owner, 99, 7);
        let got = build(&ComposeBody {
            op: "renew_session".into(),
            owner: owner.to_string(),
            expires_ts: Some(99),
            remaining_usdc: Some(7),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(want.data, got.data);
        assert_eq!(want.program_id, got.program_id);
    }

    #[test]
    fn sell_set_is_not_buy_set() {
        let owner = Pubkey::new_unique();
        let market = Pubkey::new_unique();
        let buy = build(&ComposeBody {
            op: "buy_set".into(),
            owner: owner.to_string(),
            market: Some(market.to_string()),
            mask: Some("01".into()),
            shares: Some(1),
            nonce: Some(1),
            ..Default::default()
        })
        .unwrap();
        let sell = build(&ComposeBody {
            op: "sell_set".into(),
            owner: owner.to_string(),
            market: Some(market.to_string()),
            mask: Some("01".into()),
            shares: Some(1),
            nonce: Some(1),
            ..Default::default()
        })
        .unwrap();
        assert_ne!(buy.data, sell.data);
        assert_eq!(buy.program_id, sell.program_id);
    }

    #[test]
    fn submit_result_uses_family_not_gaussian_default() {
        let owner = Pubkey::new_unique();
        let market = Pubkey::new_unique();
        let outcome = client::resolution::Outcome {
            family: 0,
            kind: 0,
            a: 2,
            b: 1,
            shares: [0; 8],
        };
        let want = client::submit_result(owner, market, outcome, [0u8; 32]);
        let got = build(&ComposeBody {
            op: "submit_result".into(),
            owner: owner.to_string(),
            market: Some(market.to_string()),
            family: Some(0),
            kind: Some(0),
            value: Some(2),
            value_b: Some(1),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(want.data, got.data);
        assert_eq!(want.program_id, got.program_id);
    }

    #[test]
    fn gaussian_milli_is_not_integer_cell_mu() {
        let owner = Pubkey::new_unique();
        let intg = build(&ComposeBody {
            op: "create_gaussian".into(),
            owner: owner.to_string(),
            topic: Some("cpi".into()),
            tag: Some("print".into()),
            n: Some(32),
            mu: Some(2),
            sigma: Some(1),
            ..Default::default()
        })
        .unwrap();
        let milli = build(&ComposeBody {
            op: "create_gaussian".into(),
            owner: owner.to_string(),
            topic: Some("cpi".into()),
            tag: Some("print".into()),
            n: Some(32),
            milli: Some(true),
            x_min: Some(-2_000),
            x_max: Some(12_000),
            mu: Some(2_400),
            sigma: Some(350),
            ..Default::default()
        })
        .unwrap();
        assert_ne!(intg.data, milli.data);
        assert!(build(&ComposeBody {
            op: "create_gaussian".into(),
            owner: owner.to_string(),
            topic: Some("cpi".into()),
            tag: Some("print".into()),
            members: Some(vec![owner.to_string()]),
            m: Some(1),
            ..Default::default()
        })
        .is_err());
    }

    #[test]
    fn submit_gaussian_milli_is_not_integer_x() {
        let owner = Pubkey::new_unique();
        let market = Pubkey::new_unique();
        let intg = build(&ComposeBody {
            op: "submit_result".into(),
            owner: owner.to_string(),
            market: Some(market.to_string()),
            family: Some(1),
            kind: Some(1),
            value: Some(2),
            ..Default::default()
        })
        .unwrap();
        let milli = build(&ComposeBody {
            op: "submit_result".into(),
            owner: owner.to_string(),
            market: Some(market.to_string()),
            family: Some(1),
            kind: Some(1),
            milli: Some(true),
            value: Some(2_400),
            ..Default::default()
        })
        .unwrap();
        assert_ne!(intg.data, milli.data);
    }

    #[test]
    fn dirichlet_simplex_over_cap_uses_k_alphas_and_bins() {
        let owner = Pubkey::new_unique();
        let n = 159u16;
        assert!(client::grid_space(n) > client::market::state::Grid::CREATE_CAP);
        let a = build(&ComposeBody {
            op: "create_dirichlet".into(),
            owner: owner.to_string(),
            topic: Some("ds".into()),
            n: Some(n),
            layout: Some(client::market::state::DIRICHLET_SIMPLEX),
            bins: Some(158),
            k: Some(2),
            ..Default::default()
        })
        .unwrap();
        let b = build(&ComposeBody {
            op: "create_dirichlet".into(),
            owner: owner.to_string(),
            topic: Some("ds".into()),
            n: Some(n),
            layout: Some(client::market::state::DIRICHLET_SIMPLEX),
            bins: Some(158),
            k: Some(2),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(a.data, b.data);
        let atoms = build(&ComposeBody {
            op: "create_dirichlet".into(),
            owner: owner.to_string(),
            topic: Some("ds".into()),
            n: Some(4),
            ..Default::default()
        })
        .unwrap();
        assert_ne!(a.data, atoms.data);
        let k4 = build(&ComposeBody {
            op: "create_dirichlet".into(),
            owner: owner.to_string(),
            topic: Some("d4".into()),
            n: Some(286),
            layout: Some(client::market::state::DIRICHLET_SIMPLEX),
            bins: Some(10),
            k: Some(4),
            ..Default::default()
        })
        .unwrap();
        assert_ne!(k4.data, a.data);
        let top = build(&ComposeBody {
            op: "create_dirichlet".into(),
            owner: owner.to_string(),
            topic: Some("top".into()),
            n: Some(6),
            layout: Some(client::market::state::DIRICHLET_TOP_N),
            top_n: Some(2),
            k: Some(4),
            ..Default::default()
        })
        .unwrap();
        assert_ne!(top.data, atoms.data);
        assert!(build(&ComposeBody {
            op: "create_dirichlet".into(),
            owner: owner.to_string(),
            topic: Some("top".into()),
            n: Some(5),
            layout: Some(client::market::state::DIRICHLET_TOP_N),
            top_n: Some(2),
            k: Some(4),
            ..Default::default()
        })
        .is_err());
    }

    #[test]
    fn finalize_matches_client() {
        let owner = Pubkey::new_unique();
        let market = Pubkey::new_unique();
        let want = client::finalize(owner, market);
        let got = build(&ComposeBody {
            op: "finalize".into(),
            owner: owner.to_string(),
            market: Some(market.to_string()),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(want.data, got.data);
        assert_eq!(want.program_id, got.program_id);
    }

    #[test]
    fn buy_skellam_is_not_buy_set() {
        let owner = Pubkey::new_unique();
        let market = Pubkey::new_unique();
        let sk = build(&ComposeBody {
            op: "buy_skellam_set".into(),
            owner: owner.to_string(),
            market: Some(market.to_string()),
            kind: Some(0),
            shares: Some(1),
            nonce: Some(1),
            ..Default::default()
        })
        .unwrap();
        let mask = build(&ComposeBody {
            op: "buy_set".into(),
            owner: owner.to_string(),
            market: Some(market.to_string()),
            mask: Some("01".into()),
            shares: Some(1),
            nonce: Some(1),
            ..Default::default()
        })
        .unwrap();
        assert_ne!(sk.data, mask.data);
        assert_eq!(sk.program_id, client::market::ID);
        let want = client::buy_skellam_set(owner, market, 0, 0, 0, Q64::from_int(1).raw(), 1);
        assert_eq!(sk.data, want.data);
        let pos = client::skellam_position(&market, &owner, 0, 0, 0);
        assert!(sk.accounts.iter().any(|m| m.pubkey == pos));
        let mask_hash = client::market::ids::set_hash(&[1]);
        let mask_pos = client::position_pda(&market, &owner, &mask_hash);
        assert_ne!(pos, mask_pos);
    }

    #[test]
    fn payout_skellam_bytes_match_client() {
        let owner = Pubkey::new_unique();
        let market = Pubkey::new_unique();
        let want = client::payout_skellam(owner, market, owner, 8, -1, 0);
        let got = build(&ComposeBody {
            op: "payout_skellam".into(),
            owner: owner.to_string(),
            market: Some(market.to_string()),
            kind: Some(8),
            value: Some(-1),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(want.data, got.data);
        assert_eq!(want.program_id, got.program_id);
    }

    #[test]
    fn draw_lp_bytes_match_client() {
        let owner = Pubkey::new_unique();
        let market = Pubkey::new_unique();
        let want = client::draw_lp(market, owner, 1);
        let got = build(&ComposeBody {
            op: "draw_lp".into(),
            owner: owner.to_string(),
            market: Some(market.to_string()),
            layer: Some(1),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(want.data, got.data);
        assert_eq!(want.program_id, got.program_id);
        assert_eq!(want.accounts.len(), got.accounts.len());
    }
}
