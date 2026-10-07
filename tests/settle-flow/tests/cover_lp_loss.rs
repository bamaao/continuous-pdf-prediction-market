//! `cover_lp_loss` account metas: authority, Market (platform constraint), LossPool, LP UserVault.
//! Inner no-op when uncovered=0 still succeeds. Wrong first-account mapping is AccountOwnedByWrongProgram.

use anchor_lang::AccountSerialize;
use solana_program_test::{ProgramTest, ProgramTestContext};
use solana_sdk::account::Account;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::system_instruction;
use solana_sdk::transaction::Transaction;

fn pack_mint(authority: &Pubkey) -> Vec<u8> {
    let mut data = vec![0u8; 82];
    data[0] = 1;
    data[4..36].copy_from_slice(authority.as_ref());
    data[44] = 6;
    data[45] = 1;
    data
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

fn pack_market(platform: Pubkey) -> (Pubkey, Account) {
    let key = Pubkey::new_unique();
    let m = market::state::Market {
        family: 0,
        status: 3,
        bump: 1,
        grid_bump: 1,
        n: 8,
        fee_bps: 0,
        creator: platform,
        committee: Pubkey::default(),
        authorized_reporter: Pubkey::default(),
        close_ts: 1,
        risk_lock_ts: 1,
        report_window_secs: 1,
        challenge_secs: 1,
        n_layers: 1,
        gamma_bps: 0,
        d_unit: 1,
        beta: 0,
        c_m: 0,
        fees_accrued: 0,
        trading_revenue: 0,
        alpha_r_bps: 0,
        platform,
        l_max: 0,
        id_hash: [0; 32],
        extra: market::state::FamilyExtra {
            a: 0,
            b: 0,
            c: 0,
            d: 0,
            e: 0,
            f: 0,
            u0: 0,
            u1: 0,
            u2: 0,
            u3: 0,
        },
        fee_timing: 0,
        delegated: false,
        trades_root: [0; 32],
        commit_ts: 0,
        p0_sum: 0,
        seal_bits: 0,
        wide_z0: 0,
        wide_z: 0,
        wide_q: 0,
        wide_nonce: 0,
        wide_read: 0,
        wide_write: 0,
        wide_tag: 0,
        wide_flags: 0,
        report_open_ts: 1,
        committee_bond: 0,
    };
    let mut data = Vec::new();
    m.try_serialize(&mut data).unwrap();
    (
        key,
        Account {
            lamports: 1_000_000_000,
            data,
            owner: market::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
}

async fn send(ctx: &mut ProgramTestContext, ixs: Vec<solana_sdk::instruction::Instruction>, extra: &[&Keypair]) {
    let payer = ctx.payer.insecure_clone();
    let blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let mut signers: Vec<&Keypair> = vec![&payer];
    signers.extend_from_slice(extra);
    let tx = Transaction::new_signed_with_payer(&ixs, Some(&payer.pubkey()), &signers, blockhash);
    ctx.banks_client.process_transaction(tx).await.unwrap();
}

#[tokio::test]
async fn cover_lp_loss_accepts_platform_signer_and_noop_when_uncovered_zero() {
    let mint_authority = Keypair::new();
    let platform = Keypair::new();
    let (market, market_acc) = pack_market(platform.pubkey());
    let mut pt = ProgramTest::default();
    pt.set_compute_max_units(1_400_000);
    pt.add_program("vault", vault::ID, None);
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
    pt.add_account(market, market_acc);
    let mut ctx = pt.start_with_context().await;

    let payer_pk = ctx.payer.pubkey();
    send(
        &mut ctx,
        vec![system_instruction::transfer(&payer_pk, &platform.pubkey(), 10_000_000_000)],
        &[],
    )
    .await;
    send(
        &mut ctx,
        vec![system_instruction::transfer(
            &payer_pk,
            &mint_authority.pubkey(),
            10_000_000_000,
        )],
        &[],
    )
    .await;

    let dest = ata(&platform.pubkey());
    send(
        &mut ctx,
        vec![
            spl_associated_token_account::instruction::create_associated_token_account(
                &payer_pk,
                &platform.pubkey(),
                &vault::USDC_MINT,
                &anchor_spl::token::ID,
            ),
            spl_token::instruction::mint_to(
                &anchor_spl::token::ID,
                &vault::USDC_MINT,
                &dest,
                &mint_authority.pubkey(),
                &[],
                1_000_000,
            )
            .unwrap(),
        ],
        &[&mint_authority],
    )
    .await;

    send(&mut ctx, vec![client::initialize_vault(payer_pk)], &[]).await;
    send(&mut ctx, vec![client::deposit(platform.pubkey(), 1_000)], &[&platform]).await;
    send(&mut ctx, vec![client::init_pool(payer_pk)], &[]).await;
    send(
        &mut ctx,
        vec![client::cover_lp_loss(platform.pubkey(), market, platform.pubkey())],
        &[&platform],
    )
    .await;

    let loss = ctx
        .banks_client
        .get_account(client::loss_pool())
        .await
        .unwrap()
        .expect("loss pool");
    let pool = client::decode_loss_pool(&loss.data).unwrap();
    assert_eq!(pool.available, 0);
}
