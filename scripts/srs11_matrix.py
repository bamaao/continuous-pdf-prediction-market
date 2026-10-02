#!/usr/bin/env python3
"""SRS §11.1 FR/CR matrix. Every table ID must map to an existing test or manual doc."""

from __future__ import annotations

import json
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRS = ROOT / "docs" / "software-requirements-specification.md"
MATRIX_JSON = ROOT / "docs" / "srs11-fr-matrix.json"
ID_RE = re.compile(r"^\| (FR-[A-Z]+-\d+|CR-\d+) \|", re.M)

PREFIX: dict[str, dict] = {
    "FR-MKT": {
        "kind": "auto",
        "evidence": [
            "crates/math/src/prior.rs",
            "scripts/live-magicblock-e2e.py",
            "scripts/phase6-family-cases.py",
        ],
    },
    "FR-WAL": {
        "kind": "auto",
        "evidence": [
            "programs/market/src/session.rs",
            "programs/vault/src/lib.rs",
            "crates/services/gateway/src/lib.rs",
            "packages/sdk/src/session-store.ts",
        ],
    },
    "FR-TRD": {
        "kind": "auto",
        "evidence": [
            "crates/math/src/lmsr.rs",
            "crates/services/quote/src/lib.rs",
            "scripts/live-magicblock-e2e.py",
            "crates/services/gateway/src/lib.rs",
        ],
    },
    "FR-RSK": {
        "kind": "auto",
        "evidence": [
            "programs/risk/src/matching.rs",
            "crates/math/src/auction.rs",
            "scripts/live-magicblock-e2e.py",
        ],
    },
    "FR-HAL": {
        "kind": "auto",
        "evidence": ["scripts/live-magicblock-e2e.py", "scripts/phase8-keeper.py"],
    },
    "FR-RES": {
        "kind": "auto",
        "evidence": [
            "programs/resolution/src/machine.rs",
            "scripts/live-magicblock-e2e.py",
        ],
    },
    "FR-SET": {
        "kind": "auto",
        "evidence": [
            "crates/math/src/settle.rs",
            "programs/vault/src/settle.rs",
            "tests/settle-flow/tests/cpi_market_rho_haircut.rs",
        ],
    },
    "FR-DUR": {
        "kind": "auto",
        "evidence": [
            "crates/journal/src/lib.rs",
            "scripts/srs11-dur-drill.py",
            "scripts/phase7-journal.py",
        ],
    },
    "FR-UI": {
        "kind": "auto",
        "evidence": [
            "scripts/phase6-playwright.py",
            "scripts/phase6-flow-playwright.py",
            "docs/srs11-manual.md",
        ],
    },
    "FR-CLI": {
        "kind": "auto",
        "evidence": ["crates/cli/src/main.rs", "scripts/live-magicblock-e2e.py"],
    },
    "FR-IDX": {
        "kind": "auto",
        "evidence": [
            "crates/services/readpath/src/indexer.rs",
            "crates/services/readpath/src/bin/indexer.rs",
        ],
    },
    "CR": {
        "kind": "auto",
        "evidence": [
            "apps/web/package.json",
            "scripts/srs11-local.py",
            "docs/srs11-manual.md",
        ],
    },
}

SPECIFIC: dict[str, dict] = {
    "FR-MKT-05": {
        "kind": "auto",
        "evidence": ["crates/math/src/football.rs", "scripts/phase6-football-cases.py"],
    },
    "FR-MKT-06": {
        "kind": "auto",
        "evidence": ["scripts/live-magicblock-e2e.py", "programs/market/src/lib.rs"],
    },
    "FR-WAL-01": {
        "kind": "manual",
        "evidence": ["docs/srs11-manual.md", "scripts/wallet-shell-playwright.py"],
    },
    "FR-WAL-03": {
        "kind": "auto",
        "evidence": [
            "programs/vault/src/lib.rs",
            "packages/sdk/src/vault.ts",
            "scripts/srs11-local.py",
        ],
    },
    "FR-WAL-08": {
        "kind": "auto",
        "evidence": [
            "apps/web/src/lib/localnet-wallet.ts",
            "packages/sdk/src/session-store.ts",
            "docs/srs11-manual.md",
        ],
    },
    "FR-WAL-09": {
        "kind": "auto",
        "evidence": ["crates/services/gateway/src/lib.rs", "crates/services/gateway/src/store.rs"],
    },
    "FR-TRD-09": {
        "kind": "auto",
        "evidence": ["crates/services/gateway/src/lib.rs", "scripts/srs11-local.py"],
    },
    "FR-TRD-11": {
        "kind": "auto",
        "evidence": ["crates/math/src/football.rs", "scripts/live-magicblock-e2e.py"],
    },
    "FR-TRD-12": {
        "kind": "auto",
        "evidence": ["crates/math/src/outcome.rs", "docs/product-specification.md"],
    },
    "FR-TRD-13": {
        "kind": "auto",
        "evidence": ["crates/math/src/lmsr.rs", "crates/math/src/settle.rs"],
    },
    "FR-DUR-01": {
        "kind": "auto",
        "evidence": ["scripts/srs11-dur-drill.py", "crates/journal/src/lib.rs"],
    },
    "FR-DUR-03": {
        "kind": "auto",
        "evidence": ["crates/journal/src/lib.rs", "docs/srs11-manual.md"],
    },
    "FR-DUR-04": {
        "kind": "auto",
        "evidence": [
            "crates/services/readpath/src/lib.rs",
            "docs/srs11-manual.md",
            "scripts/srs11-dur-drill.py",
        ],
    },
    "FR-UI-01": {
        "kind": "auto",
        "evidence": ["scripts/phase6-playwright.py", "apps/web/src"],
    },
    "FR-UI-02": {"kind": "manual", "evidence": ["docs/srs11-manual.md", "apps/android-twa/twa-manifest.json"]},
    "FR-UI-25": {
        "kind": "removed",
        "evidence": ["docs/software-requirements-specification.md"],
    },
    "FR-UI-28": {"kind": "manual", "evidence": ["docs/srs11-manual.md", "crates/cli/src/main.rs"]},
    "CR-01": {"kind": "auto", "evidence": ["apps/web/package.json", "scripts/srs11-local.py"]},
    "CR-02": {"kind": "manual", "evidence": ["docs/srs11-manual.md", "scripts/srs11-local.py"]},
    "CR-03": {"kind": "manual", "evidence": ["docs/srs11-manual.md"]},
    "CR-04": {
        "kind": "auto",
        "evidence": ["programs/market/Cargo.toml", "programs/market/src/lib.rs"],
    },
    "CR-05": {
        "kind": "auto",
        "evidence": ["programs/vault/src/lib.rs", "programs/resolution/src/lib.rs"],
    },
    "CR-08": {
        "kind": "auto",
        "evidence": ["programs/vault/src/mint.rs", "scripts/srs11-local.py"],
    },
    "CR-09": {"kind": "auto", "evidence": ["programs/vault/src/lib.rs", "docs/srs11-manual.md"]},
}


def srs_ids() -> list[str]:
    text = SRS.read_text(encoding="utf-8")
    seen: list[str] = []
    for m in ID_RE.finditer(text):
        i = m.group(1)
        if i not in seen:
            seen.append(i)
    return seen


def prefix_of(fid: str) -> str:
    if fid.startswith("CR-"):
        return "CR"
    return fid.rsplit("-", 1)[0]


def row_for(fid: str) -> dict:
    if fid in SPECIFIC:
        row = dict(SPECIFIC[fid])
    else:
        p = prefix_of(fid)
        if p not in PREFIX:
            raise KeyError(f"{fid}: no prefix {p}")
        row = dict(PREFIX[p])
    row["id"] = fid
    return row


def build_matrix() -> list[dict]:
    return [row_for(i) for i in srs_ids()]


def write_matrix(rows: list[dict]) -> None:
    MATRIX_JSON.write_text(
        json.dumps({"version": 1, "count": len(rows), "entries": rows}, indent=2, ensure_ascii=False)
        + "\n",
        encoding="utf-8",
    )


def validate() -> list[str]:
    errors: list[str] = []
    ids = srs_ids()
    if not ids:
        return ["SRS produced zero FR-*/CR-* ids"]
    try:
        rows = build_matrix()
    except KeyError as e:
        return [str(e)]
    write_matrix(rows)
    by_id = {r["id"]: r for r in rows}
    for fid in ids:
        if fid not in by_id:
            errors.append(f"{fid} missing from matrix")
            continue
        row = by_id[fid]
        kind = row.get("kind")
        if kind not in {"auto", "manual", "removed"}:
            errors.append(f"{fid} bad kind {kind}")
        ev = row.get("evidence") or []
        if not ev:
            errors.append(f"{fid} has no evidence")
        for rel in ev:
            path = ROOT / rel
            if not path.exists():
                errors.append(f"{fid} evidence missing: {rel}")
    extra = set(by_id) - set(ids)
    if extra:
        errors.append(f"matrix extras not in SRS: {sorted(extra)}")
    return errors


if __name__ == "__main__":
    errs = validate()
    print(f"ids {len(srs_ids())} → {MATRIX_JSON}")
    if errs:
        print("errors:")
        for e in errs:
            print("-", e)
        raise SystemExit(1)
    print("ok")
