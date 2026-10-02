#!/usr/bin/env python3
"""Execute the unique suites behind every SRS FR-*/CR-* (no mapping-only pass)."""

from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import srs11_matrix  # noqa: E402

# suite name → (argv, timeout_s)
SUITES: dict[str, tuple[list[str], int]] = {
    "math": (["cargo", "test", "-p", "math", "--", "--test-threads=1"], 360),
    "journal": (["cargo", "test", "-p", "journal"], 180),
    "vault": (["cargo", "test", "-p", "vault"], 180),
    "resolution": (["cargo", "test", "-p", "resolution"], 180),
    "gateway": (["cargo", "test", "-p", "gateway"], 180),
    "notify": (["cargo", "test", "-p", "notify"], 120),
    "settle": (["cargo", "test", "-p", "settle-flow"], 600),
    "market": (["cargo", "test", "-p", "market"], 180),
    "risk": (["cargo", "test", "-p", "risk"], 180),
    "quote": (["cargo", "test", "-p", "quote"], 180),
    "readpath": (["cargo", "test", "-p", "readpath", "--lib", "--test", "http_quote", "--", "--test-threads=1"], 900),
    "math_wasm": (["cargo", "test", "-p", "math-wasm"], 180),
    "sdk": (
        ["npm.cmd" if sys.platform == "win32" else "npm", "test", "--prefix", str(ROOT / "packages" / "sdk")],
        180,
    ),
    "family": ([sys.executable, str(ROOT / "scripts" / "phase6-family-cases.py")], 1200),
    "football": ([sys.executable, str(ROOT / "scripts" / "phase6-football-cases.py")], 1800),
    "e2e": ([sys.executable, str(ROOT / "scripts" / "live-magicblock-e2e.py")], 1800),
    "pw": ([sys.executable, str(ROOT / "scripts" / "phase6-playwright.py")], 300),
    "pw_flow": ([sys.executable, str(ROOT / "scripts" / "phase6-flow-playwright.py")], 420),
    "pw_wallet": ([sys.executable, str(ROOT / "scripts" / "wallet-shell-playwright.py")], 300),
    "p7": ([sys.executable, str(ROOT / "scripts" / "phase7-journal.py")], 420),
    "p8": ([sys.executable, str(ROOT / "scripts" / "phase8-keeper.py")], 420),
}

PREFIX_SUITES: dict[str, list[str]] = {
    "FR-MKT": ["math", "family", "e2e"],
    "FR-WAL": ["vault", "market", "gateway", "sdk", "static"],
    "FR-TRD": ["math", "quote", "e2e", "gateway"],
    "FR-RSK": ["risk", "e2e"],
    "FR-HAL": ["e2e", "p8"],
    "FR-RES": ["resolution", "e2e"],
    "FR-SET": ["math", "vault", "settle"],
    "FR-DUR": ["journal", "p7"],
    "FR-UI": ["pw", "pw_flow", "static"],
    "FR-CLI": ["e2e"],
    "FR-IDX": ["readpath"],
    "CR": ["static"],
}

SPECIFIC_SUITES: dict[str, list[str]] = {
    "FR-MKT-05": ["math", "football", "e2e"],
    "FR-WAL-01": ["pw_wallet", "static"],
    "FR-WAL-08": ["sdk", "static"],
    "FR-TRD-11": ["math", "football", "e2e"],
    "FR-UI-02": ["static"],
    "FR-UI-25": ["static"],
    "FR-UI-28": ["static"],
    "CR-01": ["static"],
    "CR-02": ["static"],
    "CR-03": ["static"],
    "CR-04": ["static"],
    "CR-05": ["static"],
    "CR-08": ["vault", "static"],
    "CR-09": ["static"],
}


def suites_for(fid: str) -> list[str]:
    if fid in SPECIFIC_SUITES:
        return list(SPECIFIC_SUITES[fid])
    return list(PREFIX_SUITES[srs11_matrix.prefix_of(fid)])


def static_check() -> str | None:
    web = (ROOT / "apps" / "web" / "package.json").read_text(encoding="utf-8")
    if '"next"' not in web:
        return "CR-01 apps/web is not Next.js"
    banned = list((ROOT / "apps").rglob("*.apk")) + list((ROOT / "apps").rglob("*.ipa"))
    if banned:
        return f"CR-02 store binaries {banned}"
    twa = ROOT / "apps" / "android-twa" / "twa-manifest.json"
    if not twa.exists():
        return "FR-UI-02 missing TWA manifest"
    mkt = (ROOT / "programs" / "market" / "Cargo.toml").read_text(encoding="utf-8")
    if "ephemeral-rollups-sdk" not in mkt:
        return "CR-04 market missing ephemeral-rollups-sdk"
    for name in ("vault", "resolution"):
        text = (ROOT / "programs" / name / "src" / "lib.rs").read_text(encoding="utf-8")
        if "delegate_book" in text or "ephemeral_rollups_sdk" in text:
            return f"CR-05 {name} must not Delegate"
    mint = (ROOT / "programs" / "vault" / "src" / "mint.rs").read_text(encoding="utf-8")
    if "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v" not in mint:
        return "CR-08 Circle mainnet mint missing"
    blob = ""
    for p in (ROOT / "programs").rglob("*.rs"):
        blob += p.read_text(encoding="utf-8", errors="replace")
    if "jupiter" in blob.lower():
        return "CR-09 Jupiter reference in programs"
    sess = (ROOT / "packages" / "sdk" / "src" / "session-store.ts").read_text(encoding="utf-8")
    loc = (ROOT / "apps" / "web" / "src" / "lib" / "localnet-wallet.ts").read_text(encoding="utf-8")
    if "localStorage" in sess and "forbidden" not in sess.lower() and "never" not in sess.lower():
        if "setItem" in sess:
            return "FR-WAL-08 session-store writes localStorage"
    if "localStorage.setItem" in loc:
        return "FR-WAL-08 localnet wallet writes localStorage"
    ops = (ROOT / "apps" / "web" / "src" / "app" / "ops" / "page.tsx").read_text(encoding="utf-8")
    if "keeper close" not in ops.lower() and "Read-only" not in ops:
        return "FR-UI-28 /ops missing read-only keeper copy"
    srs = (ROOT / "docs" / "software-requirements-specification.md").read_text(encoding="utf-8")
    if "FR-UI-25" in srs and "SHALL NOT collect or inject $C_M" not in srs:
        return "FR-UI-25 C_M removal text missing"
    return None


def run_cmd(argv: list[str], timeout: int, extra_env: dict[str, str] | None = None) -> tuple[int, str]:
    env = os.environ.copy()
    if extra_env:
        env.update(extra_env)
    p = subprocess.run(
        argv,
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
        env=env,
    )
    text = ((p.stdout or "") + (p.stderr or "")).strip()
    return p.returncode, text


def execute() -> tuple[list[str], dict[str, str]]:
    """Run each unique suite once. Returns (errors, id→status)."""
    needed: set[str] = set()
    for fid in srs11_matrix.srs_ids():
        needed.update(suites_for(fid))
    results: dict[str, str] = {}
    errors: list[str] = []
    order = [s for s in SUITES if s in needed] + (["static"] if "static" in needed else [])
    for name in order:
        print(f"== suite {name} ==", flush=True)
        if name == "static":
            err = static_check()
            if err:
                results[name] = f"FAIL {err}"
                errors.append(err)
                print("FAIL", err, flush=True)
            else:
                results[name] = "OK"
                print("OK  ", name, flush=True)
            continue
        argv, timeout = SUITES[name]
        extra_env = None
        # Isolated dir so cargo does not replace the live :8080 market-api.exe (Windows os error 5).
        if name == "readpath":
            extra_env = {"CARGO_TARGET_DIR": str(ROOT / "target" / "readpath-test")}
        try:
            code, text = run_cmd(argv, timeout, extra_env)
        except subprocess.TimeoutExpired:
            results[name] = f"FAIL timeout {timeout}s"
            errors.append(f"{name} timeout {timeout}s")
            print("FAIL", name, "timeout", flush=True)
            continue
        if code != 0:
            tail = text[-500:] if text else f"exit {code}"
            results[name] = f"FAIL {tail}"
            errors.append(f"{name}: {tail}")
            print("FAIL", name, tail[:200], flush=True)
        else:
            results[name] = "OK"
            print("OK  ", name, flush=True)
    id_status: dict[str, str] = {}
    for fid in srs11_matrix.srs_ids():
        bad = [s for s in suites_for(fid) if results.get(s, "FAIL missing").startswith("FAIL")]
        if bad:
            id_status[fid] = "FAIL " + ",".join(bad)
            errors.append(f"{fid} via {','.join(bad)}")
        else:
            id_status[fid] = "OK"
    return errors, id_status


def main() -> int:
    map_err = srs11_matrix.validate()
    if map_err:
        for e in map_err:
            print("FAIL map", e)
        return 1
    errors, status = execute()
    ok_n = sum(1 for v in status.values() if v == "OK")
    print(f"IDS {ok_n}/{len(status)} OK")
    for fid, st in status.items():
        if st != "OK":
            print(f"  {fid} {st}")
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
