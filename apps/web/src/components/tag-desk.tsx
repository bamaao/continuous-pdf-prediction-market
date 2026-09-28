"use client";

import { createCatalogTag, deleteCatalogTag, listCatalogTags, type CatalogTag } from "@cpm/sdk";
import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import { MARKET_API } from "@/lib/env";

export function TagDesk() {
  const [rows, setRows] = useState<CatalogTag[]>([]);
  const [name, setName] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    const items = await listCatalogTags(MARKET_API);
    setRows(items);
  }, []);

  useEffect(() => {
    load().catch((e) => setNote(e instanceof Error ? e.message : "tags unreachable"));
  }, [load]);

  async function addTag() {
    const next = name.trim();
    if (!next) {
      setNote("Type a tag name.");
      return;
    }
    setBusy(true);
    try {
      await createCatalogTag(MARKET_API, next);
      setName("");
      setNote(`Added ${next}`);
      await load();
    } catch (e) {
      setNote(e instanceof Error ? e.message : "add failed");
    } finally {
      setBusy(false);
    }
  }

  async function removeTag(tag: CatalogTag) {
    if (tag.used > 0) {
      setNote(`${tag.name} is on ${tag.used} market(s)`);
      return;
    }
    setBusy(true);
    try {
      await deleteCatalogTag(MARKET_API, tag.name);
      setNote(`Deleted ${tag.name}`);
      await load();
    } catch (e) {
      setNote(e instanceof Error ? e.message : "delete failed");
    } finally {
      setBusy(false);
    }
  }

  return (
    <div>
      <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-amber">Catalog</p>
      <h1 className="font-display text-5xl">Tags</h1>
      <p className="mt-2 max-w-2xl text-sm leading-relaxed text-paper/70">
        Tags are a maintained English vocabulary. Add epl or world cup here, then attach several to a market on{" "}
        <Link href="/create" className="text-amber">
          Create
        </Link>
        . A tag with no markets can be deleted. In-use tags stay until every market drops them. This is
        public-card metadata — not the on-chain series key.
      </p>

      <form
        className="mt-8 flex flex-wrap items-end gap-3 border border-rule p-4"
        onSubmit={(e) => {
          e.preventDefault();
          void addTag();
        }}
      >
        <label className="block min-w-[16rem] flex-1 font-mono text-[10px] uppercase tracking-widest text-paper/50">
          New tag
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="epl"
            maxLength={24}
            className="mt-1 w-full border border-rule bg-ink px-2 py-1.5 font-mono text-sm normal-case tracking-normal text-paper"
          />
        </label>
        <button
          type="submit"
          disabled={busy}
          className="bg-amber px-4 py-1.5 font-mono text-xs uppercase tracking-widest text-ink disabled:opacity-40"
        >
          Add
        </button>
      </form>
      {note ? <p className="mt-3 font-mono text-[11px] text-amber">{note}</p> : null}

      <p className="mt-6 font-mono text-[11px] text-paper/45">{rows.length} tags</p>
      <ul className="mt-2 divide-y divide-rule border border-rule">
        {rows.map((t) => (
          <li key={t.name} className="grid grid-cols-[1fr_auto_auto] items-center gap-4 px-4 py-3">
            <div>
              <p className="font-display text-xl">{t.name}</p>
              <p className="mt-0.5 font-mono text-[11px] text-paper/45">
                {t.used ? `used by ${t.used} market${t.used === 1 ? "" : "s"}` : "unused"}
              </p>
            </div>
            <Link href={`/?tag=${encodeURIComponent(t.name)}`} className="font-mono text-[11px] uppercase text-amber">
              Lobby
            </Link>
            <button
              type="button"
              disabled={busy || t.used > 0}
              onClick={() => void removeTag(t)}
              className="font-mono text-[11px] uppercase tracking-widest text-paper/50 hover:text-rust disabled:cursor-not-allowed disabled:text-paper/20"
            >
              {t.used > 0 ? "In use" : "Delete"}
            </button>
          </li>
        ))}
        {!rows.length && <li className="px-4 py-8 font-mono text-sm text-paper/50">No tags yet.</li>}
      </ul>
    </div>
  );
}
