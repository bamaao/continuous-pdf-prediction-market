/** Shared wallet copy. Session is a trading key, not a second login. */

export const NEED_WALLET = "connect a wallet first";
export const NEED_WALLET_AND_MARKET = "connect a wallet and a market key";

/** Product verbs. Not 开户 / 开盘 / Finish opening / listing. */
export const OPEN_MARKET = "开通预测市场";
export const OPEN_RISK_AUCTION = "开放风险拍卖";
export const APPROVE_AND_OPEN_MARKET = "批准并开通预测市场";
export const RETRY_OPEN_MARKET = "继续开通预测市场";

/** One-line rule for pages that lock or move funds / write committee state. */
export const SESSION_BOARD_ONLY =
  "A trading Session can only buy and sell in a prediction market. This page uses the connected wallet.";
