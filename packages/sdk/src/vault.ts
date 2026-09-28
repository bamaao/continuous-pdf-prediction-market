import { Connection, PublicKey } from "@solana/web3.js";
import { ata, userVault } from "./pda";

export type UserVaultSnap = {
  owner: string;
  exists: boolean;
  available: number;
  reserved: number;
  free: number;
  wallet_usdc: number;
  mint: "Circle SPL USDC";
};

export function decodeUserVault(data: Uint8Array): { available: number; reserved: number } | null {
  if (data.length < 8 + 32 + 8 + 8) return null;
  const view = new DataView(data.buffer, data.byteOffset, data.byteLength);
  return {
    available: Number(view.getBigUint64(8 + 32, true)),
    reserved: Number(view.getBigUint64(8 + 40, true)),
  };
}

/** Confirmed L1 ledger. Unconfirmed deposits MUST NOT appear (FR-WAL-03). */
export async function fetchUserVault(connection: Connection, owner: PublicKey): Promise<UserVaultSnap> {
  const empty: UserVaultSnap = {
    owner: owner.toBase58(),
    exists: false,
    available: 0,
    reserved: 0,
    free: 0,
    wallet_usdc: 0,
    mint: "Circle SPL USDC",
  };
  const [vaultAcc, wallet] = await Promise.all([
    connection.getAccountInfo(userVault(owner), "confirmed"),
    connection.getTokenAccountBalance(ata(owner), "confirmed").catch(() => null),
  ]);
  const wallet_usdc = wallet ? Number(wallet.value.amount) : 0;
  if (!vaultAcc) return { ...empty, wallet_usdc };
  const dec = decodeUserVault(vaultAcc.data);
  if (!dec) return { ...empty, wallet_usdc };
  const free = Math.max(0, dec.available - dec.reserved);
  return {
    owner: owner.toBase58(),
    exists: true,
    available: dec.available,
    reserved: dec.reserved,
    free,
    wallet_usdc,
    mint: "Circle SPL USDC",
  };
}

export async function fetchVaultApi(api: string, owner: string): Promise<UserVaultSnap | null> {
  const r = await fetch(`${api}/v1/owners/${owner}/vault`);
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`vault ${r.status}`);
  return r.json();
}
