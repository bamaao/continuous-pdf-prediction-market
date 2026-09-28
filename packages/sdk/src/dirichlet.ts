import { GRID_CREATE_CAP, gridGrowSteps, gridSpace } from "./pda";

export const DIRICHLET_ATOMS = 0;
export const DIRICHLET_TOP_N = 1;
export const DIRICHLET_SIMPLEX = 2;
export const MAX_N = 1024;

/** Same integer binomial as `math::prior::binom`. */
export function binom(n: number, k: number): number | null {
  if (!Number.isInteger(n) || !Number.isInteger(k) || n < 0 || k < 0 || k > n) return null;
  const kk = Math.min(k, n - k);
  let acc = 1n;
  for (let i = 0; i < kk; i++) {
    acc = (acc * BigInt(n - i)) / BigInt(i + 1);
    if (acc > 0xffffffffn) return null;
  }
  return Number(acc);
}

export function simplexCellCount(k: number, bins: number): number | null {
  if (!Number.isInteger(k) || !Number.isInteger(bins) || k < 2 || bins < 1) return null;
  return binom(bins + k - 1, k - 1);
}

export function dirichletCellCount(args: {
  layout: number;
  nAtoms?: number;
  k?: number;
  topN?: number;
  bins?: number;
}): number | null {
  if (args.layout === DIRICHLET_ATOMS) {
    const n = args.nAtoms ?? 0;
    return n >= 2 && n <= MAX_N ? n : null;
  }
  const k = args.k ?? 0;
  if (args.layout === DIRICHLET_TOP_N) {
    const topN = args.topN ?? 0;
    if (k < 2 || topN < 1 || topN >= k) return null;
    const n = binom(k, topN);
    return n != null && n >= 2 && n <= MAX_N ? n : null;
  }
  if (args.layout === DIRICHLET_SIMPLEX) {
    const n = simplexCellCount(k, args.bins ?? 0);
    return n != null && n >= 2 && n <= MAX_N ? n : null;
  }
  return null;
}

export function dirichletNeedsGrow(n: number): boolean {
  return gridSpace(n) > GRID_CREATE_CAP;
}

export function dirichletGrowHint(n: number): string {
  const g = gridGrowSteps(n);
  if (g <= 0) return "";
  return `create + ${g} grow_grid`;
}
