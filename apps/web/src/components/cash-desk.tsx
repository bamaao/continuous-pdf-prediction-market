"use client";

import { compose, fetchUserVault, fetchVaultApi, loadSessionMeta, type UserVaultSnap } from "@cpm/sdk";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { useCallback, useEffect, useState } from "react";
import { MARKET_API } from "@/lib/env";
import { SESSION_BOARD_ONLY } from "@/lib/copy";
import { sendSigned } from "@/lib/tx";

type Mode = "deposit" | "withdraw";
type Phase = "idle" | "signing" | "pending" | "confirmed";

function isErrNote(note: string): boolean {
  return /failed|error|insufficient|rejected/i.test(note);
}

export function CashDesk() {
  const { connection } = useConnection();
  const { publicKey, signTransaction } = useWallet();
  const [mode, setMode] = useState<Mode>("deposit");
  const [amount, setAmount] = useState(100);
  const [note, setNote] = useState("");
  const [phase, setPhase] = useState<Phase>("idle");
  const [sig, setSig] = useState("");
  const [ledger, setLedger] = useState<UserVaultSnap | null>(null);
  const [sessionCap, setSessionCap] = useState<number | null>(null);

  const refresh = useCallback(async () => {
    if (!publicKey) {
      setLedger(null);
      setSessionCap(null);
      return;
    }
    setSessionCap(loadSessionMeta(publicKey.toBase58())?.remaining_usdc ?? null);
    const [l1, api] = await Promise.all([
      fetchUserVault(connection, publicKey),
      fetchVaultApi(MARKET_API, publicKey.toBase58()).catch(() => null),
    ]);
    setLedger(l1.exists || !api?.exists ? l1 : api);
  }, [connection, publicKey]);

  useEffect(() => {
    refresh().catch(() => setLedger(null));
  }, [refresh]);

  const wallet = ledger?.wallet_usdc ?? 0;
  const available = ledger?.available ?? 0;
  const reserved = ledger?.reserved ?? 0;
  const free = ledger?.free ?? 0;
  const max = mode === "deposit" ? wallet : free;
  const overWallet = mode === "deposit" && !!publicKey && amount > wallet;
  const overFree = mode === "withdraw" && !!publicKey && amount > free;
  const badAmt = !Number.isFinite(amount) || amount <= 0;
  const busy = phase === "signing" || phase === "pending";

  function pick(n: number) {
    setAmount(n);
    setPhase("idle");
    setNote("");
    setSig("");
  }

  async function go(next: Mode) {
    setMode(next);
    if (!publicKey || !signTransaction) {
      setNote("connect + SIWS first");
      return;
    }
    if (badAmt) {
      setNote("amount must be > 0");
      return;
    }
    if (next === "withdraw" && publicKey && amount > free) {
      setNote("cannot withdraw reserved margin — only free (available − reserved) can leave");
      return;
    }
    setPhase("signing");
    setNote("signing with the connected wallet…");
    try {
      if (next === "deposit") {
        try {
          await sendSigned(connection, signTransaction, publicKey, [
            await compose(MARKET_API, { op: "create_ata", owner: publicKey.toBase58() }),
          ]);
        } catch {
          /* ATA already exists */
        }
      }
      setPhase("pending");
      setNote(
        next === "deposit"
          ? "pending — spendable does not move until this deposit is confirmed"
          : "pending — free margin does not leave until withdraw is confirmed",
      );
      const ix = await compose(MARKET_API, {
        op: next,
        owner: publicKey.toBase58(),
        amount: Math.trunc(amount),
      });
      const sigOut = await sendSigned(connection, signTransaction, publicKey, [ix]);
      setSig(sigOut);
      await refresh();
      setPhase("confirmed");
      setNote(`${next} confirmed`);
    } catch (e) {
      setPhase("idle");
      setNote(e instanceof Error ? e.message : "failed");
    }
  }

  return (
    <aside className="border border-amber/40 bg-amber/5 p-4 font-mono text-xs h-fit">
      <p className="uppercase tracking-widest text-amber">Cash ticket</p>
      <p className="mt-2 text-[10px] leading-relaxed text-paper/50">
        Circle SPL USDC only. The protocol does not swap. SOL pays L1 fees, not the ticket. Unconfirmed
        deposits do not raise spendable balance. Session cannot deposit or withdraw. {SESSION_BOARD_ONLY}
      </p>

      <div className="mt-4 grid grid-cols-2 gap-1">
        {(["deposit", "withdraw"] as const).map((m) => (
          <button
            key={m}
            type="button"
            onClick={() => {
              setMode(m);
              setPhase("idle");
              setNote("");
              setSig("");
            }}
            className={`py-1.5 uppercase tracking-widest ${mode === m ? "bg-amber text-ink" : "border border-rule"}`}
          >
            {m === "deposit" ? "Deposit ticket" : "Withdraw ticket"}
          </button>
        ))}
      </div>

      <div className="mt-4 space-y-1.5 border-t border-rule/60 pt-3">
        <Row k="Wallet ATA" v={publicKey ? `${wallet} USDC` : "—"} />
        <Row k="Vault available" v={publicKey ? `${available} USDC` : "—"} />
        <Row k="Vault reserved" v={publicKey ? `${reserved} USDC` : "—"} />
        <Row k="Free to withdraw" v={publicKey ? `${free} USDC` : "—"} />
        <Row k="Session cap" v={sessionCap == null ? "no session" : `${sessionCap} USDC · not a balance`} />
        <Row k="Mint" v="Circle SPL USDC" />
      </div>

      <p className="mt-4 text-[10px] uppercase tracking-widest text-paper/50">
        {mode === "deposit" ? "Wallet ATA → vault available" : "Vault free → wallet ATA"}
      </p>
      <label className="mt-2 block text-[10px] uppercase tracking-widest text-paper/50">USDC amount</label>
      <input
        className="mt-1 w-full border border-rule bg-ink px-2 py-1"
        type="number"
        min={1}
        value={amount}
        onChange={(e) => {
          setAmount(Number(e.target.value));
          setPhase("idle");
        }}
      />
      <div className="mt-2 flex flex-wrap gap-1">
        {[10, 50, 100].map((n) => (
          <button key={n} type="button" className="border border-rule px-2 py-0.5 text-[10px]" onClick={() => pick(n)}>
            {n}
          </button>
        ))}
        <button type="button" className="border border-rule px-2 py-0.5 text-[10px]" onClick={() => pick(Math.max(0, max))}>
          Max {max}
        </button>
      </div>

      <div className="mt-4 space-y-1.5 border-t border-rule/60 pt-3">
        {mode === "deposit" ? (
          <>
            <Row k="You send" v={`${Math.max(0, Math.trunc(amount))} USDC`} />
            <Row k="After confirm" v={`available +${Math.max(0, Math.trunc(amount))}`} />
            <Row k="Until confirm" v="spendable unchanged" />
          </>
        ) : (
          <>
            <Row k="You take" v={`${Math.max(0, Math.trunc(amount))} USDC`} />
            <Row k="From" v="free = available − reserved" />
            <Row k="Reserved stays" v={`${reserved} USDC locked`} />
          </>
        )}
        <Row k="Signer" v="connected wallet · not Session" />
        <Row k="Phase" v={phase} />
      </div>

      {overWallet && (
        <p className="mt-3 text-[10px] text-amber">Wallet ATA is short of this deposit. Swap into Circle USDC outside the protocol, then retry.</p>
      )}
      {overFree && (
        <p className="mt-3 text-[10px] text-rust">Amount is above free margin. Reserved cannot be withdrawn.</p>
      )}
      {phase === "pending" && (
        <p className="mt-3 border border-amber/50 bg-amber/10 px-2 py-2 text-[10px] text-amber">
          pending — unconfirmed deposits do not raise spendable balance
        </p>
      )}

      <div className="mt-4 flex gap-2">
        <button
          className="flex-1 bg-amber py-2 text-ink disabled:opacity-50"
          disabled={busy}
          onClick={() => go("deposit")}
        >
          {busy && mode === "deposit" ? (phase === "signing" ? "Signing…" : "Pending…") : "Deposit"}
        </button>
        <button
          className="flex-1 border border-rule py-2 disabled:opacity-50"
          disabled={busy || (Boolean(publicKey) && amount > free)}
          onClick={() => go("withdraw")}
        >
          {busy && mode === "withdraw" ? (phase === "signing" ? "Signing…" : "Pending…") : "Withdraw"}
        </button>
      </div>
      {sig && <p className="mt-2 break-all text-[10px] text-paper/45">{sig}</p>}
      {note && <p className={`mt-3 text-[10px] ${isErrNote(note) ? "text-rust" : "text-paper/60"}`}>{note}</p>}
    </aside>
  );
}

function Row({ k, v }: { k: string; v: string }) {
  return (
    <div className="flex justify-between gap-3 py-1">
      <span className="text-paper/50">{k}</span>
      <span className="text-right">{v}</span>
    </div>
  );
}
