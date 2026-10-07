//! Phase 2 acceptance (FR-SET-03): deposit → create a CPI (Gaussian) board →
//! two buys → committee `submit_result` → settle with one global ρ < 1.
//! FIFO is forbidden: both winners are paid, same ρ, sum ≤ C_max.

use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use math::Q64;
use solana_program_test::{ProgramTest, ProgramTestContext};
use solana_sdk::account::Account;
use solana_sdk::clock::Clock;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::system_instruction;
use solana_sdk::system_program;
use solana_sdk::transaction::Transaction;

const N: u16 = 8;
const C_M: u64 = 0;
const Q_A: i64 = 60;
const Q_B: i64 = 40;

fn q(n: i64) -> i128 {
    Q64::from_int(n).raw()
}

fn mask_cell0() -> Vec<u8> {
    vec![0b0000_0001]
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
    let ix = system_instruction::transfer(&ctx.payer.pubkey(), to, lamports);
    send(ctx, vec![ix], &[]).await;
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

async fn send_signed(ctx: &mut ProgramTestContext, ixs: Vec<Instruction>, extra: &[&Keypair]) {
    send(ctx, ixs, extra).await;
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

async fn create_ata_and_mint(
    ctx: &mut ProgramTestContext,
    mint_authority: &Keypair,
    owner: &Pubkey,
    amount: u64,
) {
    let dest = ata(owner);
    let create = spl_associated_token_account::instruction::create_associated_token_account(
        &ctx.payer.pubkey(),
        owner,
        &vault::USDC_MINT,
        &anchor_spl::token::ID,
    );
    let mint_to = spl_token::instruction::mint_to(
        &anchor_spl::token::ID,
        &vault::USDC_MINT,
        &dest,
        &mint_authority.pubkey(),
        &[],
        amount,
    )
    .unwrap();
    send(ctx, vec![create, mint_to], &[mint_authority]).await;
}

async fn account_data(ctx: &mut ProgramTestContext, key: &Pubkey) -> Vec<u8> {
    ctx.banks_client
        .get_account(*key)
        .await
        .unwrap()
        .unwrap()
        .data
}

#[tokio::test]
async fn deposit_create_cpi_buy_submit_settle_same_rho() {
    let (mut ctx, mint_auth) = start().await;
    let creator = Keypair::new();
    let alice = Keypair::new();
    let bob = Keypair::new();
    for k in [&creator, &alice, &bob] {
        air_drop(&mut ctx, &k.pubkey(), 10_000_000_000).await;
        create_ata_and_mint(&mut ctx, &mint_auth, &k.pubkey(), 1_000_000).await;
    }

    let (config, _) = Pubkey::find_program_address(&[vault::VAULT_SEED], &vault::ID);
    let vault_ata = ata(&config);
    let payer_pk = ctx.payer.pubkey();

    send_signed(
        &mut ctx,
        vec![Instruction {
            program_id: vault::ID,
            accounts: vault::accounts::Initialize {
                payer: payer_pk,
                usdc_mint: vault::USDC_MINT,
                config,
                vault_ata,
                token_program: anchor_spl::token::ID,
                associated_token_program: anchor_spl::associated_token::ID,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: vault::instruction::Initialize {}.data(),
        }],
        &[],
    )
    .await;

    for owner in [&creator, &alice, &bob] {
        let (user, _) = Pubkey::find_program_address(&[vault::USER_SEED, owner.pubkey().as_ref()], &vault::ID);
        send_signed(
            &mut ctx,
            vec![Instruction {
                program_id: vault::ID,
                accounts: vault::accounts::Deposit {
                    owner: owner.pubkey(),
                    config,
                    usdc_mint: vault::USDC_MINT,
                    vault_ata,
                    user_ata: ata(&owner.pubkey()),
                    user,
                    token_program: anchor_spl::token::ID,
                    system_program: system_program::ID,
                }
                .to_account_metas(None),
                data: vault::instruction::Deposit { amount: 50_000 }.data(),
            }],
            &[owner],
        )
        .await;
    }

    let topic = *b"us-cpi-yoy______________________";
    let tag = *b"first-print_____________________";
    let id_hash = market::ids::interval(1, &topic, &tag);
    let (market_pda, _) = Pubkey::find_program_address(&[market::state::MARKET_SEED, &id_hash], &market::ID);
    let (grid_pda, _) = Pubkey::find_program_address(&[market::state::GRID_SEED, market_pda.as_ref()], &market::ID);

    let common = market::state::CreateCommon {
        id_hash,
        n: N,
        close_ts: 1_000,
        risk_lock_ts: 1_000,
        beta: q(100),
        c_m: C_M,
        fee_bps: 0,
        fee_timing: 0,
        authorized_reporter: Pubkey::default(),
        report_window_secs: 400,
        challenge_secs: 20,
        n_layers: 1,
        d_unit: 10,
        gamma_bps: 1_000,
        alpha_r_bps: 7_000,
        platform: creator.pubkey(),
        report_open_ts: 1_000,
        committee_bond: 1,
    };
    let args = market::state::IntervalArgs {
        common,
        topic,
        tag,
        x_min: q(0),
        x_max: q(10),
        mu: q(5),
        sigma: q(2),
    };

    let (committee, _) = Pubkey::find_program_address(&[market::state::COMMITTEE_SEED], &market::ID);
    send_signed(
        &mut ctx,
        vec![Instruction {
            program_id: market::ID,
            accounts: market::accounts::InitCommittee {
                authority: creator.pubkey(),
                committee,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: market::instruction::InitCommittee {
                members: vec![creator.pubkey()],
                m: 1,
            }
            .data(),
        }],
        &[&creator],
    )
    .await;

    send_signed(
        &mut ctx,
        vec![client::init_protocol(
            creator.pubkey(),
            market::state::ProtocolArgs {
                platform: creator.pubkey(),
                fee_bps: 0,
                fee_timing: 0,
                report_window_secs: 400,
                challenge_secs: 20,
                committee_bond: 1,
                tap_cap_max: 0,
                alpha_r_bps: 7_000,
            },
        )],
        &[&creator],
    )
    .await;

    send_signed(
        &mut ctx,
        vec![Instruction {
            program_id: market::ID,
            accounts: market::accounts::CreateBoard {
                creator: creator.pubkey(),
                market: market_pda,
                grid: grid_pda,
                protocol: client::protocol_pda(),
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: market::instruction::CreateGaussianMarket {
                id_hash,
                n: N,
                args,
            }
            .data(),
        }],
        &[&creator],
    )
    .await;

    let (board, _) = Pubkey::find_program_address(&[vault::BOARD_SEED, market_pda.as_ref()], &vault::ID);
    let (creator_vault, _) =
        Pubkey::find_program_address(&[vault::USER_SEED, creator.pubkey().as_ref()], &vault::ID);
    send_signed(
        &mut ctx,
        vec![Instruction {
            program_id: vault::ID,
            accounts: vault::accounts::FundCm {
                owner: creator.pubkey(),
                market: market_pda,
                market_key: market_pda,
                board,
                user: creator_vault,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: vault::instruction::FundCm { amount: C_M }.data(),
        }],
        &[&creator],
    )
    .await;

    let set_mask = mask_cell0();
    let set_h = market::ids::set_hash(&set_mask);
    for (trader, qty) in [(&alice, Q_A), (&bob, Q_B)] {
        send_signed(
            &mut ctx,
            vec![client::buy_set(
                trader.pubkey(),
                market_pda,
                set_mask.clone(),
                q(qty),
                1,
            )],
            &[trader],
        )
        .await;
    }

    let (record, _) = Pubkey::find_program_address(&[resolution::RES_SEED, market_pda.as_ref()], &resolution::ID);
    send_signed(
        &mut ctx,
        vec![Instruction {
            program_id: resolution::ID,
            accounts: resolution::accounts::Open {
                payer: creator.pubkey(),
                market: market_pda,
                committee,
                record,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: resolution::instruction::Open {}.data(),
        }],
        &[&creator],
    )
    .await;

    set_time(&mut ctx, 1_000).await;
    let outcome = resolution::Outcome {
        family: 1,
        kind: 1,
        a: q(0),
        b: 0,
        shares: [0; 8],
    };
    send_signed(
        &mut ctx,
        vec![Instruction {
            program_id: resolution::ID,
            accounts: resolution::accounts::MutRecord {
                reporter: creator.pubkey(),
                record,
            }
            .to_account_metas(None),
            data: resolution::instruction::SubmitResult {
                outcome,
                evidence_hash: [0u8; 32],
            }
            .data(),
        }],
        &[&creator],
    )
    .await;

    set_time(&mut ctx, 1_030).await;
    send_signed(
        &mut ctx,
        vec![Instruction {
            program_id: resolution::ID,
            accounts: resolution::accounts::Finalize {
                reporter: creator.pubkey(),
                record,
                market: market_pda,
            }
            .to_account_metas(None),
            data: resolution::instruction::Finalize {}.data(),
        }],
        &[&creator],
    )
    .await;

    send_signed(
        &mut ctx,
        vec![Instruction {
            program_id: vault::ID,
            accounts: {
                let mut metas = vault::accounts::BeginSettle {
                    board,
                    market: market_pda,
                    grid: grid_pda,
                    record,
                    risk_book: None,
                    pool: None,
                    tap: None,
                }
                .to_account_metas(None);
                metas.retain(|m| m.pubkey != Pubkey::default());
                metas
            },
            data: vault::instruction::BeginSettle {}.data(),
        }],
        &[],
    )
    .await;

    let board_data = account_data(&mut ctx, &board).await;
    let board_acc = vault::Board::try_deserialize(&mut board_data.as_slice()).unwrap();
    assert!(
        board_acc.rho_raw < Q64::ONE.raw(),
        "fixture must be underfunded: rho={}",
        board_acc.rho_raw
    );
    assert_eq!(board_acc.liability, (Q_A + Q_B) as u64);
    assert!(board_acc.c_max < board_acc.liability);
    assert_eq!(board_acc.surplus, 0);
    assert_eq!(board_acc.cell, 0);

    let mut paid = Vec::new();
    for trader in [&alice, &bob] {
        let (pos, _) = Pubkey::find_program_address(
            &[
                market::state::POS_SEED,
                market_pda.as_ref(),
                trader.pubkey().as_ref(),
                set_h.as_ref(),
            ],
            &market::ID,
        );
        let (uv, _) = Pubkey::find_program_address(&[vault::USER_SEED, trader.pubkey().as_ref()], &vault::ID);
        let (claim, _) = Pubkey::find_program_address(&[vault::CLAIM_SEED, pos.as_ref()], &vault::ID);
        send_signed(
            &mut ctx,
            vec![Instruction {
                program_id: vault::ID,
                accounts: vault::accounts::Payout {
                    payer: trader.pubkey(),
                    board,
                    market: market_pda,
                    grid: grid_pda,
                    record,
                    position: pos,
                    claim,
                    user: uv,
                    system_program: system_program::ID,
                }
                .to_account_metas(None),
                data: vault::instruction::Payout {
                    set_mask: set_mask.clone(),
                }
                .data(),
            }],
            &[trader],
        )
        .await;
        let claim_data = account_data(&mut ctx, &claim).await;
        let claim_acc = vault::Claim::try_deserialize(&mut claim_data.as_slice()).unwrap();
        paid.push(claim_acc.paid);
    }

    let (pay_a, pay_b) = (paid[0], paid[1]);
    assert!(pay_a > 0 && pay_b > 0, "both winners must be paid (no FIFO)");
    assert!(pay_a + pay_b <= board_acc.c_max);
    // Same ρ: 60:40 = 3:2, allow 1-unit dust from ⌊ρq⌋.
    assert!((pay_a as i64) * 2 - (pay_b as i64) * 3 <= 2);
    assert!((pay_b as i64) * 3 - (pay_a as i64) * 2 <= 2);
    // FIFO would dump C_max onto the first fill.
    assert!(pay_a < board_acc.c_max);
    assert!(pay_b < board_acc.c_max);

}
