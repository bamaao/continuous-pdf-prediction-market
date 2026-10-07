"use client";

import { useState, type ReactNode } from "react";

export type ListingLocaleProps = {
  title?: string | null;
  titleEn?: string | null;
  event?: string | null;
  eventEn?: string | null;
  description?: string | null;
  descriptionEn?: string | null;
  locale?: string | null;
  isTranslation?: boolean;
  children?: (v: { title: string; event: string; description: string; showingEn: boolean }) => ReactNode;
};

/** Translation badge + English toggle for lobby / market cards (FR-UI-48). */
export function ListingLocale({
  title,
  titleEn,
  event,
  eventEn,
  description,
  descriptionEn,
  locale,
  isTranslation,
  children,
}: ListingLocaleProps) {
  const [showEn, setShowEn] = useState(false);
  const canToggle = !!isTranslation && !!(titleEn || eventEn || descriptionEn);
  const t = showEn ? titleEn || title || "" : title || "";
  const e = showEn ? eventEn || event || "" : event || "";
  const d = showEn ? descriptionEn || description || "" : description || "";
  return (
    <div>
      {canToggle ? (
        <p className="mb-1 flex flex-wrap items-center gap-2 font-mono text-[10px] uppercase tracking-widest text-paper/45">
          <span className="border border-rule px-1.5 py-0.5 normal-case">
            {showEn ? "English" : `Translation · ${locale || "?"}`}
          </span>
          <button type="button" className="text-amber" onClick={() => setShowEn((v) => !v)}>
            {showEn ? "Show translation" : "Show English"}
          </button>
        </p>
      ) : null}
      {children ? children({ title: t, event: e || "", description: d || "", showingEn: showEn }) : null}
    </div>
  );
}
