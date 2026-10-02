//! Phase 7: after `delegate_book`, L1 `buy_set` is `Delegated`; `commit_book` does not touch Vault.

use anchor_lang::{InstructionData, ToAccountMetas};
use math::Q64;
use solana_program_test::{BanksClientError, ProgramTest, ProgramTestContext};
use solana_sdk::account::Account;
use solana_sdk::clock::Clock;
use solana_sdk::instruction::Instruction;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signer};
use solana_sdk::system_instruction;
use solana_sdk::system_program;
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

fn point_sbf_out() {
    let deploy = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/deploy");
    std::env::set_var("SBF_OUT_DIR", deploy);
}

async fn start() -> (ProgramTestContext, Keypair) {
    point_sbf_out();
    let mint_authority = Keypair::new();
    let mut pt = ProgramTest::default();
    pt.set_compute_max_units(1_400_000);
    pt.add_program("vault", vault::ID, None);
    pt.add_program("market", market::ID, None);
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
    send(ctx, vec![ix], &[]).await.unwrap();
}

async fn set_time(ctx: &mut ProgramTestContext, ts: i64) {
    let mut clock: Clock = ctx.banks_client.get_sysvar().await.unwrap();
    clock.unix_timestamp = ts;
    ctx.set_sysvar(&clock);
}

async fn send(
    ctx: &mut ProgramTestContext,
    ixs: Vec<Instruction>,
    extra: &[&Keypair],
) -> Result<(), BanksClientError> {
    let payer = ctx.payer.insecure_clone();
    let blockhash = ctx.banks_client.get_latest_blockhash().await.unwrap();
    let mut signers: Vec<&Keypair> = vec![&payer];
    signers.extend_from_slice(extra);
    let tx = Transaction::new_signed_with_payer(&ixs, Some(&payer.pubkey()), &signers, blockhash);
    ctx.banks_client.process_transaction(tx).await
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

fn halt_ix(authority: Pubkey, market: Pubkey, committee: Pubkey, data: Vec<u8>) -> Instruction {
    Instruction {
        program_id: market::ID,
        accounts: market::accounts::Halt {
            authority,
            market,
            committee,
        }
        .to_account_metas(None),
        data,
    }
}

#[tokio::test]
async fn delegate_rejects_l1_buy_commit_leaves_vault() {
    let (mut ctx, mint_auth) = start().await;
    let creator = Keypair::new();
    let alice = Keypair::new();
    for k in [&creator, &alice] {
        air_drop(&mut ctx, &k.pubkey(), 10_000_000_000).await;
        let dest = ata(&k.pubkey());
        let create = spl_associated_token_account::instruction::create_associated_token_account(
            &ctx.payer.pubkey(),
            &k.pubkey(),
            &vault::USDC_MINT,
            &anchor_spl::token::ID,
        );
        let mint_to = spl_token::instruction::mint_to(
            &anchor_spl::token::ID,
            &vault::USDC_MINT,
            &dest,
            &mint_auth.pubkey(),
            &[],
            1_000_000,
        )
        .unwrap();
        send(&mut ctx, vec![create, mint_to], &[&mint_auth]).await.unwrap();
    }

    let (config, _) = Pubkey::find_program_address(&[vault::VAULT_SEED], &vault::ID);
    let vault_ata = ata(&config);
    let payer_pk = ctx.payer.pubkey();
    send(
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
    .await
    .unwrap();

    for owner in [&creator, &alice] {
        let (user, _) = Pubkey::find_program_address(&[vault::USER_SEED, owner.pubkey().as_ref()], &vault::ID);
        send(
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
        .await
        .unwrap();
    }

    let topic = *b"phase7-delegate_________________";
    let tag = *b"er______________________________";
    let id_hash = market::ids::interval(1, &topic, &tag);
    let (market_pda, _) = Pubkey::find_program_address(&[market::state::MARKET_SEED, &id_hash], &market::ID);
    let (grid_pda, _) = Pubkey::find_program_address(&[market::state::GRID_SEED, market_pda.as_ref()], &market::ID);
    let (committee, _) = Pubkey::find_program_address(&[market::state::COMMITTEE_SEED], &market::ID);
    send(
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
    .await
    .unwrap();

    let args = market::state::IntervalArgs {
        common: market::state::CreateCommon {
            id_hash,
            n: 8,
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
            platform: creator.pubkey(),
        },
        topic,
        tag,
        x_min: q(0),
        x_max: q(10),
        mu: q(5),
        sigma: q(2),
    };
    send(
        &mut ctx,
        vec![Instruction {
            program_id: market::ID,
            accounts: market::accounts::CreateBoard {
                creator: creator.pubkey(),
                market: market_pda,
                grid: grid_pda,
                system_program: system_program::ID,
            }
            .to_account_metas(None),
            data: market::instruction::CreateGaussianMarket { id_hash, n: 8, args }.data(),
        }],
        &[&creator],
    )
    .await
    .unwrap();

    let (board, _) = Pubkey::find_program_address(&[vault::BOARD_SEED, market_pda.as_ref()], &vault::ID);
    let (creator_vault, _) =
        Pubkey::find_program_address(&[vault::USER_SEED, creator.pubkey().as_ref()], &vault::ID);
    send(
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
            data: vault::instruction::FundCm { amount: 0 }.data(),
        }],
        &[&creator],
    )
    .await
    .unwrap();

    let set_mask = vec![0b0000_0001];
    let (uv, _) = Pubkey::find_program_address(&[vault::USER_SEED, alice.pubkey().as_ref()], &vault::ID);
    let buy = |nonce: u64| client::buy_set(alice.pubkey(), market_pda, set_mask.clone(), q(1), nonce);
    send(&mut ctx, vec![buy(1)], &[&alice]).await.unwrap();

    send(
        &mut ctx,
        vec![client::delegate_book(creator.pubkey(), market_pda)],
        &[&creator],
    )
    .await
    .unwrap();

    let err = send(&mut ctx, vec![buy(2)], &[&alice]).await.unwrap_err();
    let msg = format!("{err:?}");
    assert!(
        msg.contains("Custom(6023)")
            || msg.contains("Custom(6024)")
            || msg.contains("Delegated"),
        "want Delegated, got {msg}"
    );

    let before_board = ctx.banks_client.get_account(board).await.unwrap().unwrap().data;
    let before_user = ctx.banks_client.get_account(uv).await.unwrap().unwrap().data;
    let root = [9u8; 32];
    send(
        &mut ctx,
        vec![client::commit_book(creator.pubkey(), market_pda, root)],
        &[&creator],
    )
    .await
    .unwrap();
    let after_board = ctx.banks_client.get_account(board).await.unwrap().unwrap().data;
    let after_user = ctx.banks_client.get_account(uv).await.unwrap().unwrap().data;
    assert_eq!(before_board, after_board, "commit must not write board");
    assert_eq!(before_user, after_user, "commit must not write user vault");

    send(
        &mut ctx,
        vec![halt_ix(
            creator.pubkey(),
            market_pda,
            committee,
            market::instruction::Halt {}.data(),
        )],
        &[&creator],
    )
    .await
    .unwrap();
    send(
        &mut ctx,
        vec![client::undelegate_book(creator.pubkey(), market_pda)],
        &[&creator],
    )
    .await
    .unwrap();
}
