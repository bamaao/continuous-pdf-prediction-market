import { describe, expect, it } from "vitest";
import {
  DIRICHLET_SIMPLEX,
  DIRICHLET_TOP_N,
  binom,
  dirichletCellCount,
  dirichletNeedsGrow,
  simplexCellCount,
} from "./dirichlet";
import { GRID_CREATE_CAP, gridSpace } from "./pda";

describe("dirichlet simplex counts", () => {
  it("matches on-chain binom / simplex_cell_count", () => {
    expect(binom(4, 2)).toBe(6);
    expect(simplexCellCount(2, 4)).toBe(5);
    expect(simplexCellCount(2, 158)).toBe(159);
    expect(simplexCellCount(4, 10)).toBe(286);
    expect(gridSpace(159)).toBeGreaterThan(GRID_CREATE_CAP);
    expect(dirichletNeedsGrow(159)).toBe(true);
    expect(dirichletNeedsGrow(4)).toBe(false);
  });

  it("rejects simplex over MAX_N", () => {
    expect(dirichletCellCount({ layout: DIRICHLET_SIMPLEX, k: 4, bins: 17 })).toBeNull();
  });

  it("counts top-n combinations", () => {
    expect(dirichletCellCount({ layout: DIRICHLET_TOP_N, k: 4, topN: 2 })).toBe(6);
    expect(dirichletCellCount({ layout: DIRICHLET_TOP_N, k: 11, topN: 3 })).toBe(165);
    expect(dirichletNeedsGrow(165)).toBe(true);
  });
});
