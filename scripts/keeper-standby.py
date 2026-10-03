#!/usr/bin/env python3
"""Active / standby keeper lease (NFR-09 / NFR-11).

Two keepers may run. Only the process that holds the lease file runs `cpm keeper --once`.
The other waits and takes over if the lease expires (missed heartbeat).

Env:
  KEEPER_LEASE_PATH   default tmp/keeper-lease.json
  KEEPER_LEASE_SECS   default 30
  KEEPER_ID           default hostname:pid
  CPM_BIN             default cargo run -p cli --bin cpm --
  RPC_URL / ER_URL / keypair — passed through to cpm

Usage:
  python scripts/keeper-standby.py --once
  python scripts/keeper-standby.py --interval 10
"""

from __future__ import annotations

import argparse
import json
import os
import socket
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def now() -> int:
    return int(time.time())


def keeper_id() -> str:
    return os.environ.get("KEEPER_ID") or f"{socket.gethostname()}:{os.getpid()}"


def lease_path() -> Path:
    return Path(os.environ.get("KEEPER_LEASE_PATH", ROOT / "tmp" / "keeper-lease.json"))


def lease_secs() -> int:
    return int(os.environ.get("KEEPER_LEASE_SECS", "30"))


def read_lease(path: Path) -> dict | None:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except Exception:
        return None


def write_lease(path: Path, holder: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_suffix(".tmp")
    tmp.write_text(
        json.dumps({"holder": holder, "ts": now(), "expires": now() + lease_secs()}, indent=2),
        encoding="utf-8",
    )
    tmp.replace(path)


def hold_lease(path: Path, me: str) -> bool:
    cur = read_lease(path)
    if cur is None or int(cur.get("expires", 0)) < now() or cur.get("holder") == me:
        write_lease(path, me)
        return True
    return False


def run_keeper_once() -> int:
    bin_prefix = os.environ.get("CPM_BIN")
    if bin_prefix:
        cmd = bin_prefix.split() + ["keeper", "--once"]
    else:
        cmd = ["cargo", "run", "-q", "-p", "cli", "--bin", "cpm", "--", "keeper", "--once"]
    url = os.environ.get("RPC_URL", "http://127.0.0.1:8899")
    er = os.environ.get("ER_URL", "http://127.0.0.1:7799")
    kp = os.environ.get("KEEPER_KEYPAIR")
    cmd += ["--url", url, "--er-url", er]
    if kp:
        cmd += ["--keypair", kp]
    print("keeper-standby run:", " ".join(cmd), flush=True)
    return subprocess.call(cmd, cwd=str(ROOT))


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--once", action="store_true")
    ap.add_argument("--interval", type=int, default=10)
    args = ap.parse_args()
    me = keeper_id()
    path = lease_path()
    print(f"keeper-standby id={me} lease={path} ttl={lease_secs()}s", flush=True)
    while True:
        if hold_lease(path, me):
            write_lease(path, me)  # renew before work
            rc = run_keeper_once()
            write_lease(path, me)  # renew after work
            if args.once:
                return rc
        else:
            cur = read_lease(path) or {}
            print(
                f"standby — lease held by {cur.get('holder')} until {cur.get('expires')}",
                flush=True,
            )
            if args.once:
                return 0
        time.sleep(max(1, args.interval))


if __name__ == "__main__":
    sys.exit(main())
