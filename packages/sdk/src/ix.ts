import { PublicKey, TransactionInstruction } from "@solana/web3.js";
import { sha256 } from "js-sha256";
import { ASSOCIATED_TOKEN, MARKET_ID, SYSTEM_PROGRAM, TOKEN_PROGRAM, USDC_MINT, VAULT_ID } from "./ids";
import { ata, boardPda, gridPda, noncePda, positionPda, sessionPda, userVault, vaultConfig } from "./pda";

function disc(name: string): Buffer {
  return Buffer.from(sha256.arrayBuffer(`global:${name}`)).subarray(0, 8);
}

function u64(n: bigint | number): Buffer {
  const b = Buffer.alloc(8);
  b.writeBigUInt64LE(BigInt(n));
  return b;
}

function i64(n: bigint | number): Buffer {
  const b = Buffer.alloc(8);
  b.writeBigInt64LE(BigInt(n));
  return b;
}

function i128(n: bigint): Buffer {
  const b = Buffer.alloc(16);
  b.writeBigUInt64LE(n & ((1n << 64n) - 1n), 0);
  b.writeBigInt64LE(n >> 64n, 8);
  return b;
}

function vecU8(bytes: Uint8Array): Buffer {
  const len = Buffer.alloc(4);
  len.writeUInt32LE(bytes.length);
  return Buffer.concat([len, Buffer.from(bytes)]);
}

function w(pubkey: PublicKey, signer = false) {
  return { pubkey, isSigner: signer, isWritable: true };
}
function r(pubkey: PublicKey, signer = false) {
  return { pubkey, isSigner: signer, isWritable: false };
}

export function depositIx(owner: PublicKey, amount: bigint): TransactionInstruction {
  const config = vaultConfig();
  return new TransactionInstruction({
    programId: VAULT_ID,
    keys: [
      w(owner, true),
      r(config),
      r(USDC_MINT),
      w(ata(config)),
      w(ata(owner)),
      w(userVault(owner)),
      r(TOKEN_PROGRAM),
      r(SYSTEM_PROGRAM),
    ],
    data: Buffer.concat([disc("deposit"), u64(amount)]),
  });
}

export function withdrawIx(owner: PublicKey, amount: bigint): TransactionInstruction {
  const config = vaultConfig();
  return new TransactionInstruction({
    programId: VAULT_ID,
    keys: [
      w(owner, true),
      r(config),
      w(ata(config)),
      w(ata(owner)),
      w(userVault(owner)),
      r(TOKEN_PROGRAM),
    ],
    data: Buffer.concat([disc("withdraw"), u64(amount)]),
  });
}

export function openSessionIx(
  owner: PublicKey,
  authority: PublicKey,
  expiresTs: number,
  remainingUsdc: bigint,
  allowedIx: number,
  whitelist: PublicKey,
): TransactionInstruction {
  return new TransactionInstruction({
    programId: MARKET_ID,
    keys: [w(owner, true), w(sessionPda(owner)), r(SYSTEM_PROGRAM)],
    data: Buffer.concat([
      disc("open_session"),
      Buffer.from(authority.toBytes()),
      i64(expiresTs),
      u64(remainingUsdc),
      Buffer.from([allowedIx]),
      Buffer.from(whitelist.toBytes()),
    ]),
  });
}

export function revokeSessionIx(owner: PublicKey): TransactionInstruction {
  return new TransactionInstruction({
    programId: MARKET_ID,
    keys: [r(owner, true), w(sessionPda(owner))],
    data: disc("revoke_session"),
  });
}

export function buySetIx(
  owner: PublicKey,
  trader: PublicKey,
  market: PublicKey,
  mask: Uint8Array,
  qRaw: bigint,
  nonce: bigint,
  withSession: boolean,
): TransactionInstruction {
  const keys = [
    w(trader, true),
    r(owner),
    ...(withSession ? [w(sessionPda(owner))] : []),
    w(market),
    w(gridPda(market)),
    w(positionPda(market, owner, mask)),
    w(boardPda(market)),
    w(userVault(owner)),
    w(noncePda(owner, market)),
    r(VAULT_ID),
    r(SYSTEM_PROGRAM),
  ];
  return new TransactionInstruction({
    programId: MARKET_ID,
    keys,
    data: Buffer.concat([disc("buy_set"), vecU8(mask), i128(qRaw), u64(nonce)]),
  });
}

export function qFromShares(shares: number): bigint {
  return BigInt(shares) << 64n;
}

export function createAtaIx(payer: PublicKey, owner: PublicKey): TransactionInstruction {
  return new TransactionInstruction({
    programId: ASSOCIATED_TOKEN,
    keys: [w(payer, true), w(ata(owner)), r(owner), r(USDC_MINT), r(SYSTEM_PROGRAM), r(TOKEN_PROGRAM)],
    data: Buffer.from([1]),
  });
}
