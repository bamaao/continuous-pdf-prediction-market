"use client";

import { clearSessionMeta, compose, deleteSessionSecret, loadSessionMeta, saveSessionMeta, saveSessionSecret, SessionMeta } from "@cpm/sdk";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { WalletMultiButton } from "@solana/wallet-adapter-react-ui";
import { Keypair } from "@solana/web3.js";
import { useCallback, useEffect, useState } from "react";
import { bytesToB64 } from "@/lib/b64";
import { MARKET_API } from "@/lib/env";
import { sendSigned } from "@/lib/tx";

export function WalletBar() {
  const { connection } = useConnection();
  const { publicKey, signMessage, signTransaction, connected, disconnect } = useWallet();
  const [siws, setSiws] = useState<"off" | "on" | "bad">("off");
  const [busy, setBusy] = useState("");
  const [meta, setMeta] = useState<SessionMeta | null>(null);

  const refreshMe = useCallback(async () => {
    const r = await fetch("/api/me");
    const j = await r.json();
    setSiws(j.ok ? "on" : "off");
  }, []);

  useEffect(() => {
    refreshMe();
    setMeta(publicKey ? loadSessionMeta(publicKey.toBase58()) : null);
  }, [refreshMe, publicKey]);

  async function signIn() {
    if (!publicKey || !signMessage) {
      setBusy("wallet cannot sign messages");
      return;
    }
    const ch = await fetch("/api/siws/challenge", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ address: publicKey.toBase58(), uri: window.location.origin }),
    });
    const { message } = await ch.json();
    const sig = await signMessage(new TextEncoder().encode(message));
    const done = await fetch("/api/siws/verify", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        address: publicKey.toBase58(),
        message,
        signature: bytesToB64(sig),
      }),
    });
    if (!done.ok) {
      setSiws("bad");
      setBusy("SIWS failed");
      return;
    }
    setSiws("on");
    setBusy("");
  }

  async function openSession() {
    if (!publicKey || !signTransaction) return;
    setBusy("opening session");
    const session = Keypair.generate();
    const expires = Math.floor(Date.now() / 1000) + 7200;
    const openBody = {
      op: "open_session",
      owner: publicKey.toBase58(),
      authority: session.publicKey.toBase58(),
      expires_ts: expires,
      remaining_usdc: 20_000,
    };
    try {
      await sendSigned(connection, signTransaction, publicKey, [await compose(MARKET_API, openBody)]);
    } catch {
      try {
        await sendSigned(connection, signTransaction, publicKey, [
          await compose(MARKET_API, { op: "revoke_session", owner: publicKey.toBase58() }),
        ]);
      } catch {
        /* no live session */
      }
      await sendSigned(connection, signTransaction, publicKey, [await compose(MARKET_API, openBody)]);
    }
    await saveSessionSecret(publicKey.toBase58(), session.secretKey);
    const next = { expires_ts: expires, remaining_usdc: 20_000 };
    saveSessionMeta(publicKey.toBase58(), next);
    setMeta(next);
    setBusy("session live");
  }

  async function renewSession() {
    if (!publicKey || !signTransaction) return;
    const expires = Math.floor(Date.now() / 1000) + 7200;
    await sendSigned(connection, signTransaction, publicKey, [
      await compose(MARKET_API, { op: "renew_session", owner: publicKey.toBase58(), expires_ts: expires, remaining_usdc: 20_000 }),
    ]);
    const next = { expires_ts: expires, remaining_usdc: 20_000 };
    saveSessionMeta(publicKey.toBase58(), next);
    setMeta(next);
    setBusy("session renewed");
  }

  async function endSession() {
    if (!publicKey || !signTransaction) return;
    setBusy("revoking session");
    const ix = await compose(MARKET_API, { op: "revoke_session", owner: publicKey.toBase58() });
    await sendSigned(connection, signTransaction, publicKey, [ix]);
    await deleteSessionSecret(publicKey.toBase58());
    clearSessionMeta(publicKey.toBase58());
    setMeta(null);
    setBusy("session revoked");
  }

  async function disconnectOnly() {
    await fetch("/api/siws/logout", { method: "POST" });
    await disconnect();
    setSiws("off");
    setBusy("disconnected — on-chain session still live until you revoke");
  }

  return (
    <div className="flex flex-wrap items-center gap-2">
      <WalletMultiButton />
      {connected && siws !== "on" && (
        <button className="border border-amber px-2 py-1 font-mono text-[11px] uppercase" onClick={signIn}>
          Sign in
        </button>
      )}
      {siws === "on" && (
        <>
          <button className="border border-rule px-2 py-1 font-mono text-[11px] uppercase" onClick={openSession}>
            Open session
          </button>
          <button className="border border-rule px-2 py-1 font-mono text-[11px] uppercase" onClick={renewSession}>
            Renew session
          </button>
          <button className="border border-rust px-2 py-1 font-mono text-[11px] uppercase text-rust" onClick={endSession}>
            End session
          </button>
          {meta && (
            <span className="font-mono text-[10px] text-paper/55">
              cap {meta.remaining_usdc} USDC · exp {new Date(meta.expires_ts * 1000).toLocaleTimeString()}
            </span>
          )}
          <button className="border border-rule px-2 py-1 font-mono text-[11px] uppercase" onClick={disconnectOnly}>
            Disconnect
          </button>
        </>
      )}
      {busy && <span className="max-w-[16rem] truncate font-mono text-[10px] text-paper/60">{busy}</span>}
    </div>
  );
}
