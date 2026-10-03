"use client";

import {
  compose,
  defaultDescription,
  defaultEvent,
  defaultTags,
  defaultTitle,
  familyName,
  fetchPrior,
  formatTags,
  parseTagsInput,
  fetchListingApplication,
  submitListingApplication,
  listCatalogTags,
  TAG_HINTS,
  listingIdHash,
  marketPda,
  type PriorSnap,
  DIRICHLET_ATOMS,
  DIRICHLET_SIMPLEX,
  DIRICHLET_TOP_N,
  MAX_N,
  dirichletCellCount,
  dirichletGrowHint,
  dirichletNeedsGrow,
} from "@cpm/sdk";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import { MARKET_API } from "@/lib/env";
import { NEED_WALLET, NEED_WALLET_AND_MARKET, SESSION_BOARD_ONLY } from "@/lib/copy";
import { sendSigned } from "@/lib/tx";

const FAMILIES = [0, 1, 2, 3, 4] as const;

const FAMILY_BLURB: Record<number, { x: string; hint: string }> = {
  0: { x: "score pair H–A, overflow 10+", hint: "Football. 11×11 Skellam grid. Kickoff does not stop trading." },
  1: { x: "scalar print", hint: "CPI / macro. x* is the first official print. Units are percentage points." },
  2: { x: "positive scalar", hint: "Daily price. Lock price_rule off-chain; x* is the defined print. Ω > 0." },
  3: { x: "winner / top-n / vote share", hint: "Election. One family, three layouts: atoms, top-n combinations, vote-share simplex." },
  4: { x: "YES or NO", hint: "Binary. Early YES only when the defined event has occurred." },
};

const OPS = ["create_skellam", "create_gaussian", "create_lognormal", "create_dirichlet", "create_bernoulli"] as const;

type GaussPreset = {
  id: string;
  label: string;
  topic: string;
  tag: string;
  xMin: number;
  xMax: number;
  mu: number;
  sigma: number;
  n: number;
  hint: string;
};

const CPI_PRESETS: GaussPreset[] = [
  {
    id: "us_cpi_yoy",
    label: "US CPI YoY",
    topic: "US_CPI_YOY",
    tag: "2026-03",
    xMin: -2,
    xMax: 12,
    mu: 2.4,
    sigma: 0.35,
    n: 256,
    hint: "Survey median → μ, survey dispersion → σ. FIRST_PRINT. Ω [−2%, 12%]. n_grid 256 (create then grow_grid past the 10 240-byte account cap).",
  },
  {
    id: "us_cpi_mom",
    label: "US CPI MoM",
    topic: "US_CPI_MOM",
    tag: "2026-03",
    xMin: -1,
    xMax: 2,
    mu: 0.2,
    sigma: 0.15,
    n: 256,
    hint: "Month-over-month print. Tighter Ω. FIRST_PRINT only — revisions do not settle.",
  },
  {
    id: "wide",
    label: "Wide / near-uniform",
    topic: "cpi",
    tag: "print",
    xMin: -2,
    xMax: 12,
    mu: 5,
    sigma: 6,
    n: 256,
    hint: "Large σ vs Ω. Truncated P0 approaches uniform when there is no survey.",
  },
];

function toMilli(v: number): number {
  return Math.round(v * 1000);
}

function unixFromLocal(v: string): number {
  const t = Date.parse(v);
  return Number.isFinite(t) ? Math.floor(t / 1000) : 0;
}

function localFromUnix(ts: number): string {
  const d = new Date(ts * 1000);
  if (Number.isNaN(d.getTime())) return "";
  const p = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}T${p(d.getHours())}:${p(d.getMinutes())}`;
}

function pct(bps: number): string {
  return `${(bps / 100).toFixed(1)}%`;
}

function isErrNote(note: string): boolean {
  return /failed|error/i.test(note);
}

export function CreateDesk() {
  const { connection } = useConnection();
  const { publicKey, signTransaction } = useWallet();
  const [family, setFamily] = useState(1);
  const [title, setTitle] = useState("US CPI YoY — 2026-03");
  const [tagsText, setTagsText] = useState("macro");
  const [description, setDescription] = useState(defaultDescription(1));
  const [event, setEvent] = useState(defaultEvent(1));
  const [topic, setTopic] = useState("US_CPI_YOY");
  const [tag, setTag] = useState("2026-03");
  const [n, setN] = useState(256);
  const [beta, setBeta] = useState(100);
  const [closeIn, setCloseIn] = useState(86400);
  const [closeTs, setCloseTs] = useState(() => Math.floor(Date.now() / 1000) + 86400);
  const [lambdaH, setLambdaH] = useState(1.4);
  const [lambdaA, setLambdaA] = useState(1.1);
  /** Dixon–Coles ρ; 0 = independent Poisson (default). */
  const [dcRho, setDcRho] = useState(0);
  const [xMin, setXMin] = useState(-2);
  const [xMax, setXMax] = useState(12);
  const [mu, setMu] = useState(2.4);
  const [sigma, setSigma] = useState(0.35);
  const [priceRule, setPriceRule] = useState("Coinbase last at observe_ts");
  const [early, setEarly] = useState(false);
  const [layout, setLayout] = useState(DIRICHLET_ATOMS);
  const [kCand, setKCand] = useState(4);
  const [bins, setBins] = useState(10);
  const [topN, setTopN] = useState(1);
  const [poolAmt, setPoolAmt] = useState(0);
  const [tapCap, setTapCap] = useState(0);
  const [feeBps, setFeeBps] = useState(0);
  const [feeTiming, setFeeTiming] = useState(0);
  const [market, setMarket] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [listed, setListed] = useState(false);
  const [blockedText, setBlockedText] = useState("");
  const [appId, setAppId] = useState(0);
  const [appStatus, setAppStatus] = useState("");
  const [preset, setPreset] = useState("us_cpi_yoy");
  const [prior, setPrior] = useState<PriorSnap | null>(null);
  const [catalogTags, setCatalogTags] = useState<string[]>([...TAG_HINTS]);

  const op = OPS[family];
  const dirichletN =
    family === 3
      ? dirichletCellCount({
          layout,
          nAtoms: n,
          k: kCand,
          topN,
          bins,
        })
      : null;
  const cells = family === 0 ? 121 : family === 4 ? 2 : family === 3 ? (dirichletN ?? 0) : n;
  const derived = useMemo(() => {
    try {
      const diLayout = family === 3 ? layout : 0;
      const diTop = family === 3 && layout === DIRICHLET_TOP_N ? topN : 0;
      const diBins = family === 3 && layout === DIRICHLET_SIMPLEX ? bins : 0;
      return marketPda(
        listingIdHash({
          family,
          topic,
          tag,
          layout: diLayout,
          topN: diTop,
          bins: diBins,
        }),
      ).toBase58();
    } catch {
      return "";
    }
  }, [family, topic, tag, layout, topN, bins]);
  const blurb = FAMILY_BLURB[family];
  const usesMilli = family === 0 || family === 1 || family === 2;
  const p0Label =
    family === 1
      ? `N(${mu}, ${sigma}) on [${xMin}, ${xMax}]`
      : family === 2
        ? `logN(μ=${mu}, σ=${sigma}) on [${xMin}, ${xMax}]`
        : family === 0
          ? `Poisson λ_H=${lambdaH}, λ_A=${lambdaA}${dcRho !== 0 ? ` · DC ρ=${dcRho}` : ""}`
          : family === 3
            ? layout === DIRICHLET_SIMPLEX
              ? `vote-share simplex k=${kCand} bins=${bins} → ${cells} cells`
              : layout === DIRICHLET_TOP_N
                ? `top-${topN} · k=${kCand} → C=${cells}`
                : `Dirichlet α=1 × ${n} winners`
            : "Bernoulli 50 / 50";

  useEffect(() => {
    listCatalogTags(MARKET_API)
      .then((rows) => {
        if (rows.length) setCatalogTags(rows.map((t) => t.name));
      })
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    let alive = true;
    const t = window.setTimeout(() => {
      if (family === 3 && (!cells || cells < 2)) {
        if (alive) setPrior(null);
        return;
      }
      fetchPrior(MARKET_API, {
        family,
        n: family === 0 ? 121 : family === 3 ? cells : n,
        milli: usesMilli,
        x_min: usesMilli ? toMilli(xMin) : undefined,
        x_max: usesMilli ? toMilli(xMax) : undefined,
        mu: usesMilli ? toMilli(mu) : undefined,
        sigma: usesMilli ? toMilli(sigma) : undefined,
        lambda_home: usesMilli ? toMilli(lambdaH) : undefined,
        lambda_away: usesMilli ? toMilli(lambdaA) : undefined,
        ...(family === 3
          ? {
              layout,
              k: layout === DIRICHLET_ATOMS ? n : kCand,
              bins: layout === DIRICHLET_SIMPLEX ? bins : undefined,
              top_n: layout === DIRICHLET_TOP_N ? topN : undefined,
            }
          : {}),
      })
        .then((p) => {
          if (alive) setPrior(p);
        })
        .catch(() => {
          if (alive) setPrior(null);
        });
    }, 200);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
  }, [family, n, cells, layout, kCand, bins, topN, xMin, xMax, mu, sigma, lambdaH, lambdaA, usesMilli]);

  function applyCpi(p: GaussPreset) {
    setPrior(null);
    setPreset(p.id);
    setTitle(`${p.label} — ${p.tag}`);
    setTagsText("macro, cpi");
    setEvent(`${p.label} first print — ${p.tag}`);
    setDescription(p.hint);
    setTopic(p.topic);
    setTag(p.tag);
    setXMin(p.xMin);
    setXMax(p.xMax);
    setMu(p.mu);
    setSigma(p.sigma);
    setN(p.n);
    setListed(false);
  }

  function pickFamily(f: number) {
    setPrior(null);
    setFamily(f);
    setListed(false);
    if (f === 1) {
      applyCpi(CPI_PRESETS[0]);
      return;
    }
    setPreset("");
    setTagsText(defaultTags(f).join(", "));
    setTitle(defaultTitle(f));
    setDescription(defaultDescription(f));
    setEvent(defaultEvent(f));
    if (f === 2) {
      setTopic("BTC-USD");
      setTag("spot");
      setXMin(10_000);
      setXMax(250_000);
      setMu(11.08);
      setSigma(0.25);
      setN(256);
    } else if (f === 0) {
      setTopic("match");
      setTag("ft");
      setLambdaH(1.4);
      setLambdaA(1.1);
    } else if (f === 3) {
      setTopic("election");
      setTag("winner");
      setLayout(DIRICHLET_ATOMS);
      setN(4);
      setKCand(4);
      setBins(10);
      setTopN(1);
    } else {
      setTopic("event");
      setTag("yes");
    }
  }

  function listingFields() {
    return {
      title: title.trim(),
      tags: parseTagsInput(tagsText),
      event: event.trim(),
      description: description.trim(),
      blocked_regions: parseTagsInput(blockedText).map((r) => r.toUpperCase()),
    };
  }

  function composeSpec() {
    return {
      op,
      family,
      topic,
      tag,
      n: cells,
      beta,
      close_in: closeIn,
      close_ts: closeTs,
      risk_lock_ts: closeTs,
      fee_bps: feeBps,
      fee_timing: feeTiming,
      challenge_secs: closeIn <= 120 ? 8 : 3600,
      report_window_secs: closeIn <= 120 ? 90 : 400,
      early_resolve: early,
      tap_cap: tapCap,
      ...listingFields(),
      ...(family === 3
        ? {
            layout,
            k: layout === DIRICHLET_ATOMS ? n : kCand,
            bins: layout === DIRICHLET_SIMPLEX ? bins : 0,
            top_n: layout === DIRICHLET_TOP_N ? topN : 0,
          }
        : {}),
      ...(usesMilli
        ? {
            milli: true,
            lambda_home: toMilli(lambdaH),
            lambda_away: toMilli(lambdaA),
            ...(family === 0 && dcRho !== 0
              ? { prior_kind: 1, dc_rho: toMilli(dcRho) }
              : family === 0
                ? { prior_kind: 0, dc_rho: 0 }
                : {}),
            x_min: toMilli(xMin),
            x_max: toMilli(xMax),
            mu: toMilli(mu),
            sigma: toMilli(sigma),
          }
        : {}),
    };
  }

  async function requireSiws(): Promise<boolean> {
    try {
      const r = await fetch("/api/me");
      const v = (await r.json()) as { ok?: boolean };
      if (!v.ok) {
        setNote("SIWS first — connect and sign in, then submit for review");
        return false;
      }
      return true;
    } catch {
      setNote("SIWS first — connect and sign in, then submit for review");
      return false;
    }
  }

  async function applyForReview() {
    if (!publicKey) {
      setNote(NEED_WALLET);
      return;
    }
    if (!(await requireSiws())) return;
    if (!title.trim()) {
      setNote("market title is required");
      return;
    }
    const tags = parseTagsInput(tagsText);
    if (!tags.length) {
      setNote("add at least one catalog tag, e.g. football, epl");
      return;
    }
    if (!event.trim()) {
      setNote("trading event is required");
      return;
    }
    if (description.trim().length < 12) {
      setNote("write a description: match/print, settlement, and source — at least one sentence");
      return;
    }
    if (!topic.trim()) {
      setNote("topic is required on-chain identity");
      return;
    }
    if (beta <= 0) {
      setNote("β must be > 0");
      return;
    }
    if (closeTs <= Math.floor(Date.now() / 1000)) {
      setNote("close time must be in the future — lock the whistle / print moment, not a countdown after review");
      return;
    }
    if ((family === 1 || family === 2) && xMax <= xMin) {
      setNote("Ω requires x_max > x_min");
      return;
    }
    if (family === 2 && xMin <= 0) {
      setNote("lognormal Ω must be > 0");
      return;
    }
    if ((family === 1 || family === 2) && sigma <= 0) {
      setNote("σ must be > 0");
      return;
    }
    if (family === 0 && (lambdaH <= 0 || lambdaA <= 0)) {
      setNote("λ_H and λ_A must be > 0");
      return;
    }
    if (family === 3) {
      if (!dirichletN || dirichletN < 2 || dirichletN > MAX_N) {
        setNote("Dirichlet cell count must be 2…1024. For a simplex, reduce bins or k.");
        return;
      }
      const alphaK = layout === DIRICHLET_ATOMS ? n : kCand;
      if (alphaK > 4 && dirichletNeedsGrow(dirichletN)) {
        setNote("when the grid exceeds 10KB, k must be ≤4 (on-chain extra.a–d hold only 4 α)");
        return;
      }
      if (layout === DIRICHLET_TOP_N && (topN < 1 || topN >= kCand)) {
        setNote("top-n requires 1 ≤ n < candidate count");
        return;
      }
    }
    setBusy(true);
    setNote("submitting application…");
    try {
      const fields = listingFields();
      const row = await submitListingApplication(MARKET_API, {
        applicant: publicKey.toBase58(),
        family,
        ...fields,
        topic,
        tag,
        compose: composeSpec(),
      });
      setAppId(row.id);
      setAppStatus(row.status_name);
      setNote(`application #${row.id} · ${row.status_name} · after review, the reviewer opens the prediction market`);
    } catch (e) {
      setNote(e instanceof Error ? e.message : "application failed");
    } finally {
      setBusy(false);
    }
  }

  async function fundPool() {
    if (!publicKey || !signTransaction) {
      setNote(NEED_WALLET);
      return;
    }
    setBusy(true);
    try {
      try {
        await sendSigned(connection, signTransaction, publicKey, [
          await compose(MARKET_API, { op: "init_pool", owner: publicKey.toBase58() }),
        ]);
      } catch {
        /* already open */
      }
      const sig = await sendSigned(connection, signTransaction, publicKey, [
        await compose(MARKET_API, { op: "fund_pool", owner: publicKey.toBase58(), amount: poolAmt }),
      ]);
      setNote(`fund_pool ${sig}`);
    } catch (e) {
      setNote(e instanceof Error ? e.message : "fund_pool failed");
    } finally {
      setBusy(false);
    }
  }

  async function setBoardTap() {
    if (!publicKey || !signTransaction || !market) {
      setNote(NEED_WALLET_AND_MARKET);
      return;
    }
    setBusy(true);
    try {
      const sig = await sendSigned(connection, signTransaction, publicKey, [
        await compose(MARKET_API, { op: "set_tap", owner: publicKey.toBase58(), market, amount: tapCap }),
      ]);
      setNote(`set_tap ${sig}`);
    } catch (e) {
      setNote(e instanceof Error ? e.message : "set_tap failed");
    } finally {
      setBusy(false);
    }
  }

  async function claimFees() {
    if (!publicKey || !signTransaction || !market) {
      setNote(NEED_WALLET_AND_MARKET);
      return;
    }
    setBusy(true);
    try {
      const sig = await sendSigned(connection, signTransaction, publicKey, [
        await compose(MARKET_API, { op: "claim_fees", owner: publicKey.toBase58(), market }),
      ]);
      setNote(`claim_fees ${sig}`);
    } catch (e) {
      setNote(e instanceof Error ? e.message : "claim_fees failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-amber">Application</p>
      <h1 className="font-display text-5xl">Create prediction market</h1>
      <p className="mt-2 max-w-2xl text-sm leading-relaxed text-paper/70">
        Submit a market application by distribution family. Name and tags are the public card — they are
        not the on-chain create instruction. After a reviewer approves, that reviewer wallet opens trading
        and the risk auction. You do not open it yourself.{" "}
        {SESSION_BOARD_ONLY} Overflow on Skellam is labeled 10+.
      </p>

      <ol className="mt-6 grid max-w-3xl grid-cols-4 gap-1 font-mono text-[10px] uppercase tracking-widest">
        {["Identity", "Prior", "Capital", listed ? "Live" : "Open"].map((label, i) => {
          const step = listed ? 3 : 0;
          const on = i === step || (!listed && i < 3);
          return (
            <li key={label} className={`border px-1 py-2 text-center ${on ? "border-amber bg-amber/10 text-amber" : "border-rule text-paper/40"}`}>
              {i + 1} {label}
            </li>
          );
        })}
      </ol>

      <section className="mt-8 grid gap-8 lg:grid-cols-[1.15fr_0.85fr]">
        <div>
          <p className="font-mono text-[11px] uppercase tracking-widest text-amber">Distribution family</p>
          <ul className="mt-3 grid gap-2 sm:grid-cols-2">
            {FAMILIES.map((f) => {
              const on = f === family;
              return (
                <li key={f}>
                  <button
                    type="button"
                    onClick={() => pickFamily(f)}
                    className={`w-full border px-3 py-3 text-left font-mono text-[11px] ${on ? "border-amber bg-amber/10" : "border-rule hover:border-paper/30"}`}
                  >
                    <span className={on ? "text-amber" : ""}>{familyName(f)}</span>
                    <span className="mt-1 block text-paper/50">{FAMILY_BLURB[f].hint}</span>
                  </button>
                </li>
              );
            })}
          </ul>

          <div className="mt-8 space-y-3 border border-amber/40 bg-amber/5 p-4 font-mono text-xs">
            <p className="uppercase tracking-widest text-amber">Identity ticket</p>
            <p className="text-[10px] leading-relaxed text-paper/50">
              Name, tags, trading event, and description are the public market card. Event and description tell
              traders the situation and the settle rule. A market can have several tags (football, epl). Topic /
              series key are the 32-byte on-chain identity — not the human name.
            </p>
            <label className="block text-[10px] uppercase text-paper/50">
              Market title
              <input
                className="mt-1 w-full border border-rule bg-ink px-2 py-1 text-[12px] normal-case tracking-normal"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                placeholder="US CPI YoY — March 2026"
                maxLength={80}
              />
            </label>
            <label className="block text-[10px] uppercase text-paper/50">
              Tags
              <input
                className="mt-1 w-full border border-rule bg-ink px-2 py-1 text-[12px] normal-case tracking-normal"
                value={tagsText}
                onChange={(e) => setTagsText(e.target.value)}
                placeholder="football, epl"
                maxLength={120}
              />
            </label>
            <p className="flex flex-wrap gap-1 text-[10px] normal-case text-paper/45">
              {catalogTags.map((hint) => (
                <button
                  key={hint}
                  type="button"
                  className="border border-rule px-1.5 py-0.5 hover:border-amber hover:text-amber"
                  onClick={() => {
                    const have = parseTagsInput(tagsText);
                    if (have.some((t) => t === hint)) return;
                    setTagsText([...have, hint].join(", "));
                  }}
                >
                  {hint}
                </button>
              ))}
              <Link href="/tags" className="border border-amber/40 px-1.5 py-0.5 text-amber">
                Maintain tags
              </Link>
            </p>
            <label className="block text-[10px] uppercase text-paper/50">
              Trading event
              <input
                className="mt-1 w-full border border-rule bg-ink px-2 py-1 text-[12px] normal-case tracking-normal"
                value={event}
                onChange={(e) => setEvent(e.target.value)}
                placeholder="Arsenal vs Chelsea — Premier League full-time score"
                maxLength={120}
              />
            </label>
            <p className="text-[10px] normal-case leading-relaxed text-paper/40">
              One line: what is being predicted. Traders see this under the market name.
            </p>
            <label className="block text-[10px] uppercase text-paper/50">
              Description
              <textarea
                className="mt-1 min-h-[7rem] w-full border border-rule bg-ink px-2 py-1 text-[12px] normal-case tracking-normal"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                rows={5}
                maxLength={800}
                placeholder={defaultDescription(family)}
              />
            </label>
            <p className="text-[10px] normal-case leading-relaxed text-paper/40">
              Required. Write the situation traders need: fixture or print, settle rule, source, and what does
              not count (extra time, revisions, later data). This is shown on the lobby and the market page.
            </p>
          </div>

          <div className="mt-8 space-y-3 border border-amber/40 bg-amber/5 p-4 font-mono text-xs">
            <p className="uppercase tracking-widest text-amber">Prior ticket</p>
            <p className="text-[10px] leading-relaxed text-paper/50">
              θ = 0 at apply, so market prices equal this P0. CPI / macro uses percentage points, not cell
              indices. Survey median can be μ; survey dispersion can be σ. Written once — trading never
              rewrites it.
            </p>
            {family === 1 && (
              <div className="flex flex-wrap gap-2">
                {CPI_PRESETS.map((p) => (
                  <button
                    key={p.id}
                    type="button"
                    onClick={() => applyCpi(p)}
                    className={`border px-2 py-1 text-[10px] uppercase ${preset === p.id ? "border-amber bg-amber/15 text-amber" : "border-rule"}`}
                  >
                    {p.label}
                  </button>
                ))}
              </div>
            )}
            {family === 1 && (
              <p className="text-[10px] text-paper/45">
                {CPI_PRESETS.find((p) => p.id === preset)?.hint ?? "Ω and N(μ,σ²) in percentage points."}
              </p>
            )}
            {family === 0 && (
              <div className="grid grid-cols-2 gap-2">
                <Num label="λ_H (home)" value={lambdaH} step={0.1} onChange={setLambdaH} />
                <Num label="λ_A (away)" value={lambdaA} step={0.1} onChange={setLambdaA} />
                <Num label="Dixon–Coles ρ (0 = off)" value={dcRho} step={0.01} onChange={setDcRho} />
                <p className="col-span-2 text-[10px] text-paper/40">
                  k_max = 10. Overflow cells show as 10+. Non-zero ρ writes Dixon–Coles prior (prior_kind=1).
                </p>
              </div>
            )}
            {(family === 1 || family === 2) && (
              <div className="grid grid-cols-2 gap-2">
                <Num label={family === 1 ? "x_min (pp)" : "x_min"} value={xMin} step={family === 1 ? 0.1 : 1000} onChange={setXMin} />
                <Num label={family === 1 ? "x_max (pp)" : "x_max"} value={xMax} step={family === 1 ? 0.1 : 1000} onChange={setXMax} />
                <Num label={family === 1 ? "μ (pp)" : "μ (log x)"} value={mu} step={0.05} onChange={setMu} />
                <Num label="σ" value={sigma} step={0.05} onChange={setSigma} />
              </div>
            )}
            {family === 2 && (
              <label className="block text-[10px] uppercase text-paper/50">
                price_rule (off-chain lock)
                <input className="mt-1 w-full border border-rule bg-ink px-2 py-1" value={priceRule} onChange={(e) => setPriceRule(e.target.value)} />
              </label>
            )}
            {family === 3 && (
              <div className="space-y-3">
                <p className="text-[10px] uppercase text-paper/50">Layout</p>
                <div className="flex flex-wrap gap-2">
                  {(
                    [
                      [DIRICHLET_ATOMS, "Winner"],
                      [DIRICHLET_TOP_N, "Top-n"],
                      [DIRICHLET_SIMPLEX, "Vote share"],
                    ] as const
                  ).map(([v, label]) => (
                    <button
                      key={v}
                      type="button"
                      className={`border px-2 py-1 text-[10px] uppercase ${layout === v ? "border-amber bg-amber/15 text-amber" : "border-rule"}`}
                      onClick={() => {
                        setLayout(v);
                        setListed(false);
                        if (v === DIRICHLET_SIMPLEX) {
                          setKCand(2);
                          setBins(31);
                        } else if (v === DIRICHLET_TOP_N) {
                          setKCand(4);
                          setTopN(1);
                        } else {
                          setN(4);
                        }
                      }}
                    >
                      {label}
                    </button>
                  ))}
                </div>
                {layout === DIRICHLET_ATOMS && (
                  <p className="text-[10px] normal-case leading-relaxed text-paper/40">
                    Exactly one winner. Cell count = candidate count. α_i = 1. Atoms over 10KB are rejected on-chain.
                  </p>
                )}
                {layout === DIRICHLET_TOP_N && (
                  <p className="text-[10px] normal-case leading-relaxed text-paper/40">
                    Cells are combinations “which n names make the list” C(k, n). Uniform prior. k max 16.
                  </p>
                )}
                {layout === DIRICHLET_SIMPLEX && (
                  <p className="text-[10px] normal-case leading-relaxed text-paper/40">
                    Vote share (s₁…s_k), Σ s_i = 1, s_i = c_i / bins. Cell count = C(bins+k−1, k−1). Over 10KB,
                    k must be ≤ 4. k max 16.
                  </p>
                )}
                {layout !== DIRICHLET_ATOMS && (
                  <div className="grid grid-cols-2 gap-2">
                    <Num
                      label="Candidates k"
                      value={kCand}
                      step={1}
                      onChange={(v) => {
                        const k = Math.max(2, Math.min(16, Math.round(v) || 2));
                        setKCand(k);
                        setTopN((cur) => Math.min(cur, k - 1));
                      }}
                    />
                    {layout === DIRICHLET_TOP_N && (
                      <Num
                        label="Top-n"
                        value={topN}
                        step={1}
                        onChange={(v) => setTopN(Math.max(1, Math.min(kCand - 1, Math.round(v) || 1)))}
                      />
                    )}
                    {layout === DIRICHLET_SIMPLEX && (
                      <Num label="bins (share partitions)" value={bins} step={1} onChange={(v) => setBins(Math.max(1, Math.round(v) || 1))} />
                    )}
                  </div>
                )}
              </div>
            )}
            {family !== 0 && family !== 4 && (family !== 3 || layout === DIRICHLET_ATOMS) && (
              <label className="block text-[10px] uppercase text-paper/50">
                n_grid / atoms
                <select className="mt-1 w-full border border-rule bg-ink px-2 py-1" value={n} onChange={(e) => setN(Number(e.target.value))}>
                  {(family === 3 ? [2, 3, 4, 6, 8] : [8, 32, 64, 128, 256, 512, 1024]).map((v) => (
                    <option key={v} value={v}>
                      {v}
                      {v >= 256 ? " (create + grow_grid)" : ""}
                    </option>
                  ))}
                </select>
              </label>
            )}
            {family === 4 && (
              <label className="flex items-center gap-2 text-[11px] uppercase">
                <input type="checkbox" checked={early} onChange={(e) => setEarly(e.target.checked)} /> early YES allowed
              </label>
            )}
            {family === 3 && (
              <p className="text-[10px] normal-case text-paper/45">
                Cells {cells || "—"}
                {cells >= 2 && dirichletNeedsGrow(cells) ? ` · ${dirichletGrowHint(cells)}` : ""}
                {cells > MAX_N ? " · exceeds MAX_N=1024" : ""}
              </p>
            )}
            {prior && <PriorPreview prior={prior} family={family} />}
          </div>

          <div className="mt-8 space-y-3 border border-rule p-4 font-mono text-xs">
            <p className="uppercase tracking-widest text-amber">Spec</p>
            <label className="block text-[10px] uppercase text-paper/50">
              Topic / series (on-chain id)
              <input className="mt-1 w-full border border-rule bg-ink px-2 py-1" value={topic} onChange={(e) => setTopic(e.target.value)} />
            </label>
            {family !== 0 && family !== 3 && (
              <label className="block text-[10px] uppercase text-paper/50">
                Tag / release
                <input className="mt-1 w-full border border-rule bg-ink px-2 py-1" value={tag} onChange={(e) => setTag(e.target.value)} />
              </label>
            )}
            <label className="block text-[10px] uppercase text-paper/50">
              β
              <input className="mt-1 w-full border border-rule bg-ink px-2 py-1" type="number" value={beta} onChange={(e) => setBeta(Number(e.target.value))} />
            </label>
            <label className="block text-[10px] uppercase text-paper/50">
              Close at (absolute)
              <input
                className="mt-1 w-full border border-rule bg-ink px-2 py-1"
                type="datetime-local"
                value={localFromUnix(closeTs)}
                onChange={(e) => {
                  const ts = unixFromLocal(e.target.value);
                  if (!ts) return;
                  setCloseTs(ts);
                  setCloseIn(Math.max(1, ts - Math.floor(Date.now() / 1000)));
                }}
              />
            </label>
            <p className="text-[10px] normal-case text-paper/40">
              Locked at submit. Football = full-time whistle. CPI = first print. Review does not move this clock.
              Chain Clock only knows &quot;now&quot; and rejects create if that moment is already past.
            </p>
            <label className="block text-[10px] uppercase text-paper/50">
              Or seconds from now
              <input
                className="mt-1 w-full border border-rule bg-ink px-2 py-1"
                type="number"
                value={closeIn}
                onChange={(e) => {
                  const secs = Number(e.target.value);
                  setCloseIn(secs);
                  setCloseTs(Math.floor(Date.now() / 1000) + Math.max(1, secs));
                }}
              />
            </label>
            <label className="block text-[10px] uppercase text-paper/50">
              Platform fee (bps)
              <input
                className="mt-1 w-full border border-rule bg-ink px-2 py-1"
                type="number"
                min={0}
                max={10000}
                value={feeBps}
                onChange={(e) => setFeeBps(Math.max(0, Math.min(10000, Number(e.target.value) || 0)))}
              />
            </label>
            <p className="text-[10px] text-paper/45">When the fee is taken. Locked at apply. Does not enter C_P.</p>
            <div className="flex flex-wrap gap-2">
              <button
                type="button"
                className={`border px-2 py-1 text-[10px] uppercase ${feeTiming === 0 ? "border-amber text-amber" : "border-rule text-paper/50"}`}
                onClick={() => setFeeTiming(0)}
              >
                At fill
              </button>
              <button
                type="button"
                className={`border px-2 py-1 text-[10px] uppercase ${feeTiming === 1 ? "border-amber text-amber" : "border-rule text-paper/50"}`}
                onClick={() => setFeeTiming(1)}
              >
                At claim
              </button>
            </div>
            <p className="text-[10px] text-paper/40">
              {feeTiming === 0
                ? "Trader pays C_S + φ·C_S when they buy. Platform can claim_fees anytime."
                : "Trader pays C_S only. φ is taken from the winner’s payout when they claim. Miss / VOID: 0."}
            </p>
            <p className="text-[10px] uppercase tracking-widest text-paper/50">
              Committee is protocol-wide. Create does not read or write the roster.
            </p>
            <p className="text-[11px] text-paper/70">
              <Link href="/committee" className="text-amber">
                Protocol committee
              </Link>
            </p>
          </div>
        </div>

        <aside className="border border-amber/40 bg-amber/5 p-4 font-mono text-xs">
          <p className="uppercase tracking-widest text-amber">Application ticket</p>
          <p className="mt-2 text-[10px] leading-relaxed text-paper/50">
            SIWS submit goes to the off-chain review queue. The reviewer approves, then that reviewer wallet
            opens trading and the risk auction. You do not sign create. Duplicate events are rejected by the system.
            C_P tap is a cap, not a reserved pot.
          </p>
          <label className="mt-3 block text-[10px] uppercase text-paper/50">
            Blocked regions (ISO, optional)
            <input
              className="mt-1 w-full border border-rule bg-ink px-2 py-1"
              value={blockedText}
              onChange={(e) => setBlockedText(e.target.value)}
              placeholder="CN, IR"
            />
          </label>
          <div className="mt-4 space-y-1.5 border-t border-rule/60 pt-3">
            <Row k="Name" v={title.trim() || "—"} />
            <Row k="Tags" v={formatTags(parseTagsInput(tagsText))} />
            <Row k="Event" v={event.trim() || "—"} />
            <Row k="Blocked regions" v={parseTagsInput(blockedText).join(" · ") || "none"} />
            <Row k="Review" v={appId ? `#${appId} · ${appStatus || "—"}` : "not submitted"} />
            <div>
              <p className="text-[10px] uppercase tracking-widest text-paper/45">Description</p>
              <p className="mt-1 whitespace-pre-wrap text-[12px] normal-case leading-relaxed text-paper/80">
                {description.trim() || "—"}
              </p>
            </div>
            <Row k="Family" v={familyName(family)} />
            <Row k="x*" v={blurb.x} />
            <Row k="P0" v={p0Label} />
            {(family === 1 || family === 2) && <Row k="Ω" v={`[${xMin}, ${xMax}]${family === 1 ? " pp" : ""}`} />}
            <Row k="Cells" v={String(cells || "—")} />
            {family === 3 && (
              <Row
                k="Layout"
                v={
                  layout === DIRICHLET_SIMPLEX
                    ? `simplex k=${kCand} bins=${bins}`
                    : layout === DIRICHLET_TOP_N
                      ? `top-${topN} k=${kCand}`
                      : `atoms ${n}`
                }
              />
            )}
            <Row k="β" v={String(beta)} />
            <Row k="C_P tap cap" v={`${tapCap} USDC`} />
            <Row k="Fee" v={`${feeBps} bps · ${feeTiming === 1 ? "at claim" : "at fill"}`} />
            <Row k="Closes at" v={new Date(closeTs * 1000).toISOString()} />
            <Row k="Market PDA" v={derived ? `${derived.slice(0, 4)}…${derived.slice(-4)}` : "—"} />
          </div>
          <label className="mt-4 block text-[10px] uppercase text-paper/50">
            C_P tap cap (MAY be 0)
            <input className="mt-1 w-full border border-rule bg-ink px-2 py-1" type="number" min={0} value={tapCap} onChange={(e) => setTapCap(Number(e.target.value))} />
          </label>
          {appStatus !== "open" && (
            <button className="mt-4 w-full bg-amber py-2 text-ink disabled:opacity-50" disabled={busy || appStatus === "pending_review" || appStatus === "approved"} onClick={applyForReview}>
              {busy ? "Submitting…" : "Submit for review"}
            </button>
          )}
          {appId > 0 && appStatus !== "open" && (
            <button
              className="mt-2 w-full border border-rule py-2 disabled:opacity-50"
              disabled={busy}
              onClick={async () => {
                setBusy(true);
                try {
                  const row = await fetchListingApplication(MARKET_API, appId);
                  setAppStatus(row.status_name);
                  if (row.market) {
                    setMarket(row.market);
                    setListed(true);
                    setNote(`application #${row.id} · ${row.status_name} · ${row.market}`);
                  } else {
                    setNote(
                      row.status_name === "approved"
                        ? `application #${row.id} approved — reviewer is opening the prediction market`
                        : `application #${row.id} · ${row.status_name} · awaiting review`,
                    );
                  }
                } catch (e) {
                  setNote(e instanceof Error ? e.message : "refresh failed");
                } finally {
                  setBusy(false);
                }
              }}
            >
              Refresh review status
            </button>
          )}
          {listed && market && (
            <div className="mt-4 space-y-2 border-t border-rule/60 pt-3">
              <p className="break-all text-[11px] text-amber">{market}</p>
              <div className="flex flex-wrap gap-3 text-[11px] uppercase">
                <Link href={`/m/${market}`} className="text-amber">
                  Open market
                </Link>
                <Link href={`/auction/${market}`}>Auction</Link>
                <Link href={`/committee?market=${market}`}>Committee</Link>
              </div>
            </div>
          )}
          {note && <p className={`mt-3 text-[10px] ${isErrNote(note) ? "text-rust" : "text-paper/60"}`}>{note}</p>}
        </aside>
      </section>

      <section className="mt-10 grid gap-8 lg:grid-cols-2">
        <div className="space-y-3 border border-rule p-4 font-mono text-xs">
          <p className="uppercase tracking-widest text-amber">Platform fees</p>
          <p className="text-[10px] text-paper/45">
            φ·C_S accrues on this prediction market’s fee ledger. It never enters C_P. The platform can claim
            into its vault at any time, then withdraw.
          </p>
          <input className="w-full border border-rule bg-ink px-2 py-1" placeholder="market pubkey" value={market} onChange={(e) => setMarket(e.target.value)} />
          <button className="w-full border border-rule py-2 disabled:opacity-50" disabled={busy} onClick={claimFees}>
            Claim fees
          </button>
        </div>
      </section>

      <section className="mt-8 grid gap-8 lg:grid-cols-2">
        <div className="space-y-3 border border-rule p-4 font-mono text-xs">
          <p className="uppercase tracking-widest text-amber">C_P pool</p>
          <p className="text-[10px] text-paper/45">
            One protocol pool. Per-market tap is a cap, not a reserved pot. Drawn only if L &gt; R_net.
            Fees do not fund this pool.
          </p>
          <input className="w-full border border-rule bg-ink px-2 py-1" type="number" value={poolAmt} onChange={(e) => setPoolAmt(Number(e.target.value))} />
          <button className="w-full border border-rule py-2 disabled:opacity-50" disabled={busy} onClick={fundPool}>
            Fund C_P
          </button>
          <input className="w-full border border-rule bg-ink px-2 py-1" placeholder="market pubkey for tap" value={market} onChange={(e) => setMarket(e.target.value)} />
          <input className="w-full border border-rule bg-ink px-2 py-1" type="number" value={tapCap} onChange={(e) => setTapCap(Number(e.target.value))} />
          <button className="w-full border border-rule py-2 disabled:opacity-50" disabled={busy} onClick={setBoardTap}>
            Set market tap cap
          </button>
        </div>
      </section>
    </div>
  );
}

function PriorPreview({ prior, family }: { prior: PriorSnap; family: number }) {
  const shown = downsample(prior.cells, 48);
  const max = Math.max(...shown.map((c) => c.p_bps), 1);
  return (
    <div className="border-t border-rule/60 pt-3">
      <p className="text-[10px] uppercase text-paper/50">
        Live P0 · peak {family === 0 ? `cell ${prior.peak}` : prior.peak_x.toFixed(2)} {prior.units}
      </p>
      <div className="mt-2 flex h-20 items-end gap-px">
        {shown.map((c) => (
          <div
            key={c.i}
            className="flex-1 bg-amber/70"
            style={{ height: `${Math.max(2, (c.p_bps / max) * 100)}%` }}
            title={`${c.x.toFixed(2)} · ${pct(c.p_bps)}`}
          />
        ))}
      </div>
      {prior.intervals.length > 0 && (
        <ul className="mt-3 space-y-1 text-[10px]">
          {prior.intervals.map((row) => (
            <li key={row.label} className="flex justify-between gap-3">
              <span className="text-paper/50">{row.label}</span>
              <span>{pct(row.p_bps)}</span>
            </li>
          ))}
        </ul>
      )}
      {prior.lines.length > 0 && (
        <ul className="mt-3 space-y-1 text-[10px]">
          {prior.lines.map((row) => (
            <li key={row.label} className="flex justify-between gap-3">
              <span className="text-paper/50">{row.label}</span>
              <span>{pct(row.p_bps)}</span>
            </li>
          ))}
        </ul>
      )}
      {prior.warnings.map((w) => (
        <p key={w} className="mt-2 text-[10px] text-rust">
          {w}
        </p>
      ))}
    </div>
  );
}

function downsample<T>(cells: T[], cap: number): T[] {
  if (cells.length <= cap) return cells;
  const step = cells.length / cap;
  return Array.from({ length: cap }, (_, i) => cells[Math.min(cells.length - 1, Math.floor(i * step))]);
}

function Num({
  label,
  value,
  step,
  onChange,
}: {
  label: string;
  value: number;
  step: number;
  onChange: (v: number) => void;
}) {
  return (
    <label className="text-[10px] uppercase text-paper/50">
      {label}
      <input
        className="mt-1 w-full border border-rule bg-ink px-2 py-1"
        type="number"
        step={step}
        value={value}
        onChange={(e) => onChange(Number(e.target.value))}
      />
    </label>
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
