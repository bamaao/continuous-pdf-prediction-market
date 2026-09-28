"use client";

import { compose, familyName, fetchResolution, ResolutionSnap } from "@cpm/sdk";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { MARKET_API } from "@/lib/env";
import { NEED_WALLET, SESSION_BOARD_ONLY } from "@/lib/copy";
import { sendSigned } from "@/lib/tx";

const ZERO = "11111111111111111111111111111111";

function isErrNote(note: string): boolean {
  return /failed|error/i.test(note);
}

const STEPS = ["Open", "Propose", "Challenge", "Vote", "Final"] as const;

function defaultKind(family: number): number {
  if (family === 0) return 0;
  if (family === 3) return 2;
  if (family === 4) return 4;
  return 1;
}

function shortKey(k: string): string {
  if (!k || k === ZERO || k.length < 12) return k || "—";
  return `${k.slice(0, 4)}…${k.slice(-4)}`;
}

function remain(ts: number, now: number): { text: string; elapsed: boolean } {
  const s = Math.floor(ts - now);
  if (s <= 0) return { text: "elapsed", elapsed: true };
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const sec = s % 60;
  if (h > 48) return { text: `${Math.floor(h / 24)}d ${h % 24}h`, elapsed: false };
  if (h > 0) return { text: `${h}h ${m}m`, elapsed: false };
  if (m > 0) return { text: `${m}m ${sec}s`, elapsed: false };
  return { text: `${sec}s`, elapsed: false };
}

function evidenceKey(market: string): string {
  return `cpm.evidence.${market}`;
}

async function sha256Hex(text: string): Promise<string> {
  const buf = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return Array.from(new Uint8Array(buf), (b) => b.toString(16).padStart(2, "0")).join("");
}

function stepIndex(rec: ResolutionSnap | null): number {
  if (!rec) return 0;
  if (rec.phase === 0) return 1;
  if (rec.phase === 1) return 2;
  if (rec.phase === 2) return 3;
  return 4;
}

export function ResultForm({ market, family }: { market: string; family: number }) {
  const { connection } = useConnection();
  const { publicKey, signTransaction } = useWallet();
  const [rec, setRec] = useState<ResolutionSnap | null>(null);
  const [value, setValue] = useState(0);
  const [valueB, setValueB] = useState(0);
  const [yes, setYes] = useState(1);
  const [evidence, setEvidence] = useState("");
  const [hash, setHash] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(() => Date.now() / 1000);
  const kind = defaultKind(family);

  const load = useCallback(async () => {
    setRec(await fetchResolution(MARKET_API, market));
  }, [market]);

  useEffect(() => {
    load().catch(() => setRec(null));
    const t = window.setInterval(() => load().catch(() => undefined), 3000);
    return () => window.clearInterval(t);
  }, [load]);

  useEffect(() => {
    const raw = sessionStorage.getItem(evidenceKey(market)) ?? "";
    setEvidence(raw);
  }, [market]);

  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now() / 1000), 1000);
    return () => window.clearInterval(t);
  }, []);

  useEffect(() => {
    sessionStorage.setItem(evidenceKey(market), evidence);
    const trimmed = evidence.trim();
    if (!trimmed) {
      setHash("");
      return;
    }
    const hex = trimmed.replace(/^0x/i, "");
    if (/^[0-9a-fA-F]{64}$/.test(hex)) {
      setHash(hex.toLowerCase());
      return;
    }
    let stop = false;
    sha256Hex(trimmed).then((h) => {
      if (!stop) setHash(h);
    });
    return () => {
      stop = true;
    };
  }, [evidence, market]);

  const me = publicKey?.toBase58() ?? "";
  const member = !!me && (rec?.members ?? []).includes(me);
  const reporter = !!me && !!rec && rec.authorized_reporter !== ZERO && rec.authorized_reporter === me;
  const canPropose = member || reporter;
  const role = !me ? "connect a wallet" : member ? "committee member (this window)" : reporter ? "authorized reporter · propose only" : "observer";

  const clock = useMemo(() => {
    if (!rec) return { label: "window", text: "not opened", elapsed: true };
    if (rec.phase === 0) return { label: "report window", ...remain(rec.report_deadline, now) };
    if (rec.phase === 1) return { label: "challenge window", ...remain(rec.challenge_end, now) };
    if (rec.phase === 2) return { label: "vote window", ...remain(rec.vote_end, now) };
    return { label: "window", text: "closed", elapsed: true };
  }, [rec, now]);

  const terminal = rec != null && rec.phase >= 3;
  const canChallenge = !!rec && rec.phase === 1 && member && me !== rec.proposer && !clock.elapsed;
  const canVote = !!rec && rec.phase === 2 && member;
  const canFinalize =
    !!rec &&
    ((rec.phase === 0 && now >= rec.report_deadline) ||
      (rec.phase === 1 && now >= rec.challenge_end) ||
      (rec.phase === 2 && (rec.votes_proposal >= rec.m || rec.votes_challenge >= rec.m || now >= rec.vote_end)));
  const canVoid = !!rec && !terminal && member && now >= rec.close_ts;

  async function run(
    op: "resolve_open" | "submit_result" | "challenge" | "vote" | "finalize" | "void_resolution",
    extra: Record<string, unknown> = {},
  ) {
    if (!publicKey || !signTransaction) {
      setNote(NEED_WALLET);
      return;
    }
    setBusy(true);
    setNote("signing…");
    try {
      const ix = await compose(MARKET_API, {
        op,
        owner: publicKey.toBase58(),
        market,
        family,
        kind,
        milli: family === 1 || family === 2,
        value: family === 4 ? yes : family === 1 || family === 2 ? Math.round(value * 1000) : value,
        value_b: family === 0 ? valueB : 0,
        evidence_hex: hash || undefined,
        ...extra,
      });
      const sig = await sendSigned(connection, signTransaction, publicKey, [ix]);
      if (op === "void_resolution") {
        try {
          await sendSigned(connection, signTransaction, publicKey, [
            await compose(MARKET_API, { op: "begin_refund", owner: publicKey.toBase58(), market }),
          ]);
        } catch {
          /* board may already be BOARD_REFUND */
        }
      }
      setNote(`${op} ${sig}`);
      if (op === "resolve_open") {
        for (let i = 0; i < 24; i++) {
          await new Promise((r) => setTimeout(r, 500));
          const next = await fetchResolution(MARKET_API, market);
          if (next) {
            setRec(next);
            break;
          }
        }
      } else {
        for (let i = 0; i < 12; i++) {
          await load();
          if (i === 11) break;
          await new Promise((r) => setTimeout(r, 400));
        }
      }
    } catch (e) {
      setNote(e instanceof Error ? e.message : "failed");
    } finally {
      setBusy(false);
    }
  }

  const step = stepIndex(rec);
  const phaseTitle = rec ? rec.phase_name : "Not opened";
  const failed = rec?.phase === 4;
  const voided = rec?.phase === 5;

  return (
    <div className="space-y-4 font-mono text-xs">
      <ol className="grid grid-cols-5 gap-1 text-[10px] uppercase tracking-widest">
        {STEPS.map((label, i) => {
          const on = i === step;
          const done = i < step;
          return (
            <li
              key={label}
              className={`border px-1 py-2 text-center ${on ? "border-amber bg-amber/10 text-amber" : done ? "border-rule text-paper/70" : "border-rule text-paper/35"}`}
            >
              {i + 1} {label}
            </li>
          );
        })}
      </ol>

      <dl className="grid grid-cols-2 gap-3 border border-rule p-3 sm:grid-cols-4">
        <Stat k="Phase" v={failed ? "RESOLUTION_FAILED" : voided ? "VOID" : phaseTitle} warn={failed || voided} />
        <Stat k={clock.label} v={clock.text} warn={clock.elapsed && !terminal} />
        <Stat
          k="Tally"
          v={rec ? `keep ${rec.votes_proposal} / challenge ${rec.votes_challenge}` : "—"}
          hint={rec ? `need M=${rec.m} of N=${rec.n}` : "open window first"}
        />
        <Stat k="Your role" v={role} hint={SESSION_BOARD_ONLY} />
      </dl>

      {rec?.has_proposed && (
        <div className="grid gap-3 sm:grid-cols-2">
          <Ticket
            title="Proposed x*"
            label={rec.proposed.label}
            who={rec.proposer}
            evidence={rec.evidence_hash}
            family={family}
          />
          {rec.has_challenged && (
            <Ticket title="Challenge x*" label={rec.challenged.label} who={rec.challenger} family={family} />
          )}
          {rec.has_final && <Ticket title="Final x*" label={rec.final_outcome.label} family={family} />}
        </div>
      )}

      {rec && rec.phase === 2 && (
        <div>
          <p className="uppercase tracking-widest text-amber">Window snapshot M/N</p>
          <p className="mt-1 text-[10px] text-paper/45">Votes use the roster copied at resolve_open, not a later set_roster.</p>
          <TallyBar label="Keep proposed" votes={rec.votes_proposal} m={rec.m} n={rec.n} />
          <TallyBar label="Take challenge" votes={rec.votes_challenge} m={rec.m} n={rec.n} />
          {rec.extensions > 0 && <p className="mt-2 text-[10px] text-paper/45">Already extended once. Next miss is RESOLUTION_FAILED.</p>}
        </div>
      )}

      {rec?.refunds_due && (
        <p className="border border-rust/50 bg-rust/10 px-3 py-2 text-rust">
          RESOLUTION_FAILED / VOID — refunds due. Positions and unused LP D unlock on the prediction market.
        </p>
      )}

      {!terminal && (
        <aside className="border border-amber/40 bg-amber/5 p-4">
          <p className="uppercase tracking-widest text-amber">
            {!rec ? "Open report window" : rec.phase === 0 ? "Propose x*" : rec.phase === 1 ? "Challenge window" : "Vote"}
          </p>
          <p className="mt-2 text-[10px] leading-relaxed text-paper/50">
            {!rec && "A member or the creator opens the record. Nobody can write x* until this exists."}
            {rec?.phase === 0 &&
              (!me
                ? "Connect a wallet to propose. Evidence stays in this browser; only the SHA-256 hash is posted."
                : canPropose
                  ? "Reporter or member files the family-correct result. Evidence stays in this browser; only the SHA-256 hash is posted."
                  : "You can watch. Only a reporter or committee member may propose.")}
            {rec?.phase === 1 &&
              (canChallenge
                ? "File a different x*. Same family rules. If nobody challenges before the window ends, anyone may finalize the proposal."
                : clock.elapsed
                  ? "Challenge window elapsed. Finalize to lock the proposal."
                  : member && me === rec.proposer
                    ? "You proposed this result. Another member must challenge."
                    : "Members may challenge. Reporters propose only.")}
            {rec?.phase === 2 &&
              (canVote
                ? "One ballot per member. First side to reach M finalizes. Timeout extends once, then RESOLUTION_FAILED."
                : "Members vote. Observers wait for M or the window.")}
          </p>

          {((rec?.phase === 0 && (canPropose || !me)) || (rec?.phase === 1 && canChallenge)) && (
            <OutcomeFields
              family={family}
              value={value}
              valueB={valueB}
              yes={yes}
              onValue={setValue}
              onValueB={setValueB}
              onYes={setYes}
            />
          )}

          {rec?.phase === 0 && (canPropose || !me) && (
            <label className="mt-3 block text-[10px] uppercase text-paper/50">
              Evidence (off-chain note or URL)
              <textarea
                className="mt-1 h-16 w-full border border-rule bg-ink px-2 py-1 text-[11px]"
                value={evidence}
                onChange={(e) => setEvidence(e.target.value)}
                placeholder="source, URL, or paste a 64-char hex digest"
              />
              <span className="mt-1 block break-all normal-case tracking-normal text-paper/40">
                on-chain evidence_hash {hash || "empty (32 zero bytes)"}
              </span>
            </label>
          )}

          <div className="mt-4 flex flex-col gap-2">
            {!rec && (
              <button className="w-full bg-amber py-2 text-ink disabled:opacity-50" disabled={busy} onClick={() => run("resolve_open")}>
                {busy ? "Opening…" : "Open window"}
              </button>
            )}
            {rec?.phase === 0 && (canPropose || !me) && (
              <button className="w-full bg-amber py-2 text-ink disabled:opacity-50" disabled={busy} onClick={() => run("submit_result")}>
                {busy ? "Submitting…" : "Submit result"}
              </button>
            )}
            {canChallenge && (
              <button className="w-full bg-amber py-2 text-ink disabled:opacity-50" disabled={busy} onClick={() => run("challenge")}>
                {busy ? "Challenging…" : "Challenge"}
              </button>
            )}
            {canVote && (
              <div className="grid grid-cols-2 gap-2">
                <button className="border border-rule py-2 disabled:opacity-50" disabled={busy} onClick={() => run("vote", { for_challenge: false })}>
                  Vote keep
                </button>
                <button className="bg-amber py-2 text-ink disabled:opacity-50" disabled={busy} onClick={() => run("vote", { for_challenge: true })}>
                  Vote challenge
                </button>
              </div>
            )}
            {canFinalize && (
              <button className="w-full border border-amber py-2 text-amber disabled:opacity-50" disabled={busy} onClick={() => run("finalize")}>
                {rec?.phase === 0 && rec.extensions === 0
                  ? "Extend report window"
                  : rec?.phase === 0
                    ? "Mark RESOLUTION_FAILED"
                    : rec?.phase === 2 && rec.votes_proposal < rec.m && rec.votes_challenge < rec.m && rec.extensions === 0
                      ? "Extend once"
                      : rec?.phase === 2 && rec.votes_proposal < rec.m && rec.votes_challenge < rec.m
                        ? "Mark RESOLUTION_FAILED"
                        : "Finalize"}
              </button>
            )}
            {canVoid && (
              <button className="w-full border border-rule py-2 text-paper/60 disabled:opacity-50" disabled={busy} onClick={() => run("void_resolution")}>
                Void market
              </button>
            )}
          </div>
          {note && <p className={`mt-2 text-[10px] ${isErrNote(note) ? "text-rust" : "text-paper/60"}`}>{note}</p>}
        </aside>
      )}

      {terminal && (
        <p className="text-[10px] text-paper/45">
          {failed
            ? "RESOLUTION_FAILED. This prediction market is void. Claim refunds on the market page."
            : voided
              ? "Committee voided this prediction market after close."
              : `Settled at ${rec?.final_outcome.label ?? "x*"}. Claim on the market page.`}
        </p>
      )}
    </div>
  );
}

function OutcomeFields({
  family,
  value,
  valueB,
  yes,
  onValue,
  onValueB,
  onYes,
}: {
  family: number;
  value: number;
  valueB: number;
  yes: number;
  onValue: (n: number) => void;
  onValueB: (n: number) => void;
  onYes: (n: number) => void;
}) {
  return (
    <div className="mt-3">
      <p className="text-[10px] uppercase tracking-widest text-amber">
        {familyName(family)} · x*
      </p>
      {family === 0 && (
        <div className="mt-2 grid grid-cols-2 gap-3">
          <label className="block text-[10px] uppercase text-paper/50">
            Home
            <input className="mt-1 w-full border border-rule bg-ink px-2 py-1" type="number" min={0} value={value} onChange={(e) => onValue(Number(e.target.value))} />
          </label>
          <label className="block text-[10px] uppercase text-paper/50">
            Away
            <input className="mt-1 w-full border border-rule bg-ink px-2 py-1" type="number" min={0} value={valueB} onChange={(e) => onValueB(Number(e.target.value))} />
          </label>
        </div>
      )}
      {(family === 1 || family === 2) && (
        <label className="mt-2 block text-[10px] uppercase text-paper/50">
          Scalar x*
          <input className="mt-1 w-full border border-rule bg-ink px-2 py-1" type="number" value={value} onChange={(e) => onValue(Number(e.target.value))} />
        </label>
      )}
      {family === 3 && (
        <label className="mt-2 block text-[10px] uppercase text-paper/50">
          Winning atom
          <input className="mt-1 w-full border border-rule bg-ink px-2 py-1" type="number" min={0} value={value} onChange={(e) => onValue(Number(e.target.value))} />
        </label>
      )}
      {family === 4 && (
        <div className="mt-2 flex gap-4 text-[11px] uppercase">
          <label className="flex items-center gap-2">
            <input type="radio" checked={yes === 1} onChange={() => onYes(1)} /> YES
          </label>
          <label className="flex items-center gap-2">
            <input type="radio" checked={yes === 0} onChange={() => onYes(0)} /> NO
          </label>
        </div>
      )}
    </div>
  );
}

function Ticket({
  title,
  label,
  who,
  evidence,
  family,
}: {
  title: string;
  label: string;
  who?: string;
  evidence?: string;
  family: number;
}) {
  return (
    <div className="border border-rule p-3">
      <p className="uppercase tracking-widest text-paper/45">{title}</p>
      <p className="mt-1 text-lg text-amber">{label}</p>
      <p className="mt-1 text-[10px] text-paper/50">{familyName(family)}</p>
      {who && who !== ZERO && <p className="mt-1 text-[10px] text-paper/45">by {shortKey(who)}</p>}
      {evidence && evidence !== "0".repeat(64) && (
        <p className="mt-1 break-all text-[10px] text-paper/35">evidence {evidence.slice(0, 16)}…</p>
      )}
    </div>
  );
}

function TallyBar({ label, votes, m, n }: { label: string; votes: number; m: number; n: number }) {
  const pct = n <= 0 ? 0 : Math.min(100, Math.round((votes / n) * 100));
  const hit = votes >= m;
  return (
    <div className="mt-2">
      <div className="flex justify-between text-[10px] uppercase">
        <span>{label}</span>
        <span className={hit ? "text-amber" : "text-paper/50"}>
          {votes}/{n} · M={m}
        </span>
      </div>
      <div className="mt-1 h-1.5 overflow-hidden bg-paper/10">
        <div className={`h-full ${hit ? "bg-amber" : "bg-paper/40"}`} style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

function Stat({ k, v, hint, warn }: { k: string; v: string; hint?: string; warn?: boolean }) {
  return (
    <div className={warn ? "text-amber" : ""}>
      <dt className="text-[10px] uppercase tracking-widest text-paper/45">{k}</dt>
      <dd className="mt-1 text-sm">{v}</dd>
      {hint && <p className="mt-0.5 text-[10px] text-paper/35">{hint}</p>}
    </div>
  );
}
