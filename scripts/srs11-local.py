#!/usr/bin/env python3
"""Local SRS §11 runner. SKIP must print a reason. Never silent-skip."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "scripts"))
import srs11_matrix  # noqa: E402

FINDINGS: list[str] = []


def fail(msg: str) -> None:
    FINDINGS.append(msg)
    print("FAIL", msg)


def ok(msg: str) -> None:
    print("OK  ", msg)


def skip(msg: str) -> None:
    print("SKIP", msg)


def cargo(*args: str, timeout: int = 300) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["cargo", *args],
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=timeout,
    )


def npm_sdk() -> subprocess.CompletedProcess[str]:
    npm = "npm.cmd" if sys.platform == "win32" else "npm"
    return subprocess.run(
        [npm, "test", "--prefix", str(ROOT / "packages" / "sdk")],
        cwd=ROOT,
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        timeout=120,
    )


def section_2_inv() -> None:
    r = cargo("test", "-p", "math", "--", "--test-threads=1")
    if r.returncode != 0:
        fail(f"INV math {(r.stderr or r.stdout)[-400:]}")
        return
    ok("§11.2 cargo test -p math (INV-01–07 fixtures in crates/math)")


def section_3_dur() -> None:
    r = cargo("test", "-p", "journal", "--", "kill_replica")
    if r.returncode != 0:
        fail(f"FR-DUR journal {(r.stderr or r.stdout)[-400:]}")
        return
    ok("§11.3 journal kill-replica (one copy deleted, root unchanged)")
    # Machine postgresql-x64-17 kill needs admin; deferred by request.


def section_4_set() -> None:
    r = cargo("test", "-p", "settle-flow", "--test", "cpi_market_rho_haircut", timeout=600)
    if r.returncode != 0:
        fail(f"FR-SET-03 {(r.stderr or r.stdout)[-500:]}")
        return
    ok("§11.4 settle-flow two winners same ρ, L>C_max")


def section_5_res() -> None:
    r = cargo("test", "-p", "resolution", "--", "only_submit_result_starts_x")
    if r.returncode != 0:
        fail(f"FR-RES-01 {(r.stderr or r.stdout)[-400:]}")
        return
    ok("§11.5 only submit_result starts x*")


def section_6_usdc() -> None:
    r = cargo("test", "-p", "vault", "--", "usdc_mint_is_circle_mainnet_by_default")
    if r.returncode != 0:
        fail(f"FR-WAL-03 {(r.stderr or r.stdout)[-400:]}")
        return
    ok("§11.6 vault USDC mint is Circle mainnet by default")


def section_7_client() -> None:
    apps = ROOT / "apps"
    banned = list(apps.rglob("*.apk")) + list(apps.rglob("*.ipa")) + list(apps.rglob("*.aab"))
    if banned:
        fail(f"store binaries in apps: {banned}")
        return
    if not (apps / "android-twa" / "twa-manifest.json").exists():
        fail("missing official TWA manifest")
        return
    ok("§11.7 Next.js client only; TWA wrapper, no store binaries")


def section_8_gateway() -> None:
    r = cargo("test", "-p", "gateway")
    if r.returncode != 0:
        fail(f"FR-TRD-09 {(r.stderr or r.stdout)[-400:]}")
        return
    ok("§11.8 gateway pending receipt survives reopen; no keys in files")


def notify_and_env() -> None:
    r = cargo("test", "-p", "notify")
    if r.returncode != 0:
        fail(f"notify {(r.stderr or r.stdout)[-400:]}")
        return
    ok("inbox events are {ts,kind,market}")
    s = npm_sdk()
    if s.returncode != 0:
        fail(f"sdk notify {(s.stderr or s.stdout)[-400:]}")
        return
    ok("sdk notifyKeysOnly rejects extra identity fields")
    for name in (".env.staging.example", ".env.production.example"):
        text = (ROOT / name).read_text(encoding="utf-8")
        if "CPM_ENV=" not in text:
            fail(f"{name} missing CPM_ENV")
            return
        if "ALLOW_MEMORY_ONLY=1" in text and "# ALLOW_MEMORY_ONLY" not in text:
            fail(f"{name} enables ALLOW_MEMORY_ONLY")
            return
    ok("staging/production env examples isolate CPM_ENV")


def section_1_fr_matrix() -> None:
    import srs11_run_matrix  # noqa: WPS433

    errors = srs11_matrix.validate()
    ids = srs11_matrix.srs_ids()
    if errors:
        for e in errors:
            fail(f"§11.1 {e}")
        return
    ok(f"§11.1 map {len(ids)} FR-*/CR-* → {srs11_matrix.MATRIX_JSON.relative_to(ROOT)}")
    run_err, status = srs11_run_matrix.execute()
    ok_n = sum(1 for v in status.values() if v == "OK")
    if run_err:
        for e in run_err[:40]:
            fail(f"§11.1 run {e}")
        fail(f"§11.1 executed {ok_n}/{len(status)} ids")
        return
    ok(f"§11.1 executed {ok_n}/{len(status)} FR-*/CR-* suites")


def main() -> int:
    section_1_fr_matrix()
    section_2_inv()
    section_3_dur()
    section_4_set()
    section_5_res()
    section_6_usdc()
    section_7_client()
    section_8_gateway()
    notify_and_env()
    print("FINDINGS", len(FINDINGS))
    for row in FINDINGS:
        print("-", row)
    return 1 if FINDINGS else 0


if __name__ == "__main__":
    sys.exit(main())
