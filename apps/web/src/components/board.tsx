"use client";

import {
  formatTags,
  cellsFromPush,
  composeIxs,
  coverageLow,
  coveragePct,
  exactLine,
  familyName,
  fetchInfo,
  fetchListing,
  fetchOwnerPositions,
  formatRho,
  formatUnix,
  listComments,
  listingHeadline,
  postComment,
  shortPubkey,
  fetchQuote,
  fetchResolution,
  marketWsUrl,
  type PdfPush,
  hexMaskFromBits,
  lastTicketFor,
  loadSessionSecret,
  MarketInfo,
  Preview,
  pushInbox,
  rememberSkellam,
  rememberTicket,
  saveTicket,
  ResolutionSnap,
  SKELLAM_LINES,
  handicap1x2Mask,
  skellamMasks,
  statusName,
  type ListingI18n,
  type MarketComment,
} from "@cpm/sdk";
import { ListingI18nEditor } from "./listing-i18n-editor";
import { ListingLocale } from "./listing-locale";
import { MarketCard } from "./market-card";
import { PdfChart } from "./pdf-chart";
import { useConnection, useWallet } from "@solana/wallet-adapter-react";
import { Keypair, PublicKey, Transaction } from "@solana/web3.js";
import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import { MARKET_API } from "@/lib/env";
import { CLAIM_ON_PORTFOLIO, LOCK_RHO, LOCK_RHO_HINT, LMSR_COST_HINT, NEED_WALLET, OPEN_REFUNDS, SELLS_CLOSED } from "@/lib/copy";
import { sendSigned, submitGateway } from "@/lib/tx";

function nonceKey(market: string) {
  return `cpm.nonce.${market}`;
}

/** FR-UI-42: page+limit default 20, newest first. */
const COMMENT_LIMIT = 20;
/** FR-UI-39: HTTP fallback while the PDF socket is down. */
const PDF_POLL_MS = 30_000;
const WS_CONNECT_MS = 2_500;
const WS_RETRY_MIN_MS = 1_000;
const WS_RETRY_MAX_MS = 30_000;

export function Board({ market }: { market: string }) {
  const { connection } = useConnection();
  const { publicKey, signTransaction } = useWallet();
  const [pdf, setPdf] = useState<{ cell: number; p_bps: number; e: number }[]>([]);
  const [info, setInfo] = useState<MarketInfo | null>(null);
  const [family, setFamily] = useState(1);
  const [n, setN] = useState(8);
  const [sel, setSel] = useState<boolean[]>([]);
  const [line, setLine] = useState("custom");
  const [exactH, setExactH] = useState(1);
  const [exactA, setExactA] = useState(0);
  const [shares, setShares] = useState(1);
  const [quote, setQuote] = useState<Preview | null>(null);
  const [err, setErr] = useState("");
  const [nonce, setNonce] = useState(() => {
    if (typeof sessionStorage === "undefined") return 1;
    const saved = Number(sessionStorage.getItem(nonceKey(market)));
    return Number.isFinite(saved) && saved >= 1 && saved < 1_000_000 ? saved : 1;
  });
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [res, setRes] = useState<ResolutionSnap | null>(null);
  const [listingTitle, setListingTitle] = useState("");
  const [listingTitleEn, setListingTitleEn] = useState("");
  const [listingTags, setListingTags] = useState<string[]>([]);
  const [listingDescription, setListingDescription] = useState("");
  const [listingDescriptionEn, setListingDescriptionEn] = useState("");
  const [listingEvent, setListingEvent] = useState("");
  const [listingEventEn, setListingEventEn] = useState("");
  const [listingLocale, setListingLocale] = useState("en");
  const [listingIsTranslation, setListingIsTranslation] = useState(false);
  const [listingI18n, setListingI18n] = useState<ListingI18n>({});
  const [listingSourceLocale, setListingSourceLocale] = useState("en");
  const [listingTopic, setListingTopic] = useState("");
  const [listingTag, setListingTag] = useState("");
  const [pdfMode, setPdfMode] = useState<"idle" | "live" | "poll" | "frozen">("idle");
  const [pdfAt, setPdfAt] = useState(0);
  const [heldApi, setHeldApi] = useState<{ kind?: string; mask?: string; skellam_kind?: number; a?: number; b?: number } | null>(
    null,
  );
  const [comments, setComments] = useState<MarketComment[]>([]);
  const [commentPage, setCommentPage] = useState(1);
  const [commentPages, setCommentPages] = useState(1);
  const [commentTotal, setCommentTotal] = useState(0);
  const [commentBody, setCommentBody] = useState("");
  const [commentBusy, setCommentBusy] = useState(false);
  const [commentNote, setCommentNote] = useState("");
  const [nowSec, setNowSec] = useState(() => Math.floor(Date.now() / 1000));

  useEffect(() => {
    if (typeof sessionStorage !== "undefined") sessionStorage.setItem(nonceKey(market), String(nonce));
  }, [market, nonce]);

  useEffect(() => {
    const id = window.setInterval(() => setNowSec(Math.floor(Date.now() / 1000)), 1000);
    return () => window.clearInterval(id);
  }, []);

  useEffect(() => {
    let stop = false;
    (async () => {
      try {
        const desk = await fetchInfo(MARKET_API, market);
        if (stop) return;
        setInfo(desk);
        setListingTitle(desk.title ?? "");
        setListingTitleEn(desk.title_en ?? desk.title ?? "");
        setListingTags(desk.tags?.length ? desk.tags : desk.category ? [desk.category] : []);
        setListingDescription(desk.description ?? "");
        setListingDescriptionEn(desk.description_en ?? desk.description ?? "");
        setListingEvent(desk.event ?? "");
        setListingEventEn(desk.event_en ?? desk.event ?? "");
        setListingLocale(desk.locale ?? "en");
        setListingIsTranslation(!!desk.is_translation);
        setListingI18n(desk.i18n ?? {});
        setListingSourceLocale(desk.source_locale ?? "en");
        setListingTopic(desk.topic ?? "");
        setListingTag(desk.tag ?? "");
        setFamily(desk.family);
        setN(desk.n);
        setPdf(desk.cells);
        setPdfAt(Date.now());
        setPdfMode((desk.board_phase ?? 0) >= 1 ? "frozen" : "poll");
        setSel((prev) => (prev.length === desk.n ? prev : Array.from({ length: desk.n }, (_, i) => i === 0)));
        if ((desk.board_phase ?? 0) >= 1 && publicKey) {
          pushInbox({
            id: `settle-${market}`,
            title: desk.board_phase === 2 ? "Market VOID / refund ready" : "Market settled — review claim",
            market,
            market_title: listingHeadline({ title: desk.title, family: desk.family, market }),
            ts: Date.now(),
          });
        }
      } catch (e) {
        setErr(e instanceof Error ? e.message : "book");
      }
    })();
    fetchResolution(MARKET_API, market)
      .then((r) => {
        if (!stop) setRes(r);
      })
      .catch(() => undefined);
    fetchListing(MARKET_API, market, typeof navigator !== "undefined" ? navigator.languages?.join(",") || navigator.language : undefined)
      .then((row) => {
        if (stop || !row) return;
        if (row.title) setListingTitle(row.title);
        if (row.title_en) setListingTitleEn(row.title_en);
        if (row.tags?.length) setListingTags(row.tags);
        else if (row.category) setListingTags([row.category]);
        if (row.description) setListingDescription(row.description);
        if (row.description_en) setListingDescriptionEn(row.description_en);
        if (row.event) setListingEvent(row.event);
        if (row.event_en) setListingEventEn(row.event_en);
        if (row.locale) setListingLocale(row.locale);
        if (row.is_translation != null) setListingIsTranslation(!!row.is_translation);
        if (row.i18n) setListingI18n(row.i18n);
        if (row.source_locale) setListingSourceLocale(row.source_locale);
        if (row.topic) setListingTopic(row.topic);
        if (row.tag) setListingTag(row.tag);
      })
      .catch(() => undefined);
    return () => {
      stop = true;
    };
  }, [market, publicKey]);

  useEffect(() => {
    setCommentPage(1);
  }, [market]);

  useEffect(() => {
    let stop = false;
    listComments(MARKET_API, market, commentPage, COMMENT_LIMIT)
      .then((page) => {
        if (stop) return;
        setComments(page.items ?? []);
        setCommentPages(page.pages ?? 1);
        setCommentTotal(page.total ?? 0);
        if (page.page && page.page !== commentPage) setCommentPage(page.page);
      })
      .catch(() => {
        if (!stop) {
          setComments([]);
          setCommentPages(1);
          setCommentTotal(0);
        }
      });
    return () => {
      stop = true;
    };
  }, [market, commentPage]);

  useEffect(() => {
    if (!publicKey) {
      setHeldApi(null);
      return;
    }
    let stop = false;
    fetchOwnerPositions(MARKET_API, publicKey.toBase58(), { q: market, limit: 20 })
      .then((book) => {
        if (stop) return;
        const t = (book.items ?? []).find((row) => row.market === market);
        setHeldApi(
          t
            ? { kind: t.ticket_kind, mask: t.mask, skellam_kind: t.skellam_kind, a: t.a, b: t.b }
            : null,
        );
      })
      .catch(() => {
        if (!stop) setHeldApi(null);
      });
    return () => {
      stop = true;
    };
  }, [publicKey, market]);

  useEffect(() => {
    if (!info || (info.board_phase ?? 0) >= 1) {
      if (info && (info.board_phase ?? 0) >= 1) setPdfMode("frozen");
      return;
    }
    let stop = false;
    let poll = 0;
    let connectWait = 0;
    let retry = 0;
    let delay = WS_RETRY_MIN_MS;
    let gen = 0;
    let ws: WebSocket | null = null;
    const applyPush = (push: PdfPush) => {
      setPdf((prev) => cellsFromPush(push, prev));
      setPdfAt(Date.now());
      setInfo((cur) =>
        cur
          ? {
              ...cur,
              slot: push.slot,
              coverage_bps: push.coverage_bps ?? cur.coverage_bps,
              rho_hat_bps: push.rho_hat_bps ?? cur.rho_hat_bps,
              l_max_usdc: push.l_max_usdc ?? cur.l_max_usdc,
              c_max_usdc: push.c_max_usdc ?? cur.c_max_usdc,
              peak_risk: push.peak_risk ?? cur.peak_risk,
            }
          : cur,
      );
    };
    const pull = () => {
      fetchInfo(MARKET_API, market)
        .then((desk) => {
          if (stop) return;
          setInfo(desk);
          setPdf(desk.cells);
          setPdfAt(Date.now());
          if ((desk.board_phase ?? 0) >= 1) setPdfMode("frozen");
        })
        .catch(() => undefined);
    };
    const clearPoll = () => {
      if (poll) {
        window.clearInterval(poll);
        poll = 0;
      }
    };
    const ensurePoll = () => {
      if (stop || poll) return;
      pull();
      poll = window.setInterval(pull, PDF_POLL_MS);
    };
    const scheduleRetry = () => {
      if (stop || retry) return;
      const wait = delay;
      delay = Math.min(delay * 2, WS_RETRY_MAX_MS);
      retry = window.setTimeout(() => {
        retry = 0;
        attach();
      }, wait);
    };
    const attach = () => {
      if (stop) return;
      gen += 1;
      const my = gen;
      if (retry) {
        window.clearTimeout(retry);
        retry = 0;
      }
      if (connectWait) {
        window.clearTimeout(connectWait);
        connectWait = 0;
      }
      try {
        ws?.close();
      } catch {
        /* ignore */
      }
      let sock: WebSocket;
      try {
        sock = new WebSocket(marketWsUrl(MARKET_API, market));
      } catch {
        setPdfMode("poll");
        ensurePoll();
        scheduleRetry();
        return;
      }
      ws = sock;
      sock.onopen = () => {
        if (stop || my !== gen) return;
        delay = WS_RETRY_MIN_MS;
        clearPoll();
        setPdfMode("live");
      };
      sock.onmessage = (ev) => {
        if (stop || my !== gen) return;
        try {
          applyPush(JSON.parse(String(ev.data)) as PdfPush);
          setPdfMode("live");
        } catch {
          /* ignore */
        }
      };
      sock.onerror = () => {
        sock.close();
      };
      sock.onclose = () => {
        if (my !== gen) return;
        if (ws === sock) ws = null;
        if (stop) return;
        setPdfMode("poll");
        ensurePoll();
        scheduleRetry();
      };
      connectWait = window.setTimeout(() => {
        if (stop || my !== gen) return;
        if (sock.readyState === WebSocket.CONNECTING) sock.close();
      }, WS_CONNECT_MS);
    };
    attach();
    return () => {
      stop = true;
      gen += 1;
      try {
        ws?.close();
      } catch {
        /* ignore */
      }
      ws = null;
      clearPoll();
      if (connectWait) window.clearTimeout(connectWait);
      if (retry) window.clearTimeout(retry);
    };
  }, [market, info?.board_phase]);

  const isSkellam = family === 0 && pdf.length === 121;

  useEffect(() => {
    if (!isSkellam || line === "custom") return;
    const spec = line.startsWith("cs-") ? exactLine(exactH, exactA) : SKELLAM_LINES.find((l) => l.id === line);
    if (!spec) return;
    if (spec.kind === -2) {
      const side = spec.b > 0 ? "win" : spec.b === 0 ? "draw" : "lose";
      setSel(handicap1x2Mask(spec.a, side));
      return;
    }
    if (spec.kind < 0) return;
    const masks = skellamMasks(spec.kind, spec.a, spec.b);
    if (masks?.[0]) setSel(masks[0]);
  }, [isSkellam, line, exactH, exactA]);

  const mask = useMemo(() => hexMaskFromBits(sel.length ? sel : [true]), [sel]);

  useEffect(() => {
    if (!sel.some(Boolean)) return;
    const spec = !isSkellam || line === "custom" ? null : line.startsWith("cs-") ? exactLine(exactH, exactA) : SKELLAM_LINES.find((l) => l.id === line);
    const masks = spec && spec.kind >= 0 ? skellamMasks(spec.kind, spec.a, spec.b) : null;
    let stop = false;
    (async () => {
      try {
        if (masks && masks.length === 2) {
          const part = shares % 2 === 0 ? shares / 2 : (shares + 1) / 2;
          const [a, b] = await Promise.all([
            fetchQuote(MARKET_API, market, hexMaskFromBits(masks[0]), part),
            fetchQuote(MARKET_API, market, hexMaskFromBits(masks[1]), part),
          ]);
          if (stop) return;
          setQuote({
            ...a,
            c_s_usdc: a.c_s_usdc + b.c_s_usdc,
            pay_usdc: (a.pay_usdc ?? a.c_s_usdc) + (b.pay_usdc ?? b.c_s_usdc),
            fee_usdc: (a.fee_usdc ?? 0) + (b.fee_usdc ?? 0),
            payout_if_hit_usdc: (a.payout_if_hit_usdc ?? 0) + (b.payout_if_hit_usdc ?? 0),
            net_if_hit: (a.net_if_hit ?? 0) + (b.net_if_hit ?? 0),
            ev_if_p_s: (a.ev_if_p_s ?? 0) + (b.ev_if_p_s ?? 0),
          });
        } else {
          const q = await fetchQuote(MARKET_API, market, mask, shares);
          if (!stop) setQuote(q);
        }
      } catch (e) {
        if (!stop) setErr(String(e));
      }
    })();
    return () => {
      stop = true;
    };
  }, [market, mask, shares, sel, isSkellam, line, exactH, exactA, info?.slot]);

  function toggle(start: number, end?: number) {
    setLine("custom");
    const last = end ?? start;
    setSel((s) => {
      const on = s.slice(start, last + 1).some(Boolean);
      return s.map((v, j) => (j >= start && j <= last ? !on : v));
    });
  }

  const settled = (info?.board_phase ?? 0) >= 1;
  const needsRho = !settled && !!res?.has_final;
  const needsRefund = !settled && !!res && (res.phase === 4 || res.phase === 5 || res.refunds_due);

  async function lockSettle(op: "begin_settle" | "begin_refund") {
    if (!publicKey || !signTransaction) {
      setNote(NEED_WALLET);
      return;
    }
    setBusy(true);
    setNote(op === "begin_settle" ? "locking ρ…" : "opening refunds…");
    try {
      const ix = await compose(MARKET_API, {
        op,
        owner: publicKey.toBase58(),
        market,
      });
      const sig = await sendSigned(connection, signTransaction, publicKey, [ix]);
      setNote(`${op} ${sig}`);
      for (let i = 0; i < 16; i++) {
        const desk = await fetchInfo(MARKET_API, market);
        setInfo(desk);
        if ((desk.board_phase ?? 0) >= 1) {
          setPdfMode("frozen");
          pushInbox({
            id: `settle-${market}`,
            title: desk.board_phase === 2 ? "Market VOID / refund ready" : "Market settled — review claim",
            market,
            market_title: listingHeadline({ title: listingTitle, family: desk.family, market }),
            ts: Date.now(),
          });
          break;
        }
        await new Promise((r) => setTimeout(r, 500));
      }
    } catch (e) {
      setNote(e instanceof Error ? e.message : "settle gate failed");
    } finally {
      setBusy(false);
    }
  }
  const closeTs = info?.close_ts ?? 0;
  const tradingClosed = settled || (closeTs > 0 && nowSec >= closeTs);

  function typedLine() {
    if (!isSkellam || line === "custom") return null;
    return line.startsWith("cs-") ? exactLine(exactH, exactA) : SKELLAM_LINES.find((l) => l.id === line) ?? null;
  }

  async function trade(buy: boolean) {
    if (!buy) {
      setErr(SELLS_CLOSED);
      return;
    }
    if (tradingClosed) {
      setErr("trading closed — no buys after the deadline");
      return;
    }
    if (!publicKey || !signTransaction) {
      setErr("connect a wallet — wait for Localnet / SIWS if the page just reloaded");
      return;
    }
    if (!sel.some(Boolean)) {
      setErr("empty set");
      return;
    }
    const spec = typedLine();
    const typed = spec && spec.kind >= 0;
    let q = shares;
    if (typed && (spec.kind === 10 || spec.kind === 11) && q % 2 !== 0) {
      q += 1;
      setShares(q);
    }
    const op = typed ? (buy ? "buy_skellam_set" : "sell_skellam_set") : buy ? "buy_set" : "sell_set";
    setBusy(true);
    setNote("pending");
    try {
    const secret = await loadSessionSecret(publicKey.toBase58());
    const trader = secret ? Keypair.fromSecretKey(secret) : null;
    const ixs = await composeIxs(MARKET_API, {
      op,
      owner: publicKey.toBase58(),
      trader: (trader?.publicKey ?? publicKey).toBase58(),
      market,
      mask,
      shares: q,
      nonce,
      kind: typed ? spec.kind : undefined,
      value: typed ? spec.a : undefined,
      value_b: typed ? spec.b : undefined,
    });
      let lastSig = "";
      if (trader && ixs.length === 1) {
        const rec = await submitGateway(
          connection,
          async (tx) => {
            tx.partialSign(trader);
            return signTransaction(tx);
          },
          publicKey,
          publicKey,
          new PublicKey(market),
          nonce,
          ixs,
          typed
            ? { mask, kind: "skellam", skellam_kind: spec.kind, a: spec.a, b: spec.b }
            : { mask, kind: "mask" },
        );
        lastSig = rec.sig;
      } else {
        const signer = async (tx: Transaction) => {
          if (trader) tx.partialSign(trader);
          return signTransaction(tx);
        };
        for (const ix of ixs) {
          lastSig = await sendSigned(connection, signer, publicKey, [ix]);
        }
      }
      setNote(`confirmed ${lastSig}`);
      if (typed) {
        rememberSkellam(publicKey.toBase58(), market, spec.kind, spec.a, spec.b, q, mask);
        void saveTicket(MARKET_API, publicKey.toBase58(), {
          market,
          mask,
          kind: "skellam",
          skellam_kind: spec.kind,
          a: spec.a,
          b: spec.b,
        });
      } else {
        rememberTicket(publicKey.toBase58(), market, mask, q);
        void saveTicket(MARKET_API, publicKey.toBase58(), { market, mask, kind: "mask" });
      }
      setNonce((n) => n + 1);
      fetchInfo(MARKET_API, market)
        .then((desk) => {
          setInfo(desk);
          setPdf(desk.cells);
          return fetchResolution(MARKET_API, market);
        })
        .then((r) => setRes(r))
        .catch(() => undefined);
    } catch (e) {
      setErr(e instanceof Error ? e.message : `${op} failed`);
      setNote("pending — same nonce will retry");
    } finally {
      setBusy(false);
    }
  }

  const heldLocal = publicKey ? lastTicketFor(publicKey.toBase58(), market) : null;
  const held =
    heldApi?.mask || heldApi?.skellam_kind != null
      ? {
          kind: heldApi.kind === "skellam" ? "skellam" : "mask",
          mask: heldApi.mask,
          skellam_kind: heldApi.skellam_kind,
          a: heldApi.a,
          b: heldApi.b,
        }
      : heldLocal;

  return (
    <div>
      <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-amber">
        {listingTags.length || info?.tags?.length || info?.category ? formatTags(listingTags.length ? listingTags : info?.tags, info?.category) : "Prediction market"}
      </p>
      <ListingLocale
        title={listingTitle || info?.title}
        titleEn={listingTitleEn || info?.title_en || info?.title}
        event={listingEvent || info?.event}
        eventEn={listingEventEn || info?.event_en || info?.event}
        description={listingDescription || info?.description}
        descriptionEn={listingDescriptionEn || info?.description_en || info?.description}
        locale={listingLocale || info?.locale}
        isTranslation={listingIsTranslation || !!info?.is_translation}
      >
        {({ title, event, description }) => (
          <>
            <h1 className="mt-1 font-display text-4xl">
              {listingHeadline({ title, family: info?.family, market })}
            </h1>
            {event ? <p className="mt-2 max-w-3xl text-lg leading-snug text-paper/80">{event}</p> : null}
            {description ? (
              <p className="mt-2 max-w-3xl whitespace-pre-wrap text-sm leading-relaxed text-paper/70">{description}</p>
            ) : null}
          </>
        )}
      </ListingLocale>
      <p className="mt-1 break-all font-mono text-[11px] text-paper/40">{market}</p>
      <div className="mt-4 border border-rule p-4">
        <p className="font-mono text-[11px] uppercase tracking-widest text-amber">Market card</p>
        <MarketCard
          row={{
            market,
            title: listingTitle || info?.title,
            tags: listingTags.length ? listingTags : info?.tags,
            category: listingTags[0] || info?.category,
            description: listingDescription || info?.description,
            event: listingEvent || info?.event,
            family: info?.family ?? family,
            status: info?.status,
            close_ts: info?.close_ts,
            risk_lock_ts: info?.risk_lock_ts,
            report_open_ts: info?.report_open_ts,
            extensions: info?.extensions ?? res?.extensions,
            abnormal: info?.abnormal,
            final_result: info?.final_result || (res?.has_final ? res.final_outcome.label : ""),
            liability: info?.liability,
            l_max_usdc: info?.l_max_usdc,
            c_max_usdc: info?.c_max_usdc,
            payable_usdc: info?.payable_usdc,
            peak_risk: info?.peak_risk,
            resolution_phase: res?.phase,
            image_url: info?.image_url,
          }}
        />
      </div>
      <ListingI18nEditor
        market={market}
        meta={{
          titleEn: listingTitleEn || info?.title_en || listingTitle || info?.title || "",
          eventEn: listingEventEn || info?.event_en || listingEvent || info?.event || "",
          descriptionEn: listingDescriptionEn || info?.description_en || listingDescription || info?.description || "",
          tags: listingTags.length ? listingTags : info?.tags ?? [],
          topic: listingTopic || info?.topic,
          tag: listingTag || info?.tag,
          sourceLocale: listingSourceLocale || info?.source_locale,
          i18n: listingI18n,
        }}
        onSaved={(next) => {
          setListingI18n(next.i18n ?? {});
          setListingSourceLocale(next.source_locale ?? "en");
          setListingTitleEn(next.title_en ?? next.title);
          setListingEventEn(next.event_en ?? next.event ?? "");
          setListingDescriptionEn(next.description_en ?? next.description ?? "");
          const accept =
            typeof navigator !== "undefined" ? navigator.languages?.join(",") || navigator.language : undefined;
          void fetchListing(MARKET_API, market, accept).then((row) => {
            if (!row) return;
            if (row.title) setListingTitle(row.title);
            if (row.title_en) setListingTitleEn(row.title_en);
            if (row.event) setListingEvent(row.event);
            if (row.event_en) setListingEventEn(row.event_en);
            if (row.description) setListingDescription(row.description);
            if (row.description_en) setListingDescriptionEn(row.description_en);
            if (row.locale) setListingLocale(row.locale);
            if (row.is_translation != null) setListingIsTranslation(!!row.is_translation);
            if (row.i18n) setListingI18n(row.i18n);
          });
        }}
      />
      <p className="mt-3 max-w-2xl text-sm text-paper/65">
        The chart is the trading-implied PDF p_k = implied_probs(p0, θ, β). E is liability on that atom, not
        the distribution. Coverage / ρ̂ are displayed and not baked into the quote.
        {info ? ` ${familyName(info.family)} · ${statusName(info.status)}.` : ""}
      </p>
      <dl className="mt-6 grid grid-cols-2 gap-3 border border-rule p-4 font-mono text-xs sm:grid-cols-3 lg:grid-cols-6">
        <Stat k="Traders" v={info ? String(info.traders) : "—"} hint="distinct owners with q > 0" />
        <Stat k="Stake" v={info ? `${info.stake_usdc} USDC` : "—"} hint="sum of cost_paid" />
        <Stat
          k="Highest risk payout"
          v={info ? `${info.peak_risk?.payout_usdc ?? info.l_max_usdc} USDC` : "—"}
          hint={info?.peak_risk?.label ? `if ${info.peak_risk.label}` : "thickest overlap of bought intervals"}
        />
        <Stat k="C_R" v={info ? `${info.c_r} USDC` : "—"} hint="locked+filled risk capital" />
        <Stat k="C_max" v={info ? `${info.c_max_usdc} USDC` : "—"} hint="R_net + C_R + C_P^alloc" />
        <Stat
          k="C_P cap"
          v={info ? `${info.c_p_board ?? 0} USDC` : "—"}
          hint="optional tap; 0 is allowed and is not a warning"
        />
        <Stat
          k="Coverage"
          v={info ? coveragePct(info.l_max_usdc, info.coverage_bps) : "—"}
          hint="C_max / L_max — undefined while L_max is 0"
          warn={!!info && coverageLow(info.l_max_usdc, info.coverage_bps)}
        />
      </dl>
      <div className="mt-4 border border-rule p-4 font-mono text-[11px]">
        <p className="uppercase tracking-widest text-amber">Rules</p>
        <p className="mt-2 text-paper/70">
          Family {family} · n={n}
          {isSkellam ? " · overflow 10+" : ""} · live score is display-only and does not rewrite P0.
        </p>
        <p className="mt-1 text-paper/45">close_ts / risk_lock_ts / source_url lock at apply. Committee reports x*.</p>
        {info && (
          <p className="mt-1 text-paper/45">
            {familyName(info.family)} · {statusName(info.status)} · R_net {info.r_net}
            {res ? ` · resolution ${res.phase_name}` : ""}
          </p>
        )}
      </div>
      {quote && coverageLow(quote.l_max_usdc, quote.coverage_bps) && !settled && (
        <p className="mt-4 border border-amber/50 bg-amber/10 px-4 py-3 font-mono text-xs text-amber">
          Low coverage. The order is still allowed. This warning does not change p_S or C_S(q).
        </p>
      )}
      {needsRho && (
        <div className="mt-4 border border-amber/50 bg-amber/10 p-4 font-mono text-xs">
          <p className="uppercase tracking-widest text-amber">Settlement gate</p>
          <p className="mt-2 text-paper/70">{LOCK_RHO_HINT}</p>
          {res?.has_final && (
            <p className="mt-2 text-lg text-amber">x* {res.final_outcome.label}</p>
          )}
          <button
            className="mt-3 w-full bg-amber py-2 text-ink disabled:opacity-50"
            disabled={busy}
            onClick={() => lockSettle("begin_settle")}
          >
            {busy ? "Signing…" : LOCK_RHO}
          </button>
          {note && <p className="mt-2 text-[10px] text-paper/55">{note}</p>}
        </div>
      )}
      {needsRefund && !needsRho && (
        <div className="mt-4 border border-rule p-4 font-mono text-xs">
          <p className="uppercase tracking-widest text-amber">Refund gate</p>
          <p className="mt-2 text-paper/70">VOID / RESOLUTION_FAILED — open refunds before Portfolio can reclaim cost_paid.</p>
          <button
            className="mt-3 w-full border border-amber py-2 text-amber disabled:opacity-50"
            disabled={busy}
            onClick={() => lockSettle("begin_refund")}
          >
            {busy ? "Signing…" : OPEN_REFUNDS}
          </button>
          {note && <p className="mt-2 text-[10px] text-paper/55">{note}</p>}
        </div>
      )}
      {settled && (
        <div className="mt-4 border border-amber/40 bg-amber/5 p-4 font-mono text-xs">
          <p className="uppercase tracking-widest text-amber">Public result</p>
          <p className="mt-2">
            phase {info?.board_phase} · cell {info?.settle_cell} · L {info?.liability ?? "—"} USDC · ρ{" "}
            {formatRho(info?.board_phase, info?.rho_bps)}
            {info?.board_phase === 2 ? " (refund, not a written haircut)" : ""}
          </p>
          {res?.has_final && (
            <p className="mt-2 text-lg text-amber">
              x* {res.final_outcome.label}
            </p>
          )}
          {res?.has_proposed && !res.has_final && (
            <p className="mt-2">proposed {res.proposed.label} · {res.phase_name}</p>
          )}
          <p className="mt-1 text-paper/50">
            {info?.board_phase === 2 || res?.phase === 4 || res?.phase === 5
              ? "VOID / RESOLUTION_FAILED — refund cost_paid, not a fake 0-0."
              : "Winners receive ⌊ρ·face⌋. Same ρ for every ticket."}
          </p>
          <p className="mt-2 text-paper/45">
            C_max {info?.c_max_usdc ?? "—"} = R_net + C_R + C_P^alloc. {CLAIM_ON_PORTFOLIO} with the connected wallet.
          </p>
          {held && (
            <p className="mt-2 text-paper/70">
              Your last S on this market: {held.kind === "skellam" ? `skellam ${held.skellam_kind}/${held.a}/${held.b}` : `mask ${held.mask}`}
            </p>
          )}
          <Link href="/portfolio" className="mt-3 inline-block uppercase text-amber">
            {CLAIM_ON_PORTFOLIO}
          </Link>
        </div>
      )}
      <div className="mt-4 flex flex-wrap gap-4 font-mono text-[11px] uppercase">
        <Link href={`/auction/${market}`} className="text-amber">
          Auction
        </Link>
        <Link href={`/resolve/${market}`}>Resolve</Link>
        <Link href={`/committee?market=${market}`}>Committee</Link>
        <Link href="/create">Create</Link>
        <Link href="/portfolio">Portfolio</Link>
      </div>
      {err && <p className="mt-4 font-mono text-sm text-rust">{err}</p>}

      <section className="mt-8 grid gap-8 lg:grid-cols-[1.4fr_0.8fr]">
        <div className="border border-rule p-4">
          <p className="mb-3 font-mono text-[11px] uppercase tracking-widest text-amber">
            Implied PDF from trading
          </p>
          <p className="mb-2 font-mono text-[10px] text-paper/55">
            {pdfMode === "frozen" ? "Frozen PDF" : pdfMode === "live" ? "Live book" : pdfMode === "poll" ? "Polling book" : "Indexed snapshot"}
            {" · "}
            snapshot slot {info?.slot ?? "—"}
            {pdfAt ? ` · as of ${new Date(pdfAt).toLocaleTimeString()}` : ""}
            {" · "}
            not computed in this browser
          </p>
          <p className="mb-4 text-[11px] text-paper/50">
            Height / heat is p_k from implied_probs(p0, θ, β) on the indexed book. Hover shows percent and face E.
            The browser does not run LMSR. After close the chart freezes.
          </p>
          {isSkellam && (
            <div className="mb-4 flex flex-wrap gap-2 font-mono text-[10px] uppercase">
              {SKELLAM_LINES.map((l) => (
                <button
                  key={l.id}
                  className={`border px-2 py-1 ${line === l.id ? "border-amber text-amber" : "border-rule"}`}
                  onClick={() => setLine(l.id)}
                >
                  {l.label}
                </button>
              ))}
              <label className="flex items-center gap-1">
                CS
                <input className="w-10 border border-rule bg-ink px-1" type="number" min={0} max={10} value={exactH} onChange={(e) => { setExactH(Number(e.target.value)); setLine("cs-x"); }} />
                –
                <input className="w-10 border border-rule bg-ink px-1" type="number" min={0} max={10} value={exactA} onChange={(e) => { setExactA(Number(e.target.value)); setLine("cs-x"); }} />
              </label>
            </div>
          )}
          <PdfChart cells={pdf} sel={sel} onPick={toggle} heat={isSkellam} />
        </div>
        <aside className="border border-amber/40 bg-amber/5 p-4 font-mono text-xs">
          <p className="uppercase tracking-widest text-amber">{settled ? "Settlement ticket" : tradingClosed ? "Trading closed" : "Trade ticket"}</p>
          <p className="mt-2 text-[10px] leading-relaxed text-paper/50">
            {settled
              ? "This prediction market is settled. Quotes stay visible for review. Claim ⌊ρ·face⌋ or refund on Portfolio."
              : needsRho
                ? LOCK_RHO_HINT
                : tradingClosed
                ? "Trading closed at the deadline. No buys or sells on any prediction market after close_ts."
                : "Session may sign buy. Sells are closed. Same nonce retries the same fill. Coverage is display only."}
          </p>
          <label className="mt-4 block text-[10px] uppercase tracking-widest text-paper/50">Shares q</label>
          <input
            className="mt-1 w-full border border-rule bg-ink px-2 py-1"
            type="number"
            min={1}
            value={shares}
            onChange={(e) => setShares(Number(e.target.value))}
          />
          <label className="mt-3 block text-[10px] uppercase tracking-widest text-paper/50">Nonce</label>
          <input
            className="mt-1 w-full border border-rule bg-ink px-2 py-1"
            type="number"
            min={1}
            value={nonce}
            onChange={(e) => setNonce(Number(e.target.value))}
          />
          <p className="mt-2 text-[10px] text-paper/45">
            mask={mask || "—"} · n={n} · first fill is nonce 1, then last+1. Same nonce retries the same fill.
          </p>
          {held && (
            <p className="mt-1 text-[10px] text-paper/45">
              inventory {held.kind === "skellam" ? `skellam ${held.skellam_kind}/${held.a}/${held.b}` : `mask ${held.mask}`}
            </p>
          )}
          <p className="mt-4 mb-1 font-mono text-[11px] uppercase tracking-widest text-amber">Quote</p>
          <Row k="p_S" v={quote ? `${(quote.p_s_bps / 100).toFixed(2)}%` : "—"} />
          <Row k="C_S(q)" v={quote ? `${quote.c_s_usdc} USDC` : "—"} />
          <p className="mb-1 text-[10px] leading-snug text-paper/45">{LMSR_COST_HINT}</p>
          <Row k="coverage" v={quote ? coveragePct(quote.l_max_usdc, quote.coverage_bps) : "—"} warn={!!quote && coverageLow(quote.l_max_usdc, quote.coverage_bps)} />
          <Row k="ρ̂" v={quote ? coveragePct(quote.l_max_usdc, quote.rho_hat_bps) : "—"} />
          <Row
            k="fee"
            v={
              quote
                ? `${quote.fee_bps ?? 0} bps · ${info?.fee_timing === 1 ? "at claim (from payout)" : "at fill"} · not C_P`
                : "φ — platform fee account, not C_P"
            }
          />
          <Row k="L_max" v={quote ? `${quote.l_max_usdc}` : "—"} />
          <Row k="C_max" v={quote ? `${quote.c_max_usdc}` : "—"} />

          <div className="mt-5 border border-amber/40 bg-amber/5 p-3">
            <p className="font-mono text-[11px] uppercase tracking-widest text-amber">Pre-bet ticket</p>
            <p className="mt-1 text-[10px] text-paper/45">
              Pay = C_S + φ·C_S. Hit pays ⌊ρ̂·q⌋ integer USDC. Miss pays 0. ρ̂ / Book EV are display only.
            </p>
            <div className="mt-3">
              <Row k="You pay now" v={quote ? `${quote.pay_usdc ?? quote.c_s_usdc} USDC` : "—"} />
              <Row k="Fee" v={quote ? `${quote.fee_usdc ?? 0} USDC` : "—"} />
              <Row k="If S hits" v={quote ? `${quote.payout_if_hit_usdc ?? "—"} USDC` : "—"} />
              <Row k="If S misses" v={quote ? `${quote.payout_if_miss_usdc ?? 0} USDC` : "—"} />
              <Row k="Net if hit" v={quote ? signedUsdc(quote.net_if_hit ?? 0) : "—"} warn={!!quote && (quote.net_if_hit ?? 0) < 0} />
              <Row k="Book EV" v={quote ? signedUsdc(quote.ev_if_p_s ?? 0) : "—"} />
            </div>
            <p className="mt-2 text-[10px] text-paper/40">Book EV uses p_S as if you believe the book. Not a promise.</p>
          </div>
          {quote && coverageLow(quote.l_max_usdc, quote.coverage_bps) && (
            <p className="mt-3 text-amber">Low coverage. The order is still allowed.</p>
          )}
          <button className="mt-4 w-full bg-amber py-2 text-ink disabled:opacity-50" disabled={busy || tradingClosed} onClick={() => trade(true)}>
            {busy ? "Pending…" : tradingClosed ? "Trading closed" : typedLine() ? "Buy line" : "Buy set"}
          </button>
          <button className="mt-2 w-full border border-rule py-2 disabled:opacity-50" disabled onClick={() => trade(false)}>
            Sells closed
          </button>
          <p className="mt-2 text-[10px] text-paper/45">{SELLS_CLOSED}</p>
          {note && (
            <p className={`mt-2 text-[10px] ${note.startsWith("pending") ? "text-amber" : "text-moss"}`}>
              {note}
            </p>
          )}
        </aside>
      </section>

      <section className="mt-8 border border-rule p-4">
        <p className="font-mono text-[11px] uppercase tracking-widest text-amber">Comments</p>
        <p className="mt-1 text-[11px] text-paper/45">
          After the market is live. Off-chain catalog — not settlement, not C_P. Connect a wallet to post.
          Newest first · {COMMENT_LIMIT} per page.
        </p>
        <p className="mt-3 font-mono text-[11px] text-paper/45">
          {commentTotal} comments · page {commentPage} of {commentPages}
        </p>
        {comments.length === 0 ? (
          <p className="mt-4 font-mono text-xs text-paper/40">No comments yet.</p>
        ) : (
          <ol className="mt-4 space-y-3">
            {comments.map((c) => (
              <li key={c.id} className="border-b border-rule/60 pb-3">
                <p className="font-mono text-[11px] text-paper/50">
                  {shortPubkey(c.author)} · {formatUnix(c.created_at)}
                </p>
                <p className="mt-1 whitespace-pre-wrap text-sm leading-relaxed text-paper/80">{c.body}</p>
              </li>
            ))}
          </ol>
        )}
        {commentPages > 1 && (
          <nav className="mt-3 flex items-center justify-between font-mono text-[11px] uppercase tracking-widest">
            <button
              type="button"
              disabled={commentPage <= 1}
              className="disabled:text-paper/25"
              onClick={() => setCommentPage((p) => Math.max(1, p - 1))}
            >
              Previous
            </button>
            <span className="text-paper/45">
              page {commentPage} / {commentPages}
            </span>
            <button
              type="button"
              disabled={commentPage >= commentPages}
              className="disabled:text-paper/25"
              onClick={() => setCommentPage((p) => Math.min(commentPages, p + 1))}
            >
              Next
            </button>
          </nav>
        )}
        <label className="mt-4 block font-mono text-[10px] uppercase tracking-widest text-paper/50">Write a comment</label>
        <textarea
          className="mt-1 min-h-24 w-full border border-rule bg-ink px-2 py-1 text-sm disabled:opacity-50"
          maxLength={2000}
          disabled={!publicKey || commentBusy}
          value={commentBody}
          placeholder={publicKey ? "1–2000 characters" : NEED_WALLET}
          onChange={(e) => setCommentBody(e.target.value)}
        />
        <button
          className="mt-2 bg-amber px-4 py-2 font-mono text-xs uppercase text-ink disabled:opacity-50"
          disabled={!publicKey || commentBusy || !commentBody.trim()}
          onClick={async () => {
            if (!publicKey) {
              setCommentNote(NEED_WALLET);
              return;
            }
            setCommentBusy(true);
            setCommentNote("");
            try {
              await postComment(MARKET_API, market, publicKey.toBase58(), commentBody);
              setCommentBody("");
              // Newest-first: new posts land on page 1 (FR-UI-42).
              if (commentPage === 1) {
                const page = await listComments(MARKET_API, market, 1, COMMENT_LIMIT);
                setComments(page.items ?? []);
                setCommentPages(page.pages ?? 1);
                setCommentTotal(page.total ?? 0);
              } else {
                setCommentPage(1);
              }
            } catch (e) {
              setCommentNote(e instanceof Error ? e.message : "comment");
            } finally {
              setCommentBusy(false);
            }
          }}
        >
          {commentBusy ? "Posting…" : "Post comment"}
        </button>
        {commentNote && <p className="mt-2 font-mono text-xs text-rust">{commentNote}</p>}
      </section>
    </div>
  );
}

function Stat({ k, v, hint, warn }: { k: string; v: string; hint: string; warn?: boolean }) {
  return (
    <div className={warn ? "text-amber" : ""}>
      <dt className="text-[10px] uppercase tracking-widest text-paper/45">{k}</dt>
      <dd className="mt-1 text-sm">{v}</dd>
      <p className="mt-0.5 text-[10px] text-paper/35">{hint}</p>
    </div>
  );
}

function signedUsdc(n: number): string {
  if (n > 0) return `+${n} USDC`;
  return `${n} USDC`;
}

function Row({ k, v, warn }: { k: string; v: string; warn?: boolean }) {
  return (
    <div className={`flex justify-between border-b border-rule/60 py-1.5 ${warn ? "text-amber" : ""}`}>
      <span className="text-paper/50">{k}</span>
      <span>{v}</span>
    </div>
  );
}

