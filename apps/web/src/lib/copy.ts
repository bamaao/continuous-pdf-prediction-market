/** Shared wallet copy. Session is a trading key, not a second login. */

export const NEED_WALLET = "connect a wallet first";
export const NEED_WALLET_AND_MARKET = "connect a wallet and a market key";

/** Product verbs. Not “open an account” / Finish opening / listing. */
export const OPEN_MARKET = "open the prediction market";
export const OPEN_RISK_AUCTION = "open the risk auction";
export const APPROVE_AND_OPEN_MARKET = "Approve and open prediction market";
export const RETRY_OPEN_MARKET = "Continue opening prediction market";
export const ENTER_MARKET = "Open prediction market";

/** One-line rule for pages that lock or move funds / write committee state. */
export const SESSION_BOARD_ONLY =
  "A trading Session can only buy in a prediction market. Sells are closed. This page uses the connected wallet.";

export const OPEN_IN_PHANTOM = "Open in Phantom";
export const OPEN_IN_SOLFLARE = "Open in Solflare";
export const IN_WALLET_BROWSE_HINT =
  "This phone has no injected wallet. Open the site inside Phantom or Solflare so signing stays in that app. A home-screen icon cannot jump back from the wallet.";

/** Settlement gate after committee finalize (FR-SET-11). */
export const LOCK_RHO = "Lock ρ";
export const LOCK_RHO_HINT =
  "After finalize, lock L, C_max, and ρ before anyone can claim on Portfolio. Any connected main wallet may sign.";
export const OPEN_REFUNDS = "Open refunds";
export const CLAIM_ON_PORTFOLIO = "Claim on Portfolio";
export const LMSR_COST_HINT =
  "The same interval costs more after it is bought: C_S for the same q rises on hot cells. That is LMSR, not the clock and not a popularity fee. Cold or disjoint cells can get cheaper. Coverage does not change C_S.";
export const SELLS_CLOSED =
  "Sells are closed. A fill stays until settlement claim (or VOID refund). It is not unwound at the live LMSR price.";
