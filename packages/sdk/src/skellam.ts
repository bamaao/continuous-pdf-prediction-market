/** Typed Skellam templates → atom masks. Same rules as crates/math/src/football.rs. */

export const K_MAX = 10;
export const N_CELLS = 121;

export type SkellamLine = {
  id: string;
  label: string;
  kind: number;
  a: number;
  b: number;
};

export const SKELLAM_LINES: SkellamLine[] = [
  { id: "home", label: "Home", kind: 0, a: 0, b: 0 },
  { id: "draw", label: "Draw", kind: 1, a: 0, b: 0 },
  { id: "away", label: "Away", kind: 2, a: 0, b: 0 },
  { id: "h1x2-win", label: "AH 1X2 win −1", kind: -2, a: 1, b: 1 },
  { id: "h1x2-draw", label: "AH 1X2 draw −1", kind: -2, a: 1, b: 0 },
  { id: "h1x2-lose", label: "AH 1X2 lose −1", kind: -2, a: 1, b: -1 },
  { id: "over15", label: "Over 1.5", kind: 3, a: 3, b: 0 },
  { id: "over25", label: "Over 2.5", kind: 3, a: 5, b: 0 },
  { id: "over35", label: "Over 3.5", kind: 3, a: 7, b: 0 },
  { id: "under25", label: "Under 2.5", kind: 4, a: 5, b: 0 },
  { id: "btts_yes", label: "BTTS Yes", kind: 5, a: 0, b: 0 },
  { id: "btts_no", label: "BTTS No", kind: 6, a: 0, b: 0 },
  { id: "ah_home_05", label: "Home −0.5", kind: 8, a: -1, b: 0 },
  { id: "ah_home_10", label: "Home −1.0", kind: 8, a: -2, b: 0 },
  { id: "ah_home_15", label: "Home −1.5", kind: 8, a: -3, b: 0 },
  { id: "ah_home_q075", label: "Home −0.75 (quarter)", kind: 10, a: -3, b: 0 },
  { id: "custom", label: "Custom cells", kind: -1, a: 0, b: 0 },
];

/** Same as crates/math `quarter_to_halves`: odd quarters → two adjacent half-lines. */
export function quarterToHalves(quarters: number): [number, number] {
  const lo = Math.floor(quarters / 2);
  return [lo, lo + 1];
}

/** Asian-handicap 1X2 after home gives `h` goals: win / draw / lose. */
export function handicap1x2Mask(h: number, side: "win" | "draw" | "lose"): boolean[] {
  return mapCells((i, j) => {
    const d = i - j;
    if (side === "win") return d > h;
    if (side === "draw") return d === h;
    return d < h;
  });
}

function cell(home: number, away: number): number {
  return Math.min(home, K_MAX) * 11 + Math.min(away, K_MAX);
}

function mapCells(pred: (i: number, j: number) => boolean): boolean[] {
  const m = Array.from({ length: N_CELLS }, () => false);
  for (let i = 0; i <= K_MAX; i++) {
    for (let j = 0; j <= K_MAX; j++) m[cell(i, j)] = pred(i, j);
  }
  return m;
}

export function skellamMasks(kind: number, a: number, b: number): boolean[][] | null {
  switch (kind) {
    case 0:
      return [mapCells((i, j) => i > j)];
    case 1:
      return [mapCells((i, j) => i === j)];
    case 2:
      return [mapCells((i, j) => i < j)];
    case 3:
      return [mapCells((i, j) => 2 * (i + j) > a)];
    case 4:
      return [mapCells((i, j) => 2 * (i + j) <= a)];
    case 5:
      return [mapCells((i, j) => i >= 1 && j >= 1)];
    case 6:
      return [mapCells((i, j) => i === 0 || j === 0)];
    case 7: {
      const m = Array.from({ length: N_CELLS }, () => false);
      m[cell(a, b)] = true;
      return [m];
    }
    case 8:
      return [mapCells((i, j) => 2 * (i - j) + a > 0)];
    case 9:
      return [mapCells((i, j) => 2 * (j - i) + a > 0)];
    case 10:
    case 11: {
      if (a % 2 === 0) return null;
      const [x, y] = quarterToHalves(a);
      const home = kind === 10;
      return [
        mapCells((i, j) => (home ? 2 * (i - j) + x : 2 * (j - i) + x) > 0),
        mapCells((i, j) => (home ? 2 * (i - j) + y : 2 * (j - i) + y) > 0),
      ];
    }
    default:
      return null;
  }
}

export function exactLine(home: number, away: number): SkellamLine {
  return { id: `cs-${home}-${away}`, label: `Exact ${home}–${away}`, kind: 7, a: home, b: away };
}
