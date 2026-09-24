//! Transaction composers. Instruction data is always `anchor_lang::InstructionData`
//! from the on-chain crates — no second discriminator table.

use anchor_lang::solana_program::instruction::Instruction;
use anchor_lang::{AccountDeserialize, InstructionData, ToAccountMetas};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::system_program;

pub use market;
pub use math;
pub use resolution;
pub use risk;
pub use vault;

pub fn ata(owner: &Pubkey) -> Pubkey {
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

pub fn vault_config() -> Pubkey {
    Pubkey::find_program_address(&[vault::VAULT_SEED], &vault::ID).0
}

pub fn user_vault(owner: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[vault::USER_SEED, owner.as_ref()], &vault::ID).0
}

pub fn board(market: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[vault::BOARD_SEED, market.as_ref()], &vault::ID).0
}

pub fn claim(position: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[vault::CLAIM_SEED, position.as_ref()], &vault::ID).0
}

pub fn market_pda(id_hash: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(&[market::state::MARKET_SEED, id_hash], &market::ID).0
}

pub fn grid_pda(market: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[market::state::GRID_SEED, market.as_ref()], &market::ID).0
}

pub fn position_pda(market: &Pubkey, owner: &Pubkey, set_hash: &[u8; 32]) -> Pubkey {
    Pubkey::find_program_address(
        &[
            market::state::POS_SEED,
            market.as_ref(),
            owner.as_ref(),
            set_hash,
        ],
        &market::ID,
    )
    .0
}

pub fn record_pda(market: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[resolution::RES_SEED, market.as_ref()], &resolution::ID).0
}

pub fn decode_market(data: &[u8]) -> Result<market::state::Market, String> {
    let mut cur = data;
    market::state::Market::try_deserialize(&mut cur).map_err(|e| e.to_string())
}

pub fn decode_grid(data: &[u8]) -> Result<market::state::Grid, String> {
    let mut cur = data;
    market::state::Grid::try_deserialize(&mut cur).map_err(|e| e.to_string())
}

pub fn decode_board(data: &[u8]) -> Result<vault::Board, String> {
    let mut cur = data;
    vault::Board::try_deserialize(&mut cur).map_err(|e| e.to_string())
}

pub fn decode_nonce(data: &[u8]) -> Result<market::session::FillNonce, String> {
    let mut cur = data;
    market::session::FillNonce::try_deserialize(&mut cur).map_err(|e| e.to_string())
}

pub fn decode_risk_book(data: &[u8]) -> Result<risk::RiskBook, String> {
    let mut cur = data;
    risk::RiskBook::try_deserialize(&mut cur).map_err(|e| e.to_string())
}

pub fn risk_book(market: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[risk::BOOK_SEED, market.as_ref()], &risk::ID).0
}

pub fn session_pda(owner: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(&[market::session::SESSION_SEED, owner.as_ref()], &market::ID).0
}

pub fn nonce_pda(owner: &Pubkey, market: &Pubkey) -> Pubkey {
    Pubkey::find_program_address(
        &[market::session::NONCE_SEED, owner.as_ref(), market.as_ref()],
        &market::ID,
    )
    .0
}

pub fn initialize_vault(payer: Pubkey) -> Instruction {
    let config = vault_config();
    Instruction {
        program_id: vault::ID,
        accounts: vault::accounts::Initialize {
            payer,
            usdc_mint: vault::USDC_MINT,
            config,
            vault_ata: ata(&config),
            token_program: anchor_spl::token::ID,
            associated_token_program: anchor_spl::associated_token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: vault::instruction::Initialize {}.data(),
    }
}

pub fn deposit(owner: Pubkey, amount: u64) -> Instruction {
    let config = vault_config();
    Instruction {
        program_id: vault::ID,
        accounts: vault::accounts::Deposit {
            owner,
            config,
            usdc_mint: vault::USDC_MINT,
            vault_ata: ata(&config),
            user_ata: ata(&owner),
            user: user_vault(&owner),
            token_program: anchor_spl::token::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: vault::instruction::Deposit { amount }.data(),
    }
}

pub fn withdraw(owner: Pubkey, amount: u64) -> Instruction {
    let config = vault_config();
    Instruction {
        program_id: vault::ID,
        accounts: vault::accounts::Withdraw {
            owner,
            config,
            vault_ata: ata(&config),
            user_ata: ata(&owner),
            user: user_vault(&owner),
            token_program: anchor_spl::token::ID,
        }
        .to_account_metas(None),
        data: vault::instruction::Withdraw { amount }.data(),
    }
}

pub fn open_session(
    owner: Pubkey,
    authority: Pubkey,
    expires_ts: i64,
    remaining_usdc: u64,
    allowed_ix: u8,
    whitelist: Pubkey,
) -> Instruction {
    Instruction {
        program_id: market::ID,
        accounts: market::accounts::OpenSession {
            owner,
            session: session_pda(&owner),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: market::instruction::OpenSession {
            authority,
            expires_ts,
            remaining_usdc,
            allowed_ix,
            whitelist,
        }
        .data(),
    }
}

pub fn renew_session(owner: Pubkey, expires_ts: i64, remaining_usdc: u64) -> Instruction {
    Instruction {
        program_id: market::ID,
        accounts: market::accounts::MutSession {
            owner,
            session: session_pda(&owner),
        }
        .to_account_metas(None),
        data: market::instruction::RenewSession {
            expires_ts,
            remaining_usdc,
        }
        .data(),
    }
}

pub fn revoke_session(owner: Pubkey) -> Instruction {
    Instruction {
        program_id: market::ID,
        accounts: market::accounts::MutSession {
            owner,
            session: session_pda(&owner),
        }
        .to_account_metas(None),
        data: market::instruction::RevokeSession {}.data(),
    }
}

pub fn fund_cm(owner: Pubkey, market: Pubkey, amount: u64) -> Instruction {
    Instruction {
        program_id: vault::ID,
        accounts: vault::accounts::FundCm {
            owner,
            market,
            market_key: market,
            board: board(&market),
            user: user_vault(&owner),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: vault::instruction::FundCm { amount }.data(),
    }
}

fn create_board_accounts(creator: Pubkey, id_hash: [u8; 32]) -> market::accounts::CreateBoard {
    let market_key = market_pda(&id_hash);
    market::accounts::CreateBoard {
        creator,
        market: market_key,
        grid: grid_pda(&market_key),
        system_program: system_program::ID,
    }
}

pub fn create_gaussian(creator: Pubkey, id_hash: [u8; 32], n: u16, args: market::state::IntervalArgs) -> Instruction {
    Instruction {
        program_id: market::ID,
        accounts: create_board_accounts(creator, id_hash).to_account_metas(None),
        data: market::instruction::CreateGaussianMarket { id_hash, n, args }.data(),
    }
}

pub fn create_lognormal(creator: Pubkey, id_hash: [u8; 32], n: u16, args: market::state::IntervalArgs) -> Instruction {
    Instruction {
        program_id: market::ID,
        accounts: create_board_accounts(creator, id_hash).to_account_metas(None),
        data: market::instruction::CreateLognormalMarket { id_hash, n, args }.data(),
    }
}

pub fn create_dirichlet(creator: Pubkey, id_hash: [u8; 32], n: u16, args: market::state::DirichletArgs) -> Instruction {
    Instruction {
        program_id: market::ID,
        accounts: create_board_accounts(creator, id_hash).to_account_metas(None),
        data: market::instruction::CreateDirichletMarket { id_hash, n, args }.data(),
    }
}

pub fn create_bernoulli(creator: Pubkey, id_hash: [u8; 32], n: u16, args: market::state::BernoulliArgs) -> Instruction {
    Instruction {
        program_id: market::ID,
        accounts: create_board_accounts(creator, id_hash).to_account_metas(None),
        data: market::instruction::CreateBernoulliMarket { id_hash, n, args }.data(),
    }
}

pub fn create_skellam(creator: Pubkey, id_hash: [u8; 32], n: u16, args: market::state::SkellamArgs) -> Instruction {
    Instruction {
        program_id: market::ID,
        accounts: create_board_accounts(creator, id_hash).to_account_metas(None),
        data: market::instruction::CreateSkellamMarket { id_hash, n, args }.data(),
    }
}

pub fn create_ata(payer: Pubkey, owner: Pubkey) -> Instruction {
    spl_associated_token_account::instruction::create_associated_token_account_idempotent(
        &payer,
        &owner,
        &vault::USDC_MINT,
        &anchor_spl::token::ID,
    )
}

pub fn mint_usdc(mint_authority: Pubkey, dest_owner: Pubkey, amount: u64) -> Instruction {
    spl_token::instruction::mint_to(
        &anchor_spl::token::ID,
        &vault::USDC_MINT,
        &ata(&dest_owner),
        &mint_authority,
        &[],
        amount,
    )
    .expect("mint_to")
}

pub fn pack_local_usdc_mint(mint_authority: &Pubkey) -> Vec<u8> {
    let mut data = vec![0u8; 82];
    data[0] = 1;
    data[4..36].copy_from_slice(mint_authority.as_ref());
    data[44] = 6;
    data[45] = 1;
    data
}

pub fn buy_set(owner: Pubkey, market: Pubkey, set_mask: Vec<u8>, q_raw: i128, nonce: u64) -> Instruction {
    trade_set(owner, owner, None, market, set_mask, q_raw, nonce, true)
}

pub fn buy_set_session(
    owner: Pubkey,
    trader: Pubkey,
    market: Pubkey,
    set_mask: Vec<u8>,
    q_raw: i128,
    nonce: u64,
) -> Instruction {
    trade_set(owner, trader, Some(session_pda(&owner)), market, set_mask, q_raw, nonce, true)
}

pub fn sell_set(owner: Pubkey, market: Pubkey, set_mask: Vec<u8>, q_raw: i128, nonce: u64) -> Instruction {
    trade_set(owner, owner, None, market, set_mask, q_raw, nonce, false)
}

pub fn sell_set_session(
    owner: Pubkey,
    trader: Pubkey,
    market: Pubkey,
    set_mask: Vec<u8>,
    q_raw: i128,
    nonce: u64,
) -> Instruction {
    trade_set(owner, trader, Some(session_pda(&owner)), market, set_mask, q_raw, nonce, false)
}

fn trade_set(
    owner: Pubkey,
    trader: Pubkey,
    session: Option<Pubkey>,
    market: Pubkey,
    set_mask: Vec<u8>,
    q_raw: i128,
    nonce: u64,
    is_buy: bool,
) -> Instruction {
    let set_h = market::ids::set_hash(&set_mask);
    let accounts = market::accounts::Trade {
        trader,
        owner,
        session,
        market,
        grid: grid_pda(&market),
        position: position_pda(&market, &owner, &set_h),
        board: board(&market),
        user_vault: user_vault(&owner),
        nonce_acc: nonce_pda(&owner, &market),
        vault_program: vault::ID,
        system_program: system_program::ID,
    }
    .to_account_metas(None);
    Instruction {
        program_id: market::ID,
        accounts,
        data: if is_buy {
            market::instruction::BuySet { set_mask, q_raw, nonce }.data()
        } else {
            market::instruction::SellSet { set_mask, q_raw, nonce }.data()
        },
    }
}

pub fn halt(authority: Pubkey, market: Pubkey) -> Instruction {
    Instruction {
        program_id: market::ID,
        accounts: market::accounts::Halt { authority, market }.to_account_metas(None),
        data: market::instruction::Halt {}.data(),
    }
}

pub fn resolve_open(payer: Pubkey, market: Pubkey) -> Instruction {
    Instruction {
        program_id: resolution::ID,
        accounts: resolution::accounts::Open {
            payer,
            market,
            record: record_pda(&market),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: resolution::instruction::Open {}.data(),
    }
}

pub fn submit_result(reporter: Pubkey, market: Pubkey, outcome: resolution::Outcome, evidence_hash: [u8; 32]) -> Instruction {
    Instruction {
        program_id: resolution::ID,
        accounts: resolution::accounts::MutRecord {
            reporter,
            record: record_pda(&market),
        }
        .to_account_metas(None),
        data: resolution::instruction::SubmitResult {
            outcome,
            evidence_hash,
        }
        .data(),
    }
}

pub fn finalize(reporter: Pubkey, market: Pubkey) -> Instruction {
    Instruction {
        program_id: resolution::ID,
        accounts: resolution::accounts::Finalize {
            reporter,
            record: record_pda(&market),
            market,
        }
        .to_account_metas(None),
        data: resolution::instruction::Finalize {}.data(),
    }
}

pub fn begin_settle(market: Pubkey, include_risk: bool) -> Instruction {
    let mut metas = vault::accounts::BeginSettle {
        board: board(&market),
        market,
        grid: grid_pda(&market),
        record: record_pda(&market),
        risk_book: if include_risk {
            Some(risk_book(&market))
        } else {
            None
        },
    }
    .to_account_metas(None);
    metas.retain(|m| m.pubkey != Pubkey::default());
    Instruction {
        program_id: vault::ID,
        accounts: metas,
        data: vault::instruction::BeginSettle {}.data(),
    }
}

pub fn payout(payer: Pubkey, market: Pubkey, owner: Pubkey, set_mask: Vec<u8>) -> Instruction {
    let set_h = market::ids::set_hash(&set_mask);
    let pos = position_pda(&market, &owner, &set_h);
    Instruction {
        program_id: vault::ID,
        accounts: vault::accounts::Payout {
            payer,
            board: board(&market),
            grid: grid_pda(&market),
            record: record_pda(&market),
            position: pos,
            claim: claim(&pos),
            user: user_vault(&owner),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: vault::instruction::Payout { set_mask }.data(),
    }
}

pub fn risk_open_book(payer: Pubkey, market: Pubkey) -> Instruction {
    Instruction {
        program_id: risk::ID,
        accounts: risk::accounts::OpenBook {
            payer,
            market,
            book: risk_book(&market),
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: risk::instruction::OpenBook {}.data(),
    }
}

pub fn risk_quote(
    lp: Pubkey,
    market: Pubkey,
    layer_id: u8,
    capacity: u64,
    premium: u64,
    profit_share_bps: u16,
) -> Instruction {
    let (layer, _) = Pubkey::find_program_address(&[risk::LAYER_SEED, market.as_ref(), &[layer_id]], &risk::ID);
    let (quote, _) =
        Pubkey::find_program_address(&[risk::QUOTE_SEED, market.as_ref(), lp.as_ref(), &[layer_id]], &risk::ID);
    let (seat, _) = Pubkey::find_program_address(&[risk::SEAT_SEED, market.as_ref(), lp.as_ref()], &risk::ID);
    Instruction {
        program_id: risk::ID,
        accounts: risk::accounts::QuoteLayer {
            lp,
            market,
            book: risk_book(&market),
            layer,
            quote,
            seat,
            user_vault: user_vault(&lp),
            vault_program: vault::ID,
            system_program: system_program::ID,
        }
        .to_account_metas(None),
        data: risk::instruction::Quote {
            layer_id,
            capacity,
            premium,
            profit_share_bps,
        }
        .data(),
    }
}
