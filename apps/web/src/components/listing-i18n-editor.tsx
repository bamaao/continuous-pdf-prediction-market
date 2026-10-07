"use client";

import { saveListing, type ListingI18n, type ListingMeta } from "@cpm/sdk";
import { useWallet } from "@solana/wallet-adapter-react";
import { useEffect, useState } from "react";
import { MARKET_API } from "@/lib/env";
import { NEED_WALLET } from "@/lib/copy";

/** Light BCP-47 normalize aligned with readpath `normalize_locale` (non-en). */
function normalizeLocaleTag(raw: string): string {
  const t = raw.trim().replace(/_/g, "-");
  if (!t) return "";
  const parts = t.split("-").filter(Boolean);
  const lang = (parts[0] ?? "").toLowerCase();
  if (lang.length < 2) return t;
  const out = [lang];
  for (const p of parts.slice(1, 3)) {
    if (p.length === 2 && /^[a-zA-Z]+$/.test(p)) out.push(p.toUpperCase());
    else out.push(p.charAt(0).toUpperCase() + p.slice(1).toLowerCase());
  }
  return out.join("-");
}

function i18nRow(i18n: ListingI18n | undefined, locale: string) {
  if (!i18n) return undefined;
  const want = normalizeLocaleTag(locale);
  const hit = Object.entries(i18n).find(([k]) => normalizeLocaleTag(k) === want || k === locale.trim());
  return hit?.[1];
}

/** Post-OPEN translation editor — English identity is frozen (FR-UI-48). */
export function ListingI18nEditor({
  market,
  meta,
  onSaved,
}: {
  market: string;
  meta: {
    titleEn: string;
    eventEn: string;
    descriptionEn: string;
    tags: string[];
    topic?: string;
    tag?: string;
    sourceLocale?: string;
    i18n?: ListingI18n;
    blockedRegions?: string[];
  };
  onSaved?: (next: ListingMeta) => void;
}) {
  const { publicKey } = useWallet();
  const locales = Object.keys(meta.i18n ?? {});
  const [open, setOpen] = useState(false);
  const [locale, setLocale] = useState(locales[0] ?? "zh-Hans");
  const [title, setTitle] = useState("");
  const [event, setEvent] = useState("");
  const [description, setDescription] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState("");

  function loadLocaleFields(next: string) {
    const row = i18nRow(meta.i18n, next);
    setTitle(row?.title ?? "");
    setEvent(row?.event ?? "");
    setDescription(row?.description ?? "");
  }

  useEffect(() => {
    const keys = Object.keys(meta.i18n ?? {});
    if (!keys.length) return;
    const current = normalizeLocaleTag(locale);
    const stillThere = keys.some((k) => normalizeLocaleTag(k) === current);
    const next = stillThere ? locale : keys[0];
    if (!stillThere) setLocale(next);
    loadLocaleFields(next);
  }, [meta.i18n]);

  async function save() {
    if (!publicKey) {
      setNote(NEED_WALLET);
      return;
    }
    const loc = normalizeLocaleTag(locale);
    if (!loc || /^en(-|$)/i.test(loc)) {
      setNote("use a non-English BCP-47 tag, e.g. zh-Hans");
      return;
    }
    setBusy(true);
    setNote("saving translation…");
    try {
      const i18n: ListingI18n = { ...(meta.i18n ?? {}) };
      // Drop any case-variant of the same tag before write.
      for (const k of Object.keys(i18n)) {
        if (normalizeLocaleTag(k) === loc) delete i18n[k];
      }
      if (!title.trim() && !event.trim() && !description.trim()) {
        /* deleted */
      } else {
        i18n[loc] = {
          title: title.trim(),
          event: event.trim(),
          description: description.trim(),
        };
      }
      const payload: ListingMeta = {
        market,
        title: meta.titleEn,
        title_en: meta.titleEn,
        event: meta.eventEn,
        event_en: meta.eventEn,
        description: meta.descriptionEn,
        description_en: meta.descriptionEn,
        tags: meta.tags,
        topic: meta.topic ?? "",
        tag: meta.tag ?? "",
        source_locale: meta.sourceLocale ?? "en",
        i18n,
        blocked_regions: meta.blockedRegions ?? [],
      };
      await saveListing(MARKET_API, payload);
      setLocale(loc);
      setNote(`saved ${loc}`);
      onSaved?.(payload);
    } catch (e) {
      const msg = e instanceof Error ? e.message : "save failed";
      setNote(msg.includes("409") ? "canonical English is locked after OPEN" : msg);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="mt-4 border border-rule p-4 font-mono text-xs">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="uppercase tracking-widest text-amber">Translations</p>
        <button type="button" className="text-amber" onClick={() => setOpen((v) => !v)}>
          {open ? "Hide" : "Add / edit"}
        </button>
      </div>
      <p className="mt-1 text-[10px] normal-case leading-relaxed text-paper/45">
        English title, event, and description are frozen after OPEN. Locale strings are display only.
      </p>
      {locales.length ? (
        <p className="mt-2 text-paper/50">Have {locales.join(" · ")}</p>
      ) : (
        <p className="mt-2 text-paper/40">No locale strings yet.</p>
      )}
      {open ? (
        <div className="mt-3 space-y-2">
          <label className="block text-[10px] uppercase text-paper/50">
            Locale
            <input
              className="mt-1 w-full border border-rule bg-ink px-2 py-1 text-[12px] normal-case"
              value={locale}
              onChange={(e) => setLocale(e.target.value)}
              onBlur={() => loadLocaleFields(locale)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  loadLocaleFields(locale);
                }
              }}
              list="cpm-locale-hints"
              maxLength={16}
            />
            <datalist id="cpm-locale-hints">
              {["zh-Hans", "zh-Hant", "ja", "ko", "pt-BR", "es", "fr", "de", ...locales].map((x) => (
                <option key={x} value={x} />
              ))}
            </datalist>
          </label>
          {locales.length ? (
            <p className="flex flex-wrap gap-1 text-[10px] normal-case text-paper/45">
              {locales.map((loc) => (
                <button
                  key={loc}
                  type="button"
                  className={`border px-1.5 py-0.5 ${normalizeLocaleTag(loc) === normalizeLocaleTag(locale) ? "border-amber text-amber" : "border-rule"}`}
                  onClick={() => {
                    setLocale(loc);
                    loadLocaleFields(loc);
                  }}
                >
                  {loc}
                </button>
              ))}
            </p>
          ) : null}
          <label className="block text-[10px] uppercase text-paper/50">
            Title
            <input
              className="mt-1 w-full border border-rule bg-ink px-2 py-1 text-[12px] normal-case"
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              maxLength={120}
            />
          </label>
          <label className="block text-[10px] uppercase text-paper/50">
            Trading event
            <input
              className="mt-1 w-full border border-rule bg-ink px-2 py-1 text-[12px] normal-case"
              value={event}
              onChange={(e) => setEvent(e.target.value)}
              maxLength={160}
            />
          </label>
          <label className="block text-[10px] uppercase text-paper/50">
            Description
            <textarea
              className="mt-1 min-h-[4.5rem] w-full border border-rule bg-ink px-2 py-1 text-[12px] normal-case"
              value={description}
              onChange={(e) => setDescription(e.target.value)}
              rows={3}
              maxLength={2000}
            />
          </label>
          <button
            type="button"
            disabled={busy}
            onClick={() => void save()}
            className="bg-amber px-3 py-1 text-ink disabled:opacity-50"
          >
            Save translation
          </button>
          {note ? <p className="text-paper/55">{note}</p> : null}
        </div>
      ) : null}
    </div>
  );
}
