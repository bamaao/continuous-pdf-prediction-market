import { PublicKey } from "@solana/web3.js";
import { ASSOCIATED_TOKEN, MARKET_ID, RISK_ID, RESOLUTION_ID, TOKEN_PROGRAM, USDC_MINT, VAULT_ID } from "./ids";

export function ata(owner: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(
    [owner.toBuffer(), TOKEN_PROGRAM.toBuffer(), USDC_MINT.toBuffer()],
    ASSOCIATED_TOKEN,
  )[0];
}

export function vaultConfig(): PublicKey {
  return PublicKey.findProgramAddressSync([Buffer.from("vault")], VAULT_ID)[0];
}

export function userVault(owner: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync([Buffer.from("user"), owner.toBuffer()], VAULT_ID)[0];
}

export function boardPda(market: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync([Buffer.from("board"), market.toBuffer()], VAULT_ID)[0];
}

export function sessionPda(owner: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync([Buffer.from("session"), owner.toBuffer()], MARKET_ID)[0];
}

export function noncePda(owner: PublicKey, market: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("nonce"), owner.toBuffer(), market.toBuffer()],
    MARKET_ID,
  )[0];
}

/** Borsh size of one `Grid` shard — must match `market::state::Grid::space`. */
export function gridSpace(n: number): number {
  return 77 + 64 * n;
}

export const GRID_CREATE_CAP = 10_240;
export const GRID_PRIOR_CHUNK = 64;
export const GRID_SHARD_CELLS = 16;
const GRID_HDR = 61;
const GRID_Z_OFF = 45;

export function shardCount(n: number): number {
  if (!Number.isFinite(n) || n < 1) return 0;
  return Math.ceil(n / GRID_SHARD_CELLS);
}

export function shardLen(n: number, ix: number): number {
  const start = ix * GRID_SHARD_CELLS;
  return Math.min(GRID_SHARD_CELLS, Math.max(0, n - start));
}

export function gridGrowSteps(n: number): number {
  if (!Number.isFinite(n) || n < 2) return 0;
  const need = gridSpace(shardLen(n, 0));
  if (need <= GRID_CREATE_CAP) return 0;
  return Math.ceil((need - GRID_CREATE_CAP) / GRID_CREATE_CAP);
}

export function gridMassSteps(n: number): number {
  if (!Number.isFinite(n) || n < 2) return 0;
  return Math.ceil(shardLen(n, 0) / GRID_PRIOR_CHUNK);
}

/** Borsh `p0.len` after disc+market+n+start+bump+z. */
export function gridP0Len(data: Uint8Array): number {
  if (data.length < GRID_HDR + 4) return 0;
  return data[GRID_HDR]! | (data[GRID_HDR + 1]! << 8) | (data[GRID_HDR + 2]! << 16) | (data[GRID_HDR + 3]! << 24);
}

/** `Grid.z` is i128 LE at offset 45. Non-zero on shard 0 means `seal_grid` finished. */
export function gridZ(data: Uint8Array): bigint {
  if (data.length < GRID_Z_OFF + 16) return 0n;
  let x = 0n;
  for (let i = 0; i < 16; i++) {
    x |= BigInt(data[GRID_Z_OFF + i]!) << BigInt(8 * i);
  }
  return x;
}

/** `Market.n` after disc(8) + family/status/bump/grid_bump (4). */
export function marketN(data: Uint8Array): number {
  if (data.length < 14) return 0;
  return data[12]! | (data[13]! << 8);
}

export function gridPda(market: PublicKey): PublicKey {
  return gridShardPda(market, 0);
}

export function gridShardPda(market: PublicKey, ix: number): PublicKey {
  if (ix <= 0) {
    return PublicKey.findProgramAddressSync([Buffer.from("grid"), market.toBuffer()], MARKET_ID)[0];
  }
  return PublicKey.findProgramAddressSync(
    [Buffer.from("grid"), market.toBuffer(), Buffer.from([ix & 0xff])],
    MARKET_ID,
  )[0];
}

export function riskBookPda(market: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync([Buffer.from("risk"), market.toBuffer()], RISK_ID)[0];
}

export function recordPda(market: PublicKey): PublicKey {
  return PublicKey.findProgramAddressSync([Buffer.from("res"), market.toBuffer()], RESOLUTION_ID)[0];
}

/** Same digest as `market::ids::set_hash`. */
export function setHash(mask: Uint8Array): Buffer {
  return digest([Buffer.from("set"), Buffer.from(mask)]);
}

/** Same digest as `market::ids::skellam_ticket`. */
export function skellamTicketHash(kind: number, a: number, b: number): Buffer {
  const aa = Buffer.alloc(2);
  aa.writeInt16LE(a);
  const bb = Buffer.alloc(2);
  bb.writeInt16LE(b);
  return digest([Buffer.from("skset"), Buffer.from([kind & 0xff]), aa, bb]);
}

export function hashHex(buf: Buffer): string {
  return Buffer.from(buf).toString("hex");
}

function pad32(s: string): Buffer {
  const out = Buffer.alloc(32);
  Buffer.from(s).copy(out, 0, 0, Math.min(32, Buffer.byteLength(s)));
  return out;
}

export type ListingIdInput = {
  family: number;
  topic: string;
  tag?: string;
  scoreScope?: number;
  layout?: number;
  topN?: number;
  bins?: number;
};

/**
 * Same as `market::ids::{skellam,interval,dirichlet,bernoulli}`.
 * Dirichlet seeds are topic + layout + top_n + bins — never tag. Passing only
 * `{ family, topic, tag }` hashes the atoms layout (0, 0, 0) and will not match
 * a simplex / top-n create.
 */
export function listingIdHash(id: ListingIdInput): Buffer {
  const family = Number(id.family);
  const t = pad32(id.topic);
  const g = pad32(id.tag ?? "default");
  if (family === 0) return digest([Buffer.from("sk"), t, Buffer.from([Number(id.scoreScope ?? 0) & 0xff])]);
  if (family === 3) {
    const layout = Number(id.layout ?? 0) & 0xff;
    const topN = layout === 1 ? Number(id.topN ?? 0) & 0xff : 0;
    const bins = layout === 2 ? Number(id.bins ?? 0) & 0xffff : 0;
    const b = Buffer.alloc(2);
    b.writeUInt16LE(bins, 0);
    return digest([Buffer.from("di"), t, Buffer.from([layout]), Buffer.from([topN]), b]);
  }
  if (family === 4) return digest([Buffer.from("be"), t, g]);
  return digest([Buffer.from("iv"), Buffer.from([family]), t, g]);
}

export function marketPda(idHash: Buffer): PublicKey {
  return PublicKey.findProgramAddressSync([Buffer.from("market"), idHash], MARKET_ID)[0];
}

export function positionPda(market: PublicKey, owner: PublicKey, mask: Uint8Array): PublicKey {
  return PublicKey.findProgramAddressSync(
    [Buffer.from("pos"), market.toBuffer(), owner.toBuffer(), setHash(mask)],
    MARKET_ID,
  )[0];
}

function digest(parts: Buffer[]): Buffer {
  const st = [
    BigInt("0x736f6d6570736575"),
    BigInt("0x646f72616e646f6d"),
    BigInt("0x6c7967656e657261"),
    BigInt("0x7465646279746573"),
  ];
  const mask = BigInt("0xffffffffffffffff");
  const rotl = (x: bigint, n: number) => ((x << BigInt(n)) | (x >> BigInt(64 - n))) & mask;
  for (let n = 0; n < parts.length; n++) {
    const part = parts[n];
    st[0] = (st[0] ^ (BigInt(part.length) + (BigInt(n) << 32n))) & mask;
    for (let off = 0; off < part.length; off += 8) {
      let x = 0n;
      for (let i = 0; i < 8 && off + i < part.length; i++) {
        x |= BigInt(part[off + i]) << BigInt(8 * i);
      }
      st[0] = rotl((st[0] + x) & mask, 13);
      st[1] = (st[1] ^ st[0]) & mask;
      st[2] = rotl((st[2] + st[1]) & mask, 17);
      st[3] = (st[3] ^ st[2]) & mask;
      st[0] = (st[0] * BigInt("0x9E3779B97F4A7C15")) & mask;
    }
  }
  const out = Buffer.alloc(32);
  for (let i = 0; i < 4; i++) {
    out.writeBigUInt64LE(st[i], i * 8);
  }
  return out;
}
