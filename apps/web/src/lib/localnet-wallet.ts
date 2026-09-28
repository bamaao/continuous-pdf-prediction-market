"use client";

import { BaseSignerWalletAdapter, WalletName, WalletReadyState } from "@solana/wallet-adapter-base";
import { Keypair, PublicKey, Transaction, TransactionVersion, VersionedTransaction } from "@solana/web3.js";

export const LocalnetWalletName = "Localnet keypair" as WalletName<"Localnet keypair">;

declare global {
  interface Window {
    __cpmPendingKeypair?: number[];
  }
}

function readPending(): Keypair | null {
  if (typeof window === "undefined") return null;
  const raw = window.__cpmPendingKeypair;
  if (!raw?.length) return null;
  return Keypair.fromSecretKey(Uint8Array.from(raw));
}

function pickFile(): Promise<Keypair> {
  return new Promise((resolve, reject) => {
    const input = document.createElement("input");
    input.type = "file";
    input.accept = "application/json,.json";
    input.onchange = async () => {
      const f = input.files?.[0];
      if (!f) {
        reject(new Error("cancelled"));
        return;
      }
      try {
        const arr = JSON.parse(await f.text()) as number[];
        resolve(Keypair.fromSecretKey(Uint8Array.from(arr)));
      } catch (e) {
        reject(e);
      }
    };
    input.click();
  });
}

/** In-memory only. Never writes the secret to localStorage (FR-WAL-08). */
export class LocalnetWalletAdapter extends BaseSignerWalletAdapter {
  name = LocalnetWalletName;
  url = "http://127.0.0.1";
  icon =
    "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'><rect fill='%2312110e' width='32' height='32'/><text x='6' y='22' fill='%23d4a017' font-size='14'>L</text></svg>";
  supportedTransactionVersions: ReadonlySet<TransactionVersion> = new Set(["legacy"]);
  private _kp: Keypair | null = null;

  get publicKey() {
    return this._kp?.publicKey ?? null;
  }
  get connecting() {
    return false;
  }
  get readyState() {
    return WalletReadyState.Installed;
  }

  async connect(): Promise<void> {
    const pending = readPending();
    if (!pending) {
      this._kp = await pickFile();
      window.__cpmPendingKeypair = Array.from(this._kp.secretKey);
    } else {
      this._kp = pending;
    }
    this.emit("connect", this._kp.publicKey);
  }

  async disconnect(): Promise<void> {
    this._kp = null;
    this.emit("disconnect");
  }

  async signTransaction<T extends Transaction | VersionedTransaction>(tx: T): Promise<T> {
    if (!this._kp) throw new Error("not connected");
    if (tx instanceof Transaction) {
      tx.partialSign(this._kp);
      return tx;
    }
    tx.sign([this._kp]);
    return tx;
  }

  async signMessage(message: Uint8Array): Promise<Uint8Array> {
    if (!this._kp) throw new Error("not connected");
    const mod = (await import("tweetnacl")) as { default?: { sign: { detached: typeof import("tweetnacl").sign.detached } }; sign?: { detached: typeof import("tweetnacl").sign.detached } };
    const sign = mod.default?.sign ?? mod.sign;
    if (!sign) throw new Error("tweetnacl missing");
    return sign.detached(message, this._kp.secretKey);
  }
}
