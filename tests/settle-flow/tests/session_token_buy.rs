//! In-process BanksClient (no `solana-test-validator`): protocol Session + SessionTokenV2 + `buy_set`.

use math::Q64;
use solana_program_test::{ProgramTest, ProgramTestContext};
use solana_sdk::account::Account;
use solana_sdk::clock::Clock;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::system_instruction;
use solana_sdk::transaction::Transaction;

fn q(n: i64) -> i128 {
    Q64::from_int(n).raw()
}

fn pack_mint(authority: &Pubkey) -> Vec<u8> {
    let mut data = vec![0u8; 82];
    data[0] = 1;
    data[4..36].copy_from_slice(authority.as_ref());
    data[44] = 6;
    data[45] = 1;
    data
}

async fn start() -> (ProgramTestContext, Keypair) {
    let mint_authority = Keypair::new();
    let mut pt = ProgramTest::default();
    pt.set_compute_max_units(1_400_000);
    pt.add_program("vault", vault::ID, None);
    pt.add_program("market", market::ID, None);
    pt.add_program("resolution", resolution::ID, None);
    // Native processor lifetime doesn't match ProgramTest 2.3; load session_keys.so (SBF_OUT_DIR).
    pt.add_program("session_keys", session_keys::ID, None);
    pt.add_account(
        vault::USDC_MINT,
        Account {
            lamports: 1_000_000_000,
            data: pack_mint(&mint_authority.pubkey()),
            owner: anchor_spl::token::ID,
            executable: false,
            rent_epoch: 0,
        },
    );
    let mut ctx = pt.start_with_context().await;
    air_drop(&mut ctx, &mint_authority.pubkey(), 10_000_000_000).await;
    set_time(&mut ctx, 100).await;
    (ctx, mint_authority)
}

async fn air_drop(ctx: &mut ProgramTestContext, to: &Pubkey, lamports: u64) {
    send(ctx, vec![system_instruction::transfer(&ctx.payer.pubkey(), to, lamports)], &[]).await;
}

async fn set_time(ctx: &mut ProgramTestContext, ts: i64) {
    let mut clock: Clock = ctx.banks_client.get_sysvar().await.unwrap();
    clock.unix_timestamp = ts;
    ctx.set_sysvar(&clock);
}

async fn send(ctx: &mut ProgramTestContext, ixs: Vec<Instruction>, extra: &[&Keypair]) {
    let payer = ctx.payer.insecure_clone();
    let blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let mut signers: Vec<&Keypair> = vec![&payer];
    signers.extend_from_slice(extra);
    let tx = Transaction::new_signed_with_payer(&ixs, Some(&payer.pubkey()), &signers, blockhash);
    ctx.banks_client.process_transaction(tx).await.unwrap();
}

async fn send_err(ctx: &mut ProgramTestContext, ixs: Vec<Instruction>, extra: &[&Keypair]) -> String {
    let payer = ctx.payer.insecure_clone();
    let blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let mut signers: Vec<&Keypair> = vec![&payer];
    signers.extend_from_slice(extra);
    let tx = Transaction::new_signed_with_payer(&ixs, Some(&payer.pubkey()), &signers, blockhash);
    format!("{:?}", ctx.banks_client.process_transaction(tx).await.unwrap_err())
}

fn sells_closed(msg: &str) {
    assert!(
        msg.contains("SellsClosed") || msg.contains("Custom(6031)"),
        "want SellsClosed, got {msg}"
    );
}

fn ata(owner: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[
            owner.as_ref(),
            anchor_spl::token::ID.as_ref(),
            vault::USDC_MINT.as_ref(),
        ],
        &anchor_spl::associated_token::ID,
    )
    .0
}

fn create_common(owner: Pubkey, id_hash: [u8; 32], n: u16) -> market::state::CreateCommon {
    market::state::CreateCommon {
        id_hash,
        n,
        close_ts: 1_000,
        risk_lock_ts: 1_000,
        beta: q(100),
        c_m: 0,
        fee_bps: 0,
        fee_timing: 0,
        authorized_reporter: Pubkey::default(),
        report_window_secs: 400,
        challenge_secs: 20,
        n_layers: 1,
        d_unit: 10,
        gamma_bps: 1_000,
        alpha_r_bps: 7_000,
        platform: owner,
        report_open_ts: 1_000,
        committee_bond: 1,
    }
}

async fn fund_owner(ctx: &mut ProgramTestContext, mint_auth: &Keypair, owner: &Keypair, session: &Keypair) {
    air_drop(ctx, &owner.pubkey(), 10_000_000_000).await;
    air_drop(ctx, &session.pubkey(), 10_000_000_000).await;
    let dest = ata(&owner.pubkey());
    let payer_pk = ctx.payer.pubkey();
    send(
        ctx,
        vec![
            spl_associated_token_account::instruction::create_associated_token_account(
                &payer_pk,
                &owner.pubkey(),
                &vault::USDC_MINT,
                &anchor_spl::token::ID,
            ),
            spl_token::instruction::mint_to(
                &anchor_spl::token::ID,
                &vault::USDC_MINT,
                &dest,
                &mint_auth.pubkey(),
                &[],
                1_000_000,
            )
            .unwrap(),
        ],
        &[mint_auth],
    )
    .await;
    send(ctx, vec![client::initialize_vault(payer_pk)], &[]).await;
    send(ctx, vec![client::deposit(owner.pubkey(), 50_000)], &[owner]).await;
    send(
        ctx,
        vec![client::init_committee(owner.pubkey(), vec![owner.pubkey()], 1)],
        &[owner],
    )
    .await;
    send(
        ctx,
        vec![client::init_protocol(
            owner.pubkey(),
            market::state::ProtocolArgs {
                platform: owner.pubkey(),
                fee_bps: 0,
                fee_timing: 0,
                report_window_secs: 400,
                challenge_secs: 20,
                committee_bond: 1,
                tap_cap_max: 0,
                alpha_r_bps: 7_000,
            },
        )],
        &[owner],
    )
    .await;
}

async fn open_both_sessions(
    ctx: &mut ProgramTestContext,
    owner: &Keypair,
    session: &Keypair,
    whitelist: Pubkey,
) {
    send(
        ctx,
        vec![client::open_session(
            owner.pubkey(),
            session.pubkey(),
            800,
            20_000,
            market::session::IX_ALL_TRADES,
            whitelist,
        )],
        &[owner],
    )
    .await;
    send(
        ctx,
        vec![client::create_session_token_v2(
            owner.pubkey(),
            session.pubkey(),
            owner.pubkey(),
            800,
            false,
        )],
        &[owner, session],
    )
    .await;
    let token = client::session_token_v2_pda(&owner.pubkey(), &session.pubkey());
    assert!(ctx.banks_client.get_account(token).await.unwrap().is_some());
}

async fn finish_grid(ctx: &mut ProgramTestContext, owner: &Keypair, market: Pubkey, n: u16) {
    let count = market::state::Grid::shard_count(n);
    for ix in 1..count {
        send(
            ctx,
            vec![client::create_grid_shard(owner.pubkey(), market, ix as u8)],
            &[owner],
        )
        .await;
        send(
            ctx,
            vec![client::write_grid_shard(owner.pubkey(), market, ix as u8)],
            &[owner],
        )
        .await;
    }
    send(ctx, vec![client::write_grid_mass(owner.pubkey(), market)], &[owner]).await;
    send(ctx, vec![client::seal_grid_n(owner.pubkey(), market, n)], &[owner]).await;
}

#[tokio::test]
async fn session_token_v2_then_buy_set() {
    let (mut ctx, mint_auth) = start().await;
    let owner = Keypair::new();
    let session = Keypair::new();
    fund_owner(&mut ctx, &mint_auth, &owner, &session).await;

    let topic = *b"session-token-buy_______________";
    let tag = *b"first___________________________";
    let id_hash = market::ids::interval(1, &topic, &tag);
    let market_pda = client::market_pda(&id_hash);
    let args = market::state::IntervalArgs {
        common: create_common(owner.pubkey(), id_hash, 8),
        topic,
        tag,
        x_min: q(0),
        x_max: q(10),
        mu: q(5),
        sigma: q(2),
    };
    send(
        &mut ctx,
        vec![client::create_gaussian(owner.pubkey(), id_hash, 8, args)],
        &[&owner],
    )
    .await;
    send(&mut ctx, vec![client::fund_cm(owner.pubkey(), market_pda, 0)], &[&owner]).await;
    open_both_sessions(&mut ctx, &owner, &session, market_pda).await;

    let mask = vec![0b0000_0001];
    send(
        &mut ctx,
        vec![client::buy_set_session(
            owner.pubkey(),
            session.pubkey(),
            market_pda,
            mask.clone(),
            q(3),
            1,
        )],
        &[&session],
    )
    .await;

    let pos = client::position_pda(&market_pda, &owner.pubkey(), &market::ids::set_hash(&mask));
    let data = ctx.banks_client.get_account(pos).await.unwrap().unwrap().data;
    let decoded = client::decode_position(&data).expect("position");
    assert!(decoded.q > 0, "session fill must credit shares");
}

#[tokio::test]
async fn session_token_v2_then_sell_set() {
    let (mut ctx, mint_auth) = start().await;
    let owner = Keypair::new();
    let session = Keypair::new();
    fund_owner(&mut ctx, &mint_auth, &owner, &session).await;

    let topic = *b"session-token-sell______________";
    let tag = *b"first___________________________";
    let id_hash = market::ids::interval(1, &topic, &tag);
    let market_pda = client::market_pda(&id_hash);
    let args = market::state::IntervalArgs {
        common: create_common(owner.pubkey(), id_hash, 8),
        topic,
        tag,
        x_min: q(0),
        x_max: q(10),
        mu: q(5),
        sigma: q(2),
    };
    send(
        &mut ctx,
        vec![client::create_gaussian(owner.pubkey(), id_hash, 8, args)],
        &[&owner],
    )
    .await;
    send(&mut ctx, vec![client::fund_cm(owner.pubkey(), market_pda, 0)], &[&owner]).await;
    open_both_sessions(&mut ctx, &owner, &session, market_pda).await;

    let mask = vec![0b0000_0001];
    send(
        &mut ctx,
        vec![client::buy_set_session(
            owner.pubkey(),
            session.pubkey(),
            market_pda,
            mask.clone(),
            q(5),
            1,
        )],
        &[&session],
    )
    .await;
    let msg = send_err(
        &mut ctx,
        vec![client::sell_set_session(
            owner.pubkey(),
            session.pubkey(),
            market_pda,
            mask.clone(),
            q(2),
            2,
        )],
        &[&session],
    )
    .await;
    sells_closed(&msg);

    let pos = client::position_pda(&market_pda, &owner.pubkey(), &market::ids::set_hash(&mask));
    let data = ctx.banks_client.get_account(pos).await.unwrap().unwrap().data;
    let decoded = client::decode_position(&data).expect("position");
    assert_eq!(decoded.q, q(5), "rejected sell must leave the buy intact");
}

#[tokio::test]
async fn session_token_v2_then_skellam_buy_sell() {
    let (mut ctx, mint_auth) = start().await;
    let owner = Keypair::new();
    let session = Keypair::new();
    fund_owner(&mut ctx, &mint_auth, &owner, &session).await;

    let topic = *b"session-token-skellam___________";
    let n = market::state::FOOTBALL_N;
    let id_hash = market::ids::skellam(&topic, 0);
    let market_pda = client::market_pda(&id_hash);
    let args = market::state::SkellamArgs {
        common: create_common(owner.pubkey(), id_hash, n),
        topic,
        score_scope: 0,
        kickoff_ts: 1_000,
        prior_kind: 0,
        lambda_home: q(1),
        lambda_away: q(1),
        dc_rho: 0,
    };
    send(
        &mut ctx,
        vec![client::create_skellam(owner.pubkey(), id_hash, n, args)],
        &[&owner],
    )
    .await;
    finish_grid(&mut ctx, &owner, market_pda, n).await;
    send(&mut ctx, vec![client::fund_cm(owner.pubkey(), market_pda, 0)], &[&owner]).await;
    open_both_sessions(&mut ctx, &owner, &session, market_pda).await;

    send(
        &mut ctx,
        vec![client::buy_skellam_set_session(
            owner.pubkey(),
            session.pubkey(),
            market_pda,
            0,
            0,
            0,
            q(3),
            1,
        )],
        &[&session],
    )
    .await;
    let msg = send_err(
        &mut ctx,
        vec![client::sell_skellam_set_session(
            owner.pubkey(),
            session.pubkey(),
            market_pda,
            0,
            0,
            0,
            q(1),
            2,
        )],
        &[&session],
    )
    .await;
    sells_closed(&msg);

    let pos = client::skellam_position(&market_pda, &owner.pubkey(), 0, 0, 0);
    let data = ctx.banks_client.get_account(pos).await.unwrap().unwrap().data;
    let decoded = client::decode_position(&data).expect("skellam position");
    assert_eq!(decoded.q, q(3), "rejected skellam sell must leave the buy intact");
}
