import type { PdfCell } from "@cpm/sdk";

const BIN_CAP = 32;

function pct(bps: number): string {
  return `${(bps / 100).toFixed(2)}%`;
}

function binCells(cells: PdfCell[]): { start: number; end: number; p_bps: number; e: number }[] {
  if (cells.length <= BIN_CAP) {
    return cells.map((c) => ({ start: c.cell, end: c.cell, p_bps: c.p_bps, e: c.e }));
  }
  const w = Math.ceil(cells.length / BIN_CAP);
  const out = [];
  for (let i = 0; i < cells.length; i += w) {
    const slice = cells.slice(i, i + w);
    out.push({
      start: slice[0].cell,
      end: slice[slice.length - 1].cell,
      p_bps: slice.reduce((s, c) => s + c.p_bps, 0),
      e: slice.reduce((m, c) => Math.max(m, c.e), 0),
    });
  }
  return out;
}

export function PdfChart({
  cells,
  sel,
  onPick,
  heat,
}: {
  cells: PdfCell[];
  sel: boolean[];
  onPick: (start: number, end?: number) => void;
  heat: boolean;
}) {
  const selectedBps = cells.reduce((s, c) => s + (sel[c.cell] ? c.p_bps : 0), 0);
  const peak = cells.reduce((a, c) => (c.p_bps > a.p_bps ? c : a), cells[0] ?? { cell: 0, p_bps: 0, e: 0 });

  if (heat) {
    const max = Math.max(1, ...cells.map((c) => c.p_bps));
    return (
      <div>
        <p className="mb-2 font-mono text-[10px] text-paper/45">
          selected {pct(selectedBps)} · peak score {labelScore(peak.cell)} {pct(peak.p_bps)}
        </p>
        <div className="grid grid-cols-11 gap-0.5">
          {cells.map((c) => {
            const t = c.p_bps / max;
            return (
              <button
                key={c.cell}
                type="button"
                onClick={() => onPick(c.cell)}
                title={`score ${labelScore(c.cell)} · ${pct(c.p_bps)} · E ${c.e} USDC`}
                className={`aspect-square ${sel[c.cell] ? "ring-2 ring-amber" : ""}`}
                style={{ background: `rgba(212,160,23,${0.08 + t * 0.85})` }}
              />
            );
          })}
        </div>
        <p className="mt-2 font-mono text-[10px] text-paper/35">11×11 heat = p_k. Overflow cells are 10+. Hover is percent, not E.</p>
      </div>
    );
  }

  const bars = binCells(cells);
  const max = Math.max(1, ...bars.map((b) => b.p_bps));
  const binned = cells.length > BIN_CAP;
  const peakE = cells.reduce((a, c) => (c.e > a.e ? c : a), cells[0] ?? { cell: 0, p_bps: 0, e: 0 });
  return (
    <div>
      <p className="mb-2 font-mono text-[10px] text-paper/45">
        selected {pct(selectedBps)} · PDF peak {pct(peak.p_bps)}
        {peakE.e > 0 ? ` · thickest overlap pays ${peakE.e} USDC` : ""}
        {binned ? ` · ${cells.length} samples binned to ${bars.length}` : ""}
      </p>
      <div className="flex h-64 items-end gap-px">
        {bars.map((b) => {
          const on = rangeOn(sel, b.start, b.end);
          const hot = peakE.e > 0 && peakE.cell >= b.start && peakE.cell <= b.end;
          return (
            <button
              key={`${b.start}-${b.end}`}
              type="button"
              onClick={() => onPick(b.start, b.end)}
              className={`group relative min-w-0 flex-1 ${hot ? "ring-1 ring-rust" : ""}`}
              style={{ height: `${Math.max(6, (b.p_bps / max) * 100)}%` }}
              title={`${pct(b.p_bps)} · overlap depth ${b.e} USDC`}
            >
              <span className={`block h-full ${on ? "bg-amber" : "bg-paper/25 group-hover:bg-paper/40"}`} />
            </button>
          );
        })}
      </div>
      <p className="mt-2 font-mono text-[10px] text-paper/35">
        Bar height is p_k (relative to the peak). Hover shows percent and face E — E is not the PDF.
      </p>
    </div>
  );
}

function labelScore(cell: number): string {
  const home = Math.floor(cell / 11);
  const away = cell % 11;
  return `${home === 10 ? "10+" : home}–${away === 10 ? "10+" : away}`;
}

function rangeOn(sel: boolean[], start: number, end: number): boolean {
  for (let i = start; i <= end && i < sel.length; i++) {
    if (sel[i]) return true;
  }
  return false;
}
