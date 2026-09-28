import { PublicKey } from "@solana/web3.js";

export const VAULT_ID = new PublicKey("VaULt11111111111111111111111111111111111111");
export const MARKET_ID = new PublicKey("Market1111111111111111111111111111111111111");
export const RESOLUTION_ID = new PublicKey("Rso1111111111111111111111111111111111111111");
export const RISK_ID = new PublicKey("Rsk1111111111111111111111111111111111111111");
export const USDC_MINT = new PublicKey("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
export const TOKEN_PROGRAM = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
export const ASSOCIATED_TOKEN = new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
export const SYSTEM_PROGRAM = new PublicKey("11111111111111111111111111111111");

export const FAMILY = {
  Skellam: 0,
  Gaussian: 1,
  Lognormal: 2,
  Dirichlet: 3,
  Bernoulli: 4,
} as const;

export const IX_ALL_TRADES = 1 | 2 | 4 | 8;
