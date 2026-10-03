#!/usr/bin/env python3
"""FR-DUR-01 live drill: kill one ER, Indexer, and PG primary by PID.

Receipted fills and Vault identity (Circle mint + config) must survive.
Does not --reset ER on restart. Does not pkill -f. Does not restart :3000.
Dump leftover is a FAIL. Missing L1/ER/PG is a FAIL, never a silent SKIP.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import signal
import socket
import subprocess
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
CPM = Path(os.environ.get("CPM", str(ROOT / "target" / "debug" / "cpm.exe" if sys.platform == "win32" else ROOT / "target" / "debug" / "cpm")))
INDEXER = Path(
    os.environ.get(
        "INDEXER_BIN",
        str(ROOT / "target" / "debug" / ("indexer.exe" if sys.platform == "win32" else "indexer")),
    )
)
GW_BIN = Path(
    os.environ.get(
        "GW_BIN",
        str(ROOT / "target" / "debug" / ("trading-gateway.exe" if sys.platform == "win32" else "trading-gateway")),
    )
)
KEYPAIR = Path(os.environ.get("KEYPAIR", str(ROOT / "tmp" / "live-id.json")))
L1 = os.environ.get("URL", "http://127.0.0.1:8899")
ER = os.environ.get("ER_URL", "http://127.0.0.1:7799")
GW = os.environ.get("GW", "http://127.0.0.1:18082")
CIRCLE = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
VAULT_PROGRAM = "VaULt11111111111111111111111111111111111111"
VAULT_CONFIG = "65FUEsqD1sCkmYxCDAt8XFvBk7x9jrVL8HovXQ6nbfk7"
WORKDIR = ROOT / "tmp" / "srs11-dur"
PG_PORT = int(os.environ.get("DRILL_PG_PORT", "5432"))
PG_SERVICE = os.environ.get("DRILL_PG_SERVICE", "postgresql-x64-17")
PG_DATA = Path(os.environ.get("DRILL_PG_DATA", r"E:\Programs\PostgreSQL\17\data"))
PG_URL = os.environ.get("DATABASE_URL", f"postgres://cpm:cpm@127.0.0.1:{PG_PORT}/cpm")
REPLICA = WORKDIR / "journal-replica"
OBJECT = WORKDIR / "journal-object"
RECEIPTS = WORKDIR / "receipts"
FINDINGS: list[str] = []
OWNED: list[subprocess.Popen] = []
ER_RESTARTED: subprocess.Popen | None = None
ER_KILLED = False
PG_STOPPED = False


def log(msg: str) -> None:
    print(msg, flush=True)


def fail(msg: str) -> None:
    FINDINGS.append(msg)
    log(f"FAIL {msg}")


def ok(msg: str) -> None:
    log(f"OK   {msg}")


def rpc(url: str, method: str, params: list) -> dict:
    body = json.dumps({"jsonrpc": "2.0", "id": 1, "method": method, "params": params}).encode()
    req = urllib.request.Request(url, data=body, headers={"Content-Type": "application/json"})
    raw = urllib.request.urlopen(req, timeout=12).read().decode()
    return json.loads(raw)


def pg_bin() -> Path:
    env = os.environ.get("PG_BIN")
    candidates = [
        Path(env) if env else None,
        Path(r"E:\Programs\PostgreSQL\17\bin"),
        Path(r"C:\Program Files\PostgreSQL\17\bin"),
    ]
    for c in candidates:
        if c and (c / "pg_ctl.exe").exists():
            return c
    raise RuntimeError("PostgreSQL 17 pg_ctl not found (postgresql-x64-17)")


def rpc_ok(url: str) -> bool:
    try:
        out = rpc(url, "getHealth", [])
        return "result" in out
    except Exception:
        return False


def account_hash(url: str, pk: str) -> str:
    out = rpc(url, "getAccountInfo", [pk, {"encoding": "base64", "commitment": "confirmed"}])
    val = (out.get("result") or {}).get("value")
    if not val:
        return "missing"
    data = (val.get("data") or ["", ""])[0]
    return hashlib.sha256(data.encode()).hexdigest()


def tcp_open(host: str, port: int, timeout: float = 1.5) -> bool:
    try:
        with socket.create_connection((host, port), timeout=timeout):
            return True
    except OSError:
        return False


def pg_up() -> bool:
    return tcp_open("127.0.0.1", PG_PORT)


def vault_identity() -> dict[str, str]:
    return {
        "mint": account_hash(L1, CIRCLE),
        "config": account_hash(L1, VAULT_CONFIG),
    }


def run_cpm(args: list[str], timeout: int = 180) -> subprocess.CompletedProcess[str]:
    cmd = [str(CPM), "--url", L1, "--er-url", ER, "--keypair", str(KEYPAIR), *args]
    return subprocess.run(cmd, cwd=str(ROOT), capture_output=True, text=True, timeout=timeout)


def cpm_ok(args: list[str], timeout: int = 180, idempotent: bool = False) -> str:
    p = run_cpm(args, timeout=timeout)
    text = ((p.stdout or "") + (p.stderr or "")).strip()
    if p.returncode != 0:
        if idempotent and any(
            s in text
            for s in ("already in use", "AlreadyInUse", "already initialized", "custom program error: 0x0")
        ):
            return text
        raise RuntimeError(text[-800:] or f"exit {p.returncode}")
    return text


def first_sig(text: str) -> str:
    m = re.search(r"\b([1-9A-HJ-NP-Za-km-z]{80,88})\b", text)
    return m.group(1) if m else ""


def parse_market(text: str) -> str:
    m = re.search(r"market=([1-9A-HJ-NP-Za-km-z]{32,44})", text)
    if not m:
        raise RuntimeError(f"no market in: {text[-400:]}")
    return m.group(1)


def journal_name(market: str) -> str:
    return "".join(c if c.isalnum() else "_" for c in market) + ".jsonl"


def chain(prev: bytes, body: bytes) -> bytes:
    return hashlib.sha256(prev + body).digest()


def journal_append(market: str, owner: str, nonce: int, sig: str) -> bytes:
    name = journal_name(market)
    prev = bytes(32)
    seq = 1
    for line in replay_lines(REPLICA / name):
        prev = bytes.fromhex(line["root"])
        seq = int(line["seq"]) + 1
    rec = {
        "seq": seq,
        "market": market,
        "owner": owner,
        "nonce": nonce,
        "sig": sig,
        "prev_root": prev.hex(),
        "root": "",
    }
    body = f"{seq}|{market}|{owner}|{nonce}|{sig}".encode()
    root = chain(prev, body)
    rec["root"] = root.hex()
    line = json.dumps(rec, separators=(",", ":"))
    for d in (REPLICA, OBJECT):
        d.mkdir(parents=True, exist_ok=True)
        with (d / name).open("a", encoding="utf-8") as f:
            f.write(line + "\n")
            f.flush()
            os.fsync(f.fileno())
    return root


def replay_lines(path: Path) -> list[dict]:
    if not path.exists():
        return []
    rows = []
    prev = bytes(32)
    for raw in path.read_text(encoding="utf-8").splitlines():
        if not raw.strip():
            continue
        rec = json.loads(raw)
        body = f"{rec['seq']}|{rec['market']}|{rec['owner']}|{rec['nonce']}|{rec['sig']}".encode()
        root = chain(prev, body)
        if rec.get("prev_root") != prev.hex() or rec.get("root") != root.hex():
            raise ValueError(f"journal fork at {path} seq={rec.get('seq')}")
        prev = root
        rows.append(rec)
    return rows


def file_sha(path: Path) -> str:
    if not path.exists():
        return "missing"
    return hashlib.sha256(path.read_bytes()).hexdigest()


def wsl(cmd: str, timeout: int = 30) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        ["wsl", "-e", "bash", "-lc", cmd],
        capture_output=True,
        text=True,
        timeout=timeout,
    )


def win_pids_on_port(port: int) -> list[int]:
    p = subprocess.run(
        ["netstat", "-ano"],
        capture_output=True,
        text=True,
        timeout=20,
    )
    pids: list[int] = []
    for line in (p.stdout or "").splitlines():
        if f":{port}" not in line or "LISTENING" not in line.upper():
            continue
        parts = line.split()
        if not parts:
            continue
        try:
            pid = int(parts[-1])
        except ValueError:
            continue
        if pid > 0 and pid not in pids:
            pids.append(pid)
    return pids


def wsl_pids_on_port(port: int) -> list[int]:
    p = wsl(f"ss -lntp 2>/dev/null | grep ':{port}' || true")
    text = p.stdout or ""
    pids = [int(x) for x in re.findall(r"pid=(\d+)", text)]
    if not pids:
        q = wsl(
            "ps -eo pid,args | awk '/ephemeral-validator/ && !/awk/ {print $1}'"
        )
        pids = [int(x) for x in (q.stdout or "").split() if x.isdigit()]
    out: list[int] = []
    for pid in pids:
        if pid > 1 and pid not in out:
            out.append(pid)
    return out


def kill_pid(where: str, pid: int) -> None:
    log(f"kill {where} pid={pid}")
    if where == "wsl":
        wsl(f"kill -TERM {pid} || true")
        time.sleep(1.5)
        wsl(f"kill -KILL {pid} 2>/dev/null || true")
        return
    if sys.platform == "win32":
        subprocess.run(["taskkill", "/PID", str(pid), "/F"], capture_output=True, text=True)
        return
    try:
        os.kill(pid, signal.SIGTERM)
    except OSError:
        pass
    time.sleep(1)
    try:
        os.kill(pid, signal.SIGKILL)
    except OSError:
        pass


def pg_ctl(*args: str, timeout: int = 60) -> subprocess.CompletedProcess[str]:
    exe = str(pg_bin() / "pg_ctl.exe")
    return subprocess.run(
        [exe, *args],
        capture_output=True,
        text=True,
        timeout=timeout,
    )


def psql_c(sql: str, db: str = "cpm") -> subprocess.CompletedProcess[str]:
    env = os.environ.copy()
    env["PGPASSWORD"] = "cpm"
    return subprocess.run(
        [
            str(pg_bin() / "psql.exe"),
            "-w",
            "-h",
            "127.0.0.1",
            "-p",
            str(PG_PORT),
            "-U",
            "cpm",
            "-d",
            db,
            "-tAc",
            sql,
        ],
        capture_output=True,
        text=True,
        timeout=15,
        env=env,
    )


def ensure_pg_primary() -> None:
    """Machine PostgreSQL 17 (`postgresql-x64-17` on :5432, db `cpm`)."""
    log(f"PG binaries {pg_bin()} url={PG_URL}")
    if not pg_up():
        start_pg()
    if not wait_port("127.0.0.1", PG_PORT, 20):
        raise RuntimeError(f"machine PostgreSQL not on :{PG_PORT} ({PG_SERVICE})")
    chk = psql_c("SELECT current_database()")
    if chk.returncode != 0 or "cpm" not in (chk.stdout or ""):
        raise RuntimeError(f"psql cpm@:{PG_PORT} {(chk.stderr or chk.stdout)[-300:]}")


def stop_pg() -> None:
    """Kill the machine primary by PID. Service may respawn — PID must change or port drop."""
    global PG_STOPPED
    before = win_pids_on_port(PG_PORT)
    if not before:
        raise RuntimeError(f"no PG primary PID on :{PG_PORT}")
    for pid in before:
        r = subprocess.run(
            ["taskkill", "/PID", str(pid), "/F"],
            capture_output=True,
            text=True,
            timeout=20,
        )
        log(f"kill win pid={pid} exit={r.returncode} {(r.stderr or r.stdout or '').strip()[:160]}")
        if r.returncode != 0:
            ns = subprocess.run(["net", "stop", PG_SERVICE], capture_output=True, text=True, timeout=60)
            log(f"net stop {PG_SERVICE} exit={ns.returncode} {(ns.stderr or ns.stdout or '')[-160:]}")
    PG_STOPPED = True
    dead = False
    new_pids: list[int] = before
    for _ in range(20):
        time.sleep(0.3)
        if not pg_up():
            dead = True
            break
        new_pids = win_pids_on_port(PG_PORT)
        if new_pids and set(new_pids) != set(before):
            log(f"PG primary PID {before} → {new_pids} (service respawn after kill)")
            PG_STOPPED = False
            return
    if dead:
        return
    if set(new_pids) == set(before):
        raise RuntimeError(
            f"machine PG PID {before} still listening on :{PG_PORT} — "
            f"need admin to stop {PG_SERVICE} (last net stop was access-denied)"
        )


def start_pg() -> None:
    global PG_STOPPED
    if pg_up():
        PG_STOPPED = False
        return
    log(f"start PG service {PG_SERVICE}")
    r = subprocess.run(["net", "start", PG_SERVICE], capture_output=True, text=True, timeout=60)
    if r.returncode != 0:
        log((r.stderr or r.stdout or "").strip()[-200:])
    if not wait_port("127.0.0.1", PG_PORT, 30):
        raise RuntimeError(f"{PG_SERVICE} did not listen on :{PG_PORT} after start")
    PG_STOPPED = False


def wait_port(host: str, port: int, seconds: float) -> bool:
    deadline = time.time() + seconds
    while time.time() < deadline:
        if tcp_open(host, port):
            return True
        time.sleep(0.4)
    return False


def start_owned(bin_path: Path, env: dict[str, str]) -> subprocess.Popen:
    log_path = WORKDIR / f"{bin_path.stem}.log"
    log_path.parent.mkdir(parents=True, exist_ok=True)
    fh = log_path.open("ab")
    proc = subprocess.Popen(
        [str(bin_path)],
        cwd=str(ROOT),
        env=env,
        stdout=fh,
        stderr=fh,
    )
    OWNED.append(proc)
    return proc


def restart_er() -> subprocess.Popen:
    global ER_RESTARTED
    root_wsl = "/mnt/" + ROOT.drive[0].lower() + ROOT.as_posix()[2:] if ROOT.drive else ROOT.as_posix()
    cmd = (
        "export PATH=$HOME/.local/node_modules/.bin:/usr/bin:/bin:$HOME/.cargo/bin; "
        "export ER_RESET=0; "
        f"bash {root_wsl}/scripts/start-ephemeral-validator.sh"
    )
    log("restart ER without --reset")
    proc = subprocess.Popen(
        ["wsl", "-e", "bash", "-lc", cmd],
        cwd=str(ROOT),
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    ER_RESTARTED = proc
    return proc


def gw_health() -> bool:
    try:
        raw = urllib.request.urlopen(GW.rstrip("/") + "/v1/health", timeout=4).read().decode()
        return '"ok":true' in raw.replace(" ", "")
    except Exception:
        return False


def ensure_binaries() -> None:
    if not CPM.exists():
        raise RuntimeError(f"missing {CPM}")
    need = []
    if not INDEXER.exists():
        need.extend(["-p", "readpath", "--bin", "indexer"])
    if not GW_BIN.exists():
        need.extend(["-p", "gateway", "--bin", "trading-gateway"])
    if need:
        log(f"cargo build {' '.join(need)}")
        r = subprocess.run(["cargo", "build", *need], cwd=str(ROOT), timeout=600)
        if r.returncode != 0:
            raise RuntimeError("cargo build indexer/gateway failed")


def owner_pubkey() -> str:
    raw = json.loads(KEYPAIR.read_text(encoding="utf-8"))
    # secret key json array — derive via solana-keygen or cpm output later
    return os.environ.get("OWNER", "HaHjcAoa9wanJvWDugM4bFoBsQXyM5vg9gsSzZFp8qhn")


def cleanup() -> None:
    for proc in OWNED:
        if proc.poll() is None:
            try:
                proc.terminate()
            except OSError:
                pass
    if PG_STOPPED:
        try:
            start_pg()
        except Exception as e:
            log(f"WARN PG restart: {e}")
        for _ in range(40):
            if pg_up():
                break
            time.sleep(0.5)
    if ER_KILLED and not rpc_ok(ER):
        try:
            restart_er()
            wait_port("127.0.0.1", 7799, 90)
        except Exception as e:
            log(f"WARN ER restart: {e}")


def main() -> int:
    WORKDIR.mkdir(parents=True, exist_ok=True)
    if REPLICA.exists():
        shutil.rmtree(REPLICA, ignore_errors=True)
    if OBJECT.exists():
        shutil.rmtree(OBJECT, ignore_errors=True)
    if RECEIPTS.exists():
        shutil.rmtree(RECEIPTS, ignore_errors=True)
    REPLICA.mkdir(parents=True)
    OBJECT.mkdir(parents=True)
    RECEIPTS.mkdir(parents=True)

    try:
        ensure_binaries()
        if not KEYPAIR.exists():
            fail(f"missing keypair {KEYPAIR}")
            return 1
        if not rpc_ok(L1):
            fail(f"L1 down {L1} — start mb-test-validator; do not SKIP")
            return 1
        ok(f"L1 {L1}")
        if not rpc_ok(ER):
            fail(f"ER down {ER} — start ephemeral-validator; do not SKIP")
            return 1
        ok(f"ER {ER}")
        ensure_pg_primary()
        ok(f"PG primary {PG_URL}")

        mint0 = account_hash(L1, CIRCLE)
        if mint0 == "missing":
            fail(f"Circle mint {CIRCLE} missing on L1")
            return 1
        ok(f"Circle mint {mint0[:12]}")

        env = os.environ.copy()
        env.update(
            {
                "RPC_URL": L1,
                "ER_RPC": ER,
                "DATABASE_URL": PG_URL,
                "ALLOW_MEMORY_ONLY": "0",
                "LISTEN": GW.split("://", 1)[-1],
                "RECEIPT_DIR": str(RECEIPTS),
                "JOURNAL_REPLICA_DIR": str(REPLICA),
                "JOURNAL_OBJECT_DIR": str(OBJECT),
            }
        )
        idx = start_owned(INDEXER, env)
        time.sleep(1.2)
        if idx.poll() is not None:
            fail(f"indexer exited {idx.returncode} ({(WORKDIR / 'indexer.log').read_text(encoding='utf-8', errors='replace')[-400:]})")
            return 1
        ok(f"indexer pid={idx.pid}")

        gw = start_owned(GW_BIN, env)
        for _ in range(25):
            if gw_health():
                break
            time.sleep(0.3)
        if not gw_health():
            fail(f"gateway not healthy {GW}")
            return 1
        ok(f"gateway pid={gw.pid} {GW}")

        cpm_ok(["faucet", "50000"], idempotent=True)
        cpm_ok(["vault-init"], timeout=40, idempotent=True)
        cpm_ok(["committee", "init", "--m", "1"], timeout=40, idempotent=True)
        cpm_ok(["deposit", "20000"], idempotent=True)
        ident = vault_identity()
        if ident["mint"] == "missing" or ident["config"] == "missing":
            fail(f"Vault identity missing after vault-init {ident}")
            return 1
        ok(f"vault-identity mint={ident['mint'][:12]} config={ident['config'][:12]}")
        close_ts = int(time.time()) + 900
        stamp = str(int(time.time()))[-6:]
        created = cpm_ok(
            [
                "market",
                "create-bernoulli",
                f"dur-{stamp}",
                "tag",
                "--close-ts",
                str(close_ts),
                "--challenge-secs",
                "8",
            ],
            timeout=180,
        )
        market = parse_market(created)
        ok(f"market {market}")
        try:
            cpm_ok(["market", "delegate", market], timeout=120)
            ok("delegated")
        except Exception as e:
            fail(f"delegate {e}")
            return 1
        buy = cpm_ok(["trade", "buy-set", market, "01", "40"], timeout=180)
        if "dump" in buy.lower():
            fail("buy used dump leftover")
            return 1
        sig = first_sig(buy)
        if not sig:
            fail(f"buy produced no signature: {buy[-300:]}")
            return 1
        owner = owner_pubkey()
        root = journal_append(market, owner, 1, sig)
        # durable receipt file (gateway ACK shape)
        receipt = {
            "status": "pending",
            "sig": sig,
            "market": market,
            "owner": owner,
            "nonce": 1,
        }
        rec_path = RECEIPTS / f"{owner}_{market}_1.json"
        rec_path.write_text(json.dumps(receipt, indent=2), encoding="utf-8")
        ok(f"receipted fill sig={sig[:12]} root={root.hex()[:12]}")

        snap = {
            "ident": ident,
            "replica": file_sha(REPLICA / journal_name(market)),
            "object": file_sha(OBJECT / journal_name(market)),
            "receipt": file_sha(rec_path),
            "rows": len(replay_lines(REPLICA / journal_name(market))),
            "root": root.hex(),
        }
        if snap["replica"] == "missing" or snap["object"] == "missing":
            fail("journal copies missing before kill")
            return 1

        er_pids = wsl_pids_on_port(7799)
        if not er_pids:
            # Windows listener fallback
            er_pids_win = win_pids_on_port(7799)
            if not er_pids_win:
                fail("cannot resolve ER PID on :7799")
                return 1
            er_where = "win"
            er_pids = er_pids_win
        else:
            er_where = "wsl"
        ok(f"ER pids {er_where}={er_pids}")

        # Kill ER, then indexer, then PG primary — by PID only.
        for pid in er_pids:
            kill_pid(er_where, pid)
        time.sleep(1)
        if rpc_ok(ER):
            for pid in win_pids_on_port(7799):
                kill_pid("win", pid)
            time.sleep(1)
        ER_KILLED = True
        if rpc_ok(ER):
            fail("ER still healthy after kill")
        else:
            ok("ER down")

        if idx.poll() is None:
            kill_pid("win" if sys.platform == "win32" else "unix", idx.pid)
        time.sleep(0.5)
        if idx.poll() is None:
            fail(f"indexer pid={idx.pid} still running")
        else:
            ok("indexer down")

        stop_pg()
        time.sleep(1.2)
        if pg_up():
            fail(f"PG primary still listening on :{PG_PORT} after stop")
        else:
            ok("PG primary down")

        mid = vault_identity()
        if mid != snap["ident"]:
            fail(f"Vault identity changed while crashed: {mid} vs {snap['ident']}")
        else:
            ok("Vault identity unchanged while ER/indexer/PG dead")
        if file_sha(REPLICA / journal_name(market)) != snap["replica"]:
            fail("replica journal mutated after kill")
        elif file_sha(OBJECT / journal_name(market)) != snap["object"]:
            fail("object journal mutated after kill")
        else:
            ok("dual journal copies unchanged")
        if file_sha(rec_path) != snap["receipt"]:
            fail("receipt file mutated after kill")
        else:
            ok("receipt file unchanged")
        rows, replayed_root = replay_lines(REPLICA / journal_name(market)), bytes.fromhex(snap["root"])
        obj_rows = replay_lines(OBJECT / journal_name(market))
        if len(rows) != snap["rows"] or rows[-1]["sig"] != sig:
            fail("replica replay lost the receipted fill")
        elif len(obj_rows) != snap["rows"] or obj_rows[-1]["sig"] != sig:
            fail("object-store replay lost the receipted fill")
        elif bytes.fromhex(rows[-1]["root"]) != replayed_root:
            fail("trades_root drifted")
        else:
            ok(f"θ reconstructible: {len(rows)} fill(s) root={snap['root'][:12]}")

        log("restart PG primary")
        start_pg()
        if not wait_port("127.0.0.1", PG_PORT, 30):
            fail(f"PG did not come back on :{PG_PORT}")
        else:
            ok("PG restarted")

        idx2 = start_owned(INDEXER, env)
        time.sleep(1.5)
        if idx2.poll() is not None:
            fail("indexer failed to restart (PG is not the ledger; process must come back)")
        else:
            ok(f"indexer restarted pid={idx2.pid}")

        restart_er()
        if not wait_port("127.0.0.1", 7799, 90):
            fail("ER did not listen on :7799 after no-reset restart")
        elif not rpc_ok(ER):
            fail("ER restarted but getHealth failed")
        else:
            ok("ER restarted without --reset")

        after = vault_identity()
        if after != snap["ident"]:
            fail(f"Vault identity changed after restart: {after} vs {snap['ident']}")
        else:
            ok("Vault identity unchanged after restart")
        if replay_lines(REPLICA / journal_name(market))[-1]["sig"] != sig:
            fail("fill missing after restart")
        else:
            ok("receipted fill persisted across ER + indexer + PG kill")

    except Exception as e:
        fail(str(e)[:800])
    finally:
        cleanup()

    log(f"FINDINGS {len(FINDINGS)}")
    for row in FINDINGS:
        log(f"- {row}")
    return 1 if FINDINGS else 0


if __name__ == "__main__":
    sys.exit(main())
