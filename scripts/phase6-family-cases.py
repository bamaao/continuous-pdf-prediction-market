#!/usr/bin/env python3
"""Asserted cases: prior/compose params, then each family create→buy→x*→ρ→payout."""

from __future__ import annotations

import importlib.util
import json
import os
import sys
import time
import urllib.request
from pathlib import Path

from solders.pubkey import Pubkey

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("flow", ROOT / "scripts" / "phase6-flow-playwright.py")
flow = importlib.util.module_from_spec(spec)
assert spec.loader
spec.loader.exec_module(flow)

MARKET_ID = Pubkey.from_string("Market1111111111111111111111111111111111111")
MASK64 = (1 << 64) - 1
OUT = ROOT / "tmp" / "phase6-cases"
CLOSE_IN = 16
CHALLENGE = 8
REPORT = 90

FINDINGS: list[str] = []
RESULTS: list[dict] = []
if __name__ in sys.modules:
    sys.modules["phase6_family_cases"] = sys.modules[__name__]


def rotl(x: int, n: int) -> int:
    return ((x << n) | (x >> (64 - n))) & MASK64


def digest(parts: list[bytes]) -> bytes:
    st = [0x736F6D6570736575, 0x646F72616E646F6D, 0x6C7967656E657261, 0x7465646279746573]
    for n, part in enumerate(parts):
        st[0] ^= (len(part) + (n << 32)) & MASK64
        for off in range(0, len(part), 8):
            chunk = part[off : off + 8]
            x = 0
            for i, b in enumerate(chunk):
                x |= b << (8 * i)
            st[0] = rotl((st[0] + x) & MASK64, 13)
            st[1] = (st[1] ^ st[0]) & MASK64
            st[2] = rotl((st[2] + st[1]) & MASK64, 17)
            st[3] = (st[3] ^ st[2]) & MASK64
            st[0] = (st[0] * 0x9E3779B97F4A7C15) & MASK64
    out = bytearray(32)
    for i, w in enumerate(st):
        out[i * 8 : i * 8 + 8] = w.to_bytes(8, "little")
    return bytes(out)


def pad32(s: str) -> bytes:
    raw = s.encode()[:32]
    return raw + b"\x00" * (32 - len(raw))


def listing_id_hash(
    family: int,
    topic: str,
    tag: str = "default",
    layout: int = 0,
    top_n: int = 0,
    bins: int = 0,
) -> bytes:
    t, g = pad32(topic), pad32(tag)
    if family == 0:
        return digest([b"sk", t, bytes([0])])
    if family == 3:
        return digest([b"di", t, bytes([layout]), bytes([top_n]), int(bins).to_bytes(2, "little")])
    if family == 4:
        return digest([b"be", t, g])
    return digest([b"iv", bytes([family]), t, g])


def market_pda(family: int, topic: str, tag: str, layout: int = 0, top_n: int = 0, bins: int = 0) -> str:
    h = listing_id_hash(family, topic, tag, layout, top_n, bins)
    return str(Pubkey.find_program_address([b"market", h], MARKET_ID)[0])


def mask_hex(cells: list[int], n: int) -> str:
    buf = bytearray((n + 7) // 8)
    for c in cells:
        buf[c // 8] |= 1 << (c % 8)
    return bytes(buf).hex()


def home_cells() -> list[int]:
    return [i * 11 + j for i in range(11) for j in range(11) if i > j]


def fail(case: str, msg: str) -> None:
    FINDINGS.append(f"{case}: {msg}")
    flow.out(f"FAIL {case}: {msg}")


def ok(case: str, msg: str) -> None:
    flow.out(f"OK   {case}: {msg}")


def get(path: str, timeout=30):
    return flow.http_json("GET", f"{flow.API}{path}", timeout=timeout)


def wait_json(path: str, pred, timeout=25.0, interval=0.4):
    deadline = time.time() + timeout
    last = (0, {})
    while time.time() < deadline:
        last = get(path)
        if last[0] == 200 and pred(last[1]):
            return last[1]
        time.sleep(interval)
    raise TimeoutError(f"{path} last={last[0]} {str(last[1])[:240]}")


def grid_space(n: int) -> int:
    return 75 + 64 * n


def grid_grow_steps(n: int) -> int:
    need = grid_space(n)
    if need <= 10_240:
        return 0
    return (need - 10_240 + 10_239) // 10_240


def grid_mass_steps(n: int) -> int:
    return (n + 255) // 256


def tx_cu(sig: str) -> int | None:
    try:
        req = urllib.request.Request(
            flow.RPC,
            data=json.dumps(
                {
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "getTransaction",
                    "params": [
                        sig,
                        {
                            "encoding": "json",
                            "commitment": "confirmed",
                            "maxSupportedTransactionVersion": 0,
                        },
                    ],
                }
            ).encode(),
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(req, timeout=10) as r:
            body = json.loads(r.read())
        cu = ((body.get("result") or {}).get("meta") or {}).get("computeUnitsConsumed")
        return int(cu) if cu is not None else None
    except Exception as e:
        flow.out(f"CU fetch {e}")
        return None


def send_cu(kp, op: str, extra=None, **kw) -> str:
    sig = send(kp, op, extra=extra, **kw)
    cu = tx_cu(sig)
    flow.out(f"CU {op} {cu if cu is not None else 'unknown'}")
    return sig


def shard_count(n: int) -> int:
    cells = 16
    return (n + cells - 1) // cells


def finish_grid(kp, market: str, n: int) -> None:
    owner = str(kp.pubkey())
    grow = grid_grow_steps(n)
    count = shard_count(n)
    flow.out(f"finish_grid n={n} grow={grow} shards={count}")
    for _ in range(grow):
        send_cu(kp, "grow_grid", owner=owner, market=market)
    for ix in range(1, count):
        try:
            send(kp, "create_grid_shard", owner=owner, market=market, ix=ix)
        except Exception as e:
            if "already in use" not in str(e).lower() and "0x0" not in str(e):
                raise
        send(kp, "write_grid_shard", owner=owner, market=market, ix=ix)
    send_cu(kp, "write_grid_mass", owner=owner, market=market)
    extra = count - 1
    if extra > 16:
        ixs = list(range(1, count))
        for i in range(0, len(ixs), 16):
            send(kp, "accum_seal", owner=owner, market=market, extras=ixs[i : i + 16])
        for i in range(0, len(ixs), 16):
            send(kp, "apply_seal", owner=owner, market=market, extras=ixs[i : i + 16])
    else:
        send_cu(kp, "seal_grid", owner=owner, market=market, n=n)


def record_application(kp, spec: dict, topic: str, tag: str, close_ts: int, title: str, event: str) -> int | None:
    owner = str(kp.pubkey())
    st, app = flow.http_json(
        "POST",
        f"{flow.API}/v1/listings/applications",
        {
            "applicant": owner,
            "family": spec["family"],
            "title": title,
            "tags": [spec.get("category") or "test"],
            "event": event,
            "description": spec.get("id") or event or title,
            "topic": topic,
            "tag": tag,
            "compose": create_body(spec, owner, topic, tag, close_ts),
        },
    )
    if st not in (200, 201):
        fail(spec["id"], f"application {st} {app}")
        return None
    app_id = int(app.get("id") or 0)
    st, rev = flow.http_json(
        "POST",
        f"{flow.API}/v1/review",
        {"id": app_id, "reviewer": owner, "action": "approve", "reason": "e2e"},
    )
    if st != 200:
        fail(spec["id"], f"approve {st} {rev}")
        return None
    return app_id


def mark_opened(kp, spec: dict, app_id: int | None, market: str) -> None:
    if not app_id:
        return
    st, body = flow.http_json(
        "POST",
        f"{flow.API}/v1/review",
        {"id": app_id, "reviewer": str(kp.pubkey()), "action": "opened", "market": market},
    )
    if st != 200:
        fail(spec["id"], f"opened {st} {body}")


def send(kp, op: str, extra=None, **kw) -> str:
    return flow.send_ixs(
        flow.Client(flow.RPC, commitment=flow.Confirmed),
        kp,
        [flow.compose(op, **kw)],
        extra=extra,
    )


def expect_eq(case: str, got, want, label: str) -> bool:
    if got != want:
        fail(case, f"{label} got={got} want={want}")
        return False
    return True


def expect_near(case: str, got: float, want: float, tol: float, label: str) -> bool:
    if abs(got - want) > tol:
        fail(case, f"{label} got={got} want={want}±{tol}")
        return False
    return True


def expect_range(case: str, got: float, lo: float, hi: float, label: str) -> bool:
    if not (lo <= got <= hi):
        fail(case, f"{label} got={got} not in [{lo},{hi}]")
        return False
    return True


def h_hat(liability: int, attachment: int, filled: int) -> int:
    return max(0, min(max(liability - attachment, 0), filled))


def prem_due(premium: int, capacity: int, take: int) -> int:
    if capacity <= 0:
        return 0
    return (premium * take) // capacity


def vault_of(owner: str) -> dict:
    st, body = get(f"/v1/owners/{owner}/vault")
    return body if st == 200 else {}


def layers_of(market: str) -> dict:
    st, body = get(f"/v1/markets/{market}/layers")
    return body if st == 200 else {}


def quote_of(book: dict, lp: str, layer: int = 1) -> dict:
    for q in book.get("quotes") or []:
        if q.get("lp") == lp and int(q.get("layer") or 0) == layer:
            return q
    return {}


def resolve_to_settle(kp, owner: str, market: str, family: int, kind: int, value: int, value_b: int = 0, milli: bool = False, include_pool: bool = False, challenge_wait: int | None = None) -> dict:
    send(kp, "resolve_open", owner=owner, market=market)
    wait_json(f"/v1/markets/{market}/resolution", lambda b: b.get("phase") == 0)
    send(
        kp,
        "submit_result",
        owner=owner,
        market=market,
        family=family,
        kind=kind,
        value=value,
        value_b=value_b,
        **({"milli": True} if milli else {}),
    )
    wait_json(f"/v1/markets/{market}/resolution", lambda b: b.get("phase") == 1)
    time.sleep((CHALLENGE if challenge_wait is None else challenge_wait) + 1)
    send(kp, "finalize", owner=owner, market=market)
    wait_json(f"/v1/markets/{market}/resolution", lambda b: b.get("phase") == 3)
    send(
        kp,
        "begin_settle",
        owner=owner,
        market=market,
        include_pool=include_pool,
        family=family,
        kind=kind,
        value=value,
        value_b=value_b,
        n=121 if family == 0 else None,
        **({"milli": True} if milli else {}),
    )
    return wait_json(f"/v1/markets/{market}/info", lambda b: int(b.get("board_phase") or 0) >= 1)


# --- offline / API-only cases -------------------------------------------------

PRIOR_CASES = [
    {
        "id": "prior-skellam-1x2",
        "q": "family=0&milli=true&lambda_home=1400&lambda_away=1100",
        "n": 121,
        "units": "goals",
        "lines": True,
        "peak_in": None,
    },
    {
        "id": "prior-gaussian-cpi",
        "q": "family=1&milli=true&n=32&x_min=-2000&x_max=12000&mu=2400&sigma=350",
        "n": 32,
        "units": "percentage points",
        "peak_x": (1.5, 3.5),
        "mu": 2.4,
    },
    {
        "id": "prior-gaussian-n8-warn",
        "q": "family=1&milli=true&n=8&x_min=-2000&x_max=12000&mu=2400&sigma=350",
        "n": 8,
        "units": "percentage points",
        "warn": "CPI n_grid",
        "peak_x": (0.0, 4.5),
    },
    {
        "id": "prior-lognormal-btc",
        "q": "family=2&milli=true&n=8&x_min=10000000&x_max=250000000&mu=11080&sigma=250",
        "n": 8,
        "units": "price units",
        "omega": (10000.0, 250000.0),
        "mu": 11.08,
    },
    {
        "id": "prior-dirichlet-uniform",
        "q": "family=3&n=4",
        "n": 4,
        "units": "atoms",
        "uniform": True,
    },
    {
        "id": "prior-bernoulli-5050",
        "q": "family=4",
        "n": 2,
        "units": "atoms",
        "uniform": True,
    },
]


def run_prior_cases() -> None:
    st, _ = get("/v1/prior?family=2&milli=false&n=8&x_min=0&x_max=10&mu=1&sigma=1")
    if st != 400:
        fail("prior-lognormal-xmin-reject", f"status {st} want 400")
    else:
        ok("prior-lognormal-xmin-reject", "400")

    st, _ = get("/v1/prior?family=1&milli=true&n=8&x_min=-2000&x_max=12000&mu=2400&sigma=0")
    if st != 400:
        fail("prior-gaussian-sigma0-reject", f"status {st} want 400")
    else:
        ok("prior-gaussian-sigma0-reject", "400")

    for spec in PRIOR_CASES:
        st, body = get(f"/v1/prior?{spec['q']}")
        if st != 200:
            fail(spec["id"], f"HTTP {st} {body}")
            continue
        expect_eq(spec["id"], body.get("n"), spec["n"], "n")
        expect_eq(spec["id"], body.get("units"), spec["units"], "units")
        cells = body.get("cells") or []
        if len(cells) != spec["n"]:
            fail(spec["id"], f"cells {len(cells)} want {spec['n']}")
        mass = sum(int(c.get("p_bps") or 0) for c in cells)
        expect_range(spec["id"], mass, 9900, 10100, "P0 mass bps")
        if spec.get("peak_x"):
            expect_range(spec["id"], float(body["peak_x"]), spec["peak_x"][0], spec["peak_x"][1], "peak_x")
        if spec.get("mu") is not None:
            expect_near(spec["id"], float(body["mu"]), spec["mu"], 0.02, "mu")
        if spec.get("omega"):
            expect_near(spec["id"], float(body["omega"]["x_min"]), spec["omega"][0], 1e-6, "x_min")
            expect_near(spec["id"], float(body["omega"]["x_max"]), spec["omega"][1], 1e-6, "x_max")
        if spec.get("uniform"):
            bps = [int(c["p_bps"]) for c in cells]
            if max(bps) - min(bps) > 20:
                fail(spec["id"], f"not uniform {bps}")
        if spec.get("lines"):
            lines = {r["label"]: int(r["p_bps"]) for r in body.get("lines") or []}
            if set(lines) < {"Home", "Draw", "Away"}:
                fail(spec["id"], f"missing 1X2 {lines}")
            else:
                expect_range(spec["id"], lines["Home"] + lines["Draw"] + lines["Away"], 9900, 10100, "1X2 sum")
                if not (lines["Home"] > lines["Draw"] and lines["Home"] > lines["Away"]):
                    fail(spec["id"], f"λH>λA should favor Home {lines}")
        if spec.get("warn") and not any(spec["warn"] in w for w in body.get("warnings") or []):
            fail(spec["id"], f"missing warning {spec['warn']}: {body.get('warnings')}")
        if spec["id"] not in " ".join(FINDINGS):
            ok(spec["id"], f"n={body.get('n')} peak={body.get('peak_x')} mass={mass}")
        RESULTS.append({"id": spec["id"], "kind": "prior", "body": {k: body.get(k) for k in ("n", "peak", "peak_x", "mu", "units")}})


def run_compose_milli_case() -> None:
    owner = "HaHjcAoa9wanJvWDugM4bFoBsQXyM5vg9gsSzZFp8qhn"
    common = {
        "op": "create_gaussian",
        "owner": owner,
        "topic": "milli-check",
        "tag": "x",
        "n": 8,
        "close_ts": int(time.time()) + 80,
    }
    a = flow.http_json("POST", f"{flow.API}/v1/compose", {**common, "milli": False, "mu": 2, "sigma": 1, "x_min": 0, "x_max": 8})
    b = flow.http_json("POST", f"{flow.API}/v1/compose", {**common, "milli": True, "mu": 2400, "sigma": 350, "x_min": -2000, "x_max": 12000})
    if a[0] != 200 or b[0] != 200:
        fail("compose-milli-vs-int", f"{a[0]} {b[0]}")
        return
    if a[1].get("data_b64") == b[1].get("data_b64"):
        fail("compose-milli-vs-int", "milli create bytes equal integer-cell create")
    else:
        ok("compose-milli-vs-int", "data differs")


# --- signed family flows ------------------------------------------------------

FLOW_CASES = [
    {
        "id": "skellam-home-hit",
        "family": 0,
        "op": "create_skellam",
        "topic": "sk",
        "tag": "ft",
        "n": 121,
        "title": "Skellam Home HIT",
        "category": "football",
        "milli": True,
        "lambda_home": 1400,
        "lambda_away": 1100,
        "buy": "skellam",
        "kind": 0,
        "value": 1,
        "value_b": 0,
        "expect_cell": 11,
        "expect_label": "1-0",
        "hit": True,
        "create_extra": {},
    },
    {
        "id": "gaussian-n256-hit",
        "family": 1,
        "op": "create_gaussian",
        "topic": "g2",
        "tag": "cpi",
        "n": 256,
        "title": "Gaussian n=256 HIT",
        "category": "macro",
        "milli": True,
        "x_min": -2000,
        "x_max": 12000,
        "mu": 2400,
        "sigma": 350,
        "buy": "peak",
        "kind": 1,
        "value": 2400,
        "value_b": 0,
        "submit_milli": True,
        "expect_label": "2.400",
        "hit": True,
        "close_in": 90,
        "create_extra": {},
    },
    {
        "id": "gaussian-n1024-hit",
        "family": 1,
        "op": "create_gaussian",
        "topic": "g4",
        "tag": "cpi",
        "n": 1024,
        "title": "Gaussian n=1024 HIT",
        "category": "macro",
        "milli": True,
        "x_min": -2000,
        "x_max": 12000,
        "mu": 2400,
        "sigma": 350,
        "buy": "peak",
        "kind": 1,
        "value": 2400,
        "value_b": 0,
        "submit_milli": True,
        "expect_label": "2.400",
        "hit": True,
        "close_in": 120,
        "create_extra": {},
    },
    {
        "id": "gaussian-mu-hit",
        "family": 1,
        "op": "create_gaussian",
        "topic": "ga",
        "tag": "yoy",
        "n": 32,
        "title": "Gaussian μ HIT",
        "category": "macro",
        "milli": True,
        "x_min": -2000,
        "x_max": 12000,
        "mu": 2400,
        "sigma": 350,
        "buy": "peak",
        "kind": 1,
        "value": 2400,
        "value_b": 0,
        "submit_milli": True,
        "expect_label": "2.400",
        "hit": True,
        "create_extra": {},
    },
    {
        "id": "gaussian-edge-miss",
        "family": 1,
        "op": "create_gaussian",
        "topic": "gm",
        "tag": "miss",
        "n": 32,
        "title": "Gaussian edge MISS",
        "category": "macro",
        "milli": True,
        "x_min": -2000,
        "x_max": 12000,
        "mu": 2400,
        "sigma": 350,
        "buy": "not_peak",
        "kind": 1,
        "value": 2400,
        "value_b": 0,
        "submit_milli": True,
        "expect_label": "2.400",
        "hit": False,
        "create_extra": {},
    },
    {
        "id": "lognormal-peak-hit",
        "family": 2,
        "op": "create_lognormal",
        "topic": "ln",
        "tag": "spot",
        "n": 8,
        "title": "Lognormal peak HIT",
        "category": "price",
        "milli": True,
        "x_min": 10_000_000,
        "x_max": 250_000_000,
        "mu": 11080,
        "sigma": 250,
        "buy": "peak",
        "kind": 1,
        "value": 64880,
        "value_b": 0,
        "expect_label": "64880",
        "hit": True,
        "create_extra": {},
    },
    {
        "id": "dirichlet-simplex-grow",
        "family": 3,
        "op": "create_dirichlet",
        "topic": "ds",
        "tag": "grow",
        "n": 159,
        "layout": 2,
        "bins": 158,
        "k": 2,
        "title": "Dirichlet simplex grow",
        "category": "election",
        "milli": False,
        "buy": "cells",
        "cells": [0],
        "kind": 2,
        "value": 0,
        "value_b": 0,
        "expect_cell": 0,
        "expect_label": "atom 0",
        "hit": True,
        "close_in": 45,
        "create_extra": {},
    },
    {
        "id": "dirichlet-simplex-k4-grow",
        "family": 3,
        "op": "create_dirichlet",
        "topic": "d4",
        "tag": "grow",
        "n": 286,
        "layout": 2,
        "bins": 10,
        "k": 4,
        "title": "Dirichlet simplex k=4 grow",
        "category": "election",
        "milli": False,
        "buy": "cells",
        "cells": [0],
        "kind": 2,
        "value": 0,
        "value_b": 0,
        "expect_cell": 0,
        "expect_label": "atom 0",
        "hit": True,
        "close_in": 60,
        "create_extra": {},
    },
    {
        "id": "dirichlet-atom0-hit",
        "family": 3,
        "op": "create_dirichlet",
        "topic": "di",
        "tag": "winner",
        "n": 4,
        "title": "Dirichlet atom0 HIT",
        "category": "election",
        "milli": False,
        "buy": "cells",
        "cells": [0],
        "kind": 2,
        "value": 0,
        "value_b": 0,
        "expect_cell": 0,
        "expect_label": "atom 0",
        "hit": True,
        "create_extra": {},
    },
    {
        "id": "bernoulli-yes-hit",
        "family": 4,
        "op": "create_bernoulli",
        "topic": "be",
        "tag": "yes",
        "n": 2,
        "title": "Bernoulli YES HIT",
        "category": "binary",
        "milli": False,
        "buy": "cells",
        "cells": [1],
        "kind": 4,
        "value": 1,
        "value_b": 0,
        "expect_cell": 1,
        "expect_label": "YES",
        "hit": True,
        "create_extra": {},
    },
]


def create_body(spec: dict, owner: str, topic: str, tag: str, close_ts: int) -> dict:
    body = {
        "op": spec["op"],
        "owner": owner,
        "family": spec["family"],
        "topic": topic,
        "tag": tag,
        "n": spec["n"],
        "beta": 100,
        "c_m": 0,
        "close_ts": close_ts,
        "risk_lock_ts": close_ts + spec.get("risk_lock_extra", 0),
        "challenge_secs": spec.get("challenge_secs", CHALLENGE),
        "report_window_secs": spec.get("report_window_secs", REPORT),
        "n_layers": spec.get("n_layers", 1),
        "d_unit": spec.get("d_unit", 10),
    }
    if spec.get("milli"):
        body["milli"] = True
        for k in ("lambda_home", "lambda_away", "x_min", "x_max", "mu", "sigma"):
            if k in spec:
                body[k] = spec[k]
    for k in ("layout", "top_n", "bins", "k"):
        if k in spec:
            body[k] = spec[k]
    return body


def fund_second(main) -> "object":
    from solders.keypair import Keypair

    lp = Keypair()
    rpc = flow.Client(flow.RPC, commitment=flow.Confirmed)
    try:
        sig = rpc.request_airdrop(lp.pubkey(), 2_000_000_000).value
        rpc.confirm_transaction(sig, flow.Confirmed)
    except Exception as e:
        flow.out(f"lp2 airdrop {e}")
    send(lp, "create_ata", owner=str(lp.pubkey()))
    auth = flow.load_kp(flow.AUTH_PATH)
    dest = flow.ata(lp.pubkey())
    flow.send_ixs(rpc, main, [flow.mint_to_ix(auth.pubkey(), dest, 1_000_000_000)], extra=[auth])
    send(lp, "deposit", owner=str(lp.pubkey()), amount=200)
    flow.out(f"lp2 {lp.pubkey()}")
    return lp


def run_auction_book(kp, cid: str, market: str, owner: str, spec: dict) -> None:
    cap = spec.get("quote_capacity", 100)
    prem = spec.get("quote_premium", 100)
    share = spec.get("quote_share_bps", 2_000)
    reserved0 = int(vault_of(owner).get("reserved") or 0)
    try:
        send(kp, "risk_quote", owner=owner, market=market, layer=1, capacity=cap, premium=prem, profit_share_bps=share)
    except Exception as e:
        fail(cid, f"risk_quote {e}")
        return
    try:
        book = wait_json(
            f"/v1/markets/{market}/layers",
            lambda b: int(b.get("c_r") or 0) >= 1
            and any(q.get("lp") == owner and int(q.get("capacity") or 0) == cap for q in b.get("quotes") or []),
        )
    except Exception as e:
        fail(cid, f"layers after quote {e}")
        return
    layers = book.get("layers") or []
    if not layers:
        fail(cid, "no published layers after quote")
        return
    layer = layers[0]
    expect_eq(cid, int(layer.get("id") or 0), 1, "layer id")
    want_a = 0
    att = layer.get("attachment")
    expect_eq(cid, int(att) if att is not None else -1, want_a, "layer attachment A=(k-1)d")
    expect_eq(cid, int(layer.get("thickness") or 0), spec.get("d_unit", 10), "layer thickness T")
    mine = quote_of(book, owner, 1)
    if not mine:
        fail(cid, "standing ladder missing our quote")
        return
    filled = int(mine.get("filled") or 0)
    expect_eq(cid, int(mine.get("capacity") or 0), cap, "quote capacity D")
    expect_eq(cid, int(mine.get("premium") or 0), prem, "quote premium")
    cr = int(book.get("c_r") or 0)
    if filled < 1 or cr < 1:
        fail(cid, f"try_fill at post filled={filled} C_R={cr} want ≥1")
    else:
        ok(cid, f"quote D={cap} try_fill={filled} C_R={cr} A={layer.get('attachment')} owed={mine.get('premium_owed')}")
    reserved1 = int(vault_of(owner).get("reserved") or 0)
    expect_eq(cid, reserved1, reserved0 + cap, "quote locks D into reserved")

    st, catalog = get("/v1/auctions?limit=100")
    if st != 200 or not any(i.get("market") == market for i in catalog.get("items") or []):
        fail(cid, "market missing from /v1/auctions")
    else:
        row = next(i for i in catalog["items"] if i["market"] == market)
        expect_eq(cid, row.get("title"), spec.get("title"), "auction catalog title")
        expect_eq(cid, row.get("category"), spec.get("category"), "auction catalog category")

    st, risk = get(f"/v1/owners/{owner}/risk")
    if st != 200 or not any(i.get("market") == market for i in risk.get("items") or []):
        fail(cid, "owner risk desk missing quoted layer")
    else:
        row = next(i for i in risk["items"] if i["market"] == market)
        expect_eq(cid, int(row.get("layer") or 0), 1, "owner-risk layer")
        expect_eq(cid, int(row.get("capacity") or 0), cap, "owner-risk D")
        expect_eq(cid, int(row.get("filled") or 0), filled, "owner-risk filled")
        expect_eq(cid, int(row.get("premium_owed") or 0), prem_due(prem, cap, filled), "premium_owed=π·take/D")


def fill_or_skip(kp, cid: str, owner: str, market: str, layer: int) -> bool:
    try:
        send(kp, "fill_next", owner=owner, market=market, layer=layer)
        return True
    except Exception as e:
        err = str(e)
        if "NotCheapest" in err or "6008" in err:
            ok(cid, f"fill_next layer={layer} skipped (not cheapest live bid)")
            return False
        if "Locked" in err or "6005" in err:
            ok(cid, f"fill_next layer={layer} skipped (risk locked)")
            return False
        if "AccountNotInitialized" in err or "3012" in err:
            ok(cid, f"fill_next layer={layer} skipped (no next quote)")
            return False
        fail(cid, f"fill_next layer={layer} {e}")
        return False


def run_auction_match_after_trade(kp, cid: str, market: str, owner: str) -> None:
    book0 = layers_of(market)
    cr0 = int(book0.get("c_r") or 0)
    filled0 = int(quote_of(book0, owner, 1).get("filled") or 0)
    try:
        send(kp, "fill_next", owner=owner, market=market, layer=1)
    except Exception as e:
        flow.out(f"{cid} fill_next after trade {e}")
        return
    try:
        book1 = wait_json(f"/v1/markets/{market}/layers", lambda b: int(b.get("c_r") or -1) >= cr0)
    except Exception as e:
        fail(cid, f"layers after fill_next {e}")
        return
    cr1 = int(book1.get("c_r") or 0)
    filled1 = int(quote_of(book1, owner, 1).get("filled") or 0)
    if cr1 < cr0 or filled1 < filled0:
        fail(cid, f"fill_next shrank book C_R {cr0}→{cr1} filled {filled0}→{filled1}")
    else:
        ok(cid, f"fill_next post-trade filled {filled0}→{filled1} C_R {cr0}→{cr1}")


def run_lp_after_settle(kp, cid: str, market: str, owner: str) -> None:
    st, info = get(f"/v1/markets/{market}/info")
    L = int((info or {}).get("liability") or 0) if st == 200 else 0
    st, risk = get(f"/v1/owners/{owner}/risk")
    rows = [i for i in (risk.get("items") or []) if i.get("market") == market and not i.get("cancelled")] if st == 200 else []
    rows.sort(key=lambda r: int(r.get("filled") or 0), reverse=True)
    if not rows:
        fail(cid, "owner risk empty after settle — cannot draw H / premium")
        return
    if int(rows[0].get("board_phase") or 0) != 1:
        fail(cid, f"LP desk board_phase {rows[0].get('board_phase')} want 1 (settled)")
        return
    for row in rows:
        filled = int(row.get("filled") or 0)
        att = int(row.get("attachment") or 0)
        want_h = h_hat(L, att, filled)
        expect_eq(cid, int(row.get("expected_h") or 0), want_h, f"layer {row.get('layer')} Ĥ=min((L-A)+,D_i)")
        expect_near(
            cid,
            int(row.get("premium_owed") or 0),
            prem_due(int(row.get("premium") or 0), int(row.get("capacity") or 0), filled),
            2,
            f"layer {row.get('layer')} premium_owed",
        )
        reserved0 = int(vault_of(owner).get("reserved") or 0)
        avail0 = int(vault_of(owner).get("available") or 0)
        layer = row.get("layer") or 1
        try:
            send(kp, "draw_lp", owner=owner, market=market, layer=layer)
            reserved1 = int(vault_of(owner).get("reserved") or 0)
            if reserved1 > reserved0:
                fail(cid, f"draw_lp layer={layer} reserved rose {reserved0}→{reserved1}")
            ok(cid, f"draw_lp layer={layer} Ĥ={want_h} filled={filled} reserved {reserved0}→{reserved1}")
        except Exception as e:
            fail(cid, f"draw_lp layer={layer} {e}")
            return
        try:
            send(kp, "pay_premium", owner=owner, market=market, layer=layer)
            avail1 = int(vault_of(owner).get("available") or 0)
            owed = int(row.get("premium_owed") or 0)
            if owed > 0 and avail1 < avail0:
                fail(cid, f"pay_premium layer={layer} available fell {avail0}→{avail1} owed={owed}")
            ok(cid, f"pay_premium layer={layer} owed={owed} available {avail0}→{avail1}")
        except Exception as e:
            fail(cid, f"pay_premium layer={layer} {e}")
        w = int(row.get("weight_sum") or 1)
        try:
            send(kp, "pay_surplus_lp", owner=owner, market=market, layer=layer, weight_sum=w)
            ok(cid, f"pay_surplus_lp layer={layer}")
        except Exception as e:
            if "NoSurplus" in str(e) or "600" in str(e):
                ok(cid, f"pay_surplus_lp layer={layer} skipped (ρ<1 or no surplus)")
            else:
                fail(cid, f"pay_surplus_lp layer={layer} {e}")


def prior_query(spec: dict) -> str:
    q = [f"family={spec['family']}", f"n={spec['n']}"]
    if spec.get("milli"):
        q.append("milli=true")
        for k in ("lambda_home", "lambda_away", "x_min", "x_max", "mu", "sigma"):
            if k in spec:
                q.append(f"{k}={spec[k]}")
    return "&".join(q)


def run_flow_case(kp, spec: dict, stamp: str) -> None:
    cid = spec["id"]
    owner = str(kp.pubkey())
    topic = f"{spec['topic']}{stamp}"
    tag = spec["tag"]
    close_ts = int(time.time()) + int(spec.get("close_in") or CLOSE_IN)
    title = f"{spec['title']} {stamp}"
    event = f"{cid} {stamp}"
    spec = {**spec, "title": title}
    market = market_pda(
        spec["family"],
        topic,
        tag,
        spec.get("layout", 0),
        spec.get("top_n", 0),
        spec.get("bins", 0),
    )
    flow.out(f"CASE {cid} market {market} n={spec['n']}")

    app_id = record_application(kp, spec, topic, tag, close_ts, title, event)
    try:
        send(kp, **create_body(spec, owner, topic, tag, close_ts))
        finish_grid(kp, market, spec["n"])
        send(kp, "fund_cm", owner=owner, market=market, amount=0)
        try:
            send(kp, "risk_open_book", owner=owner, market=market)
        except Exception:
            pass
    except Exception as e:
        fail(cid, f"create {e}")
        return
    mark_opened(kp, spec, app_id, market)

    st, _ = flow.http_json(
        "POST",
        f"{flow.API}/v1/listings",
        {
            "market": market,
            "title": title,
            "category": spec["category"],
            "topic": topic,
            "tag": tag,
            "description": cid,
            "event": event,
        },
    )
    if st not in (200, 201):
        fail(cid, f"listing {st}")

    try:
        info = wait_json(
            f"/v1/markets/{market}/info",
            lambda b: b.get("family") == spec["family"] and b.get("n") == spec["n"],
            timeout=90.0 if spec["n"] >= 1024 else 45.0 if spec["n"] >= 256 else 25.0,
        )
    except Exception as e:
        fail(cid, f"info after create {e}")
        return

    expect_eq(cid, info["family"], spec["family"], "family")
    expect_eq(cid, info["n"], spec["n"], "n")
    expect_eq(cid, info.get("title"), title, "listing title")
    expect_eq(cid, info.get("category"), spec["category"], "category")
    expect_near(cid, info.get("close_ts") or 0, close_ts, 3, "close_ts")
    cells = info.get("cells") or []
    if len(cells) != spec["n"]:
        fail(cid, f"indexed cells {len(cells)} want {spec['n']}")
        return
    mass = sum(int(c.get("p_bps") or 0) for c in cells)
    expect_range(cid, mass, 9900, 10100, "indexed P0 mass")

    st, prior = get(f"/v1/prior?{prior_query(spec)}")
    if st != 200:
        fail(cid, f"prior {st}")
        return
    if spec.get("auction", True):
        run_auction_book(kp, cid, market, owner, spec)

    if spec["family"] in (0, 1, 2, 3, 4):
        tol = 80 if spec["n"] >= 256 else 40
        diffs = 0
        for i, c in enumerate(cells):
            pb = int((prior.get("cells") or [{}])[i].get("p_bps") or 0) if i < len(prior.get("cells") or []) else -1
            if abs(int(c["p_bps"]) - pb) > tol:
                diffs += 1
        if diffs > max(1, spec["n"] // 256):
            fail(cid, f"indexed PDF ≠ prior on {diffs} cells (θ should be 0)")
    if spec.get("expect_cell") is None and spec.get("expect_label") and spec["family"] == 1:
        spec["expect_cell"] = int(prior.get("peak") or 0)

    if spec["buy"] == "skellam":
        buy_cells = home_cells()
        mask = mask_hex(buy_cells, 121)
    elif spec["buy"] == "peak":
        peak = int(prior.get("peak") or 0)
        buy_cells = [peak]
        mask = mask_hex(buy_cells, spec["n"])
        spec = {**spec, "expect_cell": peak, "cells": buy_cells}
    elif spec["buy"] == "not_peak":
        peak = int(prior.get("peak") or 0)
        buy_cells = None
        for i, c in enumerate(prior.get("cells") or []):
            pb = int(c.get("p_bps") or 0)
            if i != peak and 80 <= pb <= 4000:
                buy_cells = [i]
                break
        if not buy_cells:
            fail(cid, f"no off-peak cell with mass (peak={peak})")
            return
        mask = mask_hex(buy_cells, spec["n"])
        spec = {**spec, "expect_cell": peak, "cells": buy_cells}
    else:
        buy_cells = spec["cells"]
        mask = mask_hex(buy_cells, spec["n"])

    st, quote0 = get(f"/v1/markets/{market}/quote?mask={mask}&shares=1")
    if st != 200:
        fail(cid, f"quote {st} {quote0}")
        return
    want_ps = sum(int(cells[i]["p_bps"]) for i in buy_cells)
    expect_near(cid, int(quote0["p_s_bps"]), want_ps, 40, "θ=0 p_S vs set mass")
    if int(quote0["p_s_bps"]) <= 0 or int(quote0["p_s_bps"]) >= 10000:
        fail(cid, f"p_S {quote0['p_s_bps']} not a proper probability")

    try:
        if spec["buy"] == "skellam":
            send(kp, "buy_skellam_set", owner=owner, market=market, kind=0, value=0, value_b=0, shares=1, nonce=1)
        else:
            send_cu(kp, "buy_set", owner=owner, market=market, mask=mask, shares=1, nonce=1)
    except Exception as e:
        fail(cid, f"buy {e}")
        return

    try:
        quote1 = wait_json(
            f"/v1/markets/{market}/quote?mask={mask}&shares=1",
            lambda b: int(b.get("p_s_bps") or 0) > int(quote0["p_s_bps"]),
        )
        ok(cid, f"buy raised p_S {quote0['p_s_bps']}→{quote1['p_s_bps']}")
    except Exception:
        fail(cid, f"p_S did not rise after buy (was {quote0['p_s_bps']})")

    if spec.get("auction", True):
        run_auction_match_after_trade(kp, cid, market, owner)

    remain = close_ts - time.time() + 2
    if remain > 0:
        time.sleep(remain)

    try:
        send(kp, "resolve_open", owner=owner, market=market)
        rec = wait_json(f"/v1/markets/{market}/resolution", lambda b: b.get("phase") == 0)
        expect_eq(cid, rec.get("challenge_secs"), CHALLENGE, "challenge_secs")
        expect_eq(cid, rec.get("report_window_secs"), REPORT, "report_window_secs")
        expect_eq(cid, (rec.get("members") or [None])[0], owner, "committee member")
        send(
            kp,
            "submit_result",
            owner=owner,
            market=market,
            family=spec["family"],
            kind=spec["kind"],
            value=spec["value"],
            value_b=spec.get("value_b", 0),
            **({"milli": True} if spec.get("submit_milli") else {}),
        )
        rec = wait_json(f"/v1/markets/{market}/resolution", lambda b: b.get("phase") == 1)
        expect_eq(cid, rec.get("proposed", {}).get("label"), spec["expect_label"], "proposed label")
        time.sleep(CHALLENGE + 1)
        send(kp, "finalize", owner=owner, market=market)
        rec = wait_json(f"/v1/markets/{market}/resolution", lambda b: b.get("phase") == 3)
        expect_eq(cid, rec.get("final_outcome", {}).get("label"), spec["expect_label"], "final x*")
        settle_kw = {
            "family": spec["family"],
            "kind": spec["kind"],
            "value": spec["value"],
            "value_b": spec.get("value_b", 0),
            "n": spec["n"],
        }
        if spec.get("submit_milli") or spec.get("milli"):
            settle_kw["milli"] = True
            if spec.get("x_min") is not None:
                settle_kw["x_min"] = spec["x_min"]
            if spec.get("x_max") is not None:
                settle_kw["x_max"] = spec["x_max"]
        send_cu(kp, "begin_settle", owner=owner, market=market, **settle_kw)
    except Exception as e:
        fail(cid, f"resolve/settle {e}")
        return

    try:
        settled = wait_json(
            f"/v1/markets/{market}/info",
            lambda b: int(b.get("board_phase") or 0) >= 1 and (b.get("final_result") or "") != "",
        )
    except Exception as e:
        fail(cid, f"settled info {e}")
        return

    expect_eq(cid, settled.get("final_result"), spec["expect_label"], "info.final_result")
    if spec.get("expect_cell") is not None:
        expect_eq(cid, int(settled.get("settle_cell")), spec["expect_cell"], "settle_cell")
    c_max = int(settled.get("c_max_usdc") or 0)
    r_net = int(settled.get("r_net") or 0)
    c_m = int(settled.get("c_m") or 0)
    c_r = int(settled.get("c_r") or 0)
    c_p = int(settled.get("c_p_alloc") or 0)
    expect_eq(cid, c_max, r_net + c_r + c_p, "C_max identity")
    L = int(settled.get("liability") or 0)
    rho = int(settled.get("rho_bps") or 0)
    if L <= 0:
        expect_eq(cid, rho, 10000, "ρ when L=0")
    elif c_max >= L:
        expect_eq(cid, rho, 10000, "ρ when C_max≥L")
    else:
        want = 0 if L == 0 else int(c_max * 10000 / L)
        expect_near(cid, rho, want, 50, "ρ=C_max/L")

    try:
        if spec["buy"] == "skellam":
            send(kp, "payout_skellam", owner=owner, market=market, kind=0, value=0, value_b=0)
        else:
            send(kp, "payout", owner=owner, market=market, mask=mask)
    except Exception as e:
        fail(cid, f"payout {e}")
        return

    try:
        book = wait_json(
            f"/v1/owners/{owner}/positions?q={market}",
            lambda b: any(i.get("market") == market and i.get("claimed") for i in b.get("items") or []),
        )
    except Exception as e:
        fail(cid, f"positions {e}")
        return
    ticket = next(i for i in book["items"] if i["market"] == market)
    paid = int(ticket.get("paid_usdc") or 0)
    shares = int(ticket.get("shares") or 0)
    if shares < 1:
        fail(cid, f"shares {shares}")
    face = shares
    want_pay = 0 if not spec["hit"] else (face * rho) // 10000
    expect_eq(cid, paid, want_pay, "payout USDC")
    if spec["hit"] and rho == 10000 and shares >= 1 and paid != shares:
        fail(cid, f"HIT ρ=1 paid {paid} want face {shares}")
    if not spec["hit"] and paid != 0:
        fail(cid, f"MISS ticket paid {paid}")
    prompt = ticket.get("prompt")
    if spec["hit"] and paid > 0 and prompt != "paid":
        fail(cid, f"prompt {prompt} want paid")
    if not spec["hit"] and prompt not in ("claimed_zero", "paid"):
        fail(cid, f"MISS prompt {prompt}")

    if spec.get("auction", True):
        run_lp_after_settle(kp, cid, market, owner)

    if cid not in " ".join(FINDINGS):
        ok(cid, f"cell={settled.get('settle_cell')} ρ={rho} paid={paid} C_max={c_max} L={L} C_R={c_r}")
    RESULTS.append(
        {
            "id": cid,
            "market": market,
            "family": spec["family"],
            "n": spec["n"],
            "p_s_before": quote0.get("p_s_bps"),
            "settle_cell": settled.get("settle_cell"),
            "final": settled.get("final_result"),
            "rho_bps": rho,
            "c_max": c_max,
            "c_r": c_r,
            "liability": L,
            "paid": paid,
            "hit": spec["hit"],
        }
    )


def run_vault_session(kp) -> None:
    owner = str(kp.pubkey())
    st, before = get(f"/v1/owners/{owner}/vault")
    if st != 200 or not before.get("exists"):
        fail("vault-withdraw", f"vault {st} {before}")
        return
    avail = int(before.get("available") or 0)
    if avail < 10:
        fail("vault-withdraw", f"available {avail} < 10")
        return
    try:
        send(kp, "withdraw", owner=owner, amount=10)
    except Exception as e:
        fail("vault-withdraw", str(e))
        return
    try:
        after = wait_json(f"/v1/owners/{owner}/vault", lambda b: int(b.get("available") or -1) == avail - 10)
        expect_eq("vault-withdraw", int(after["available"]), avail - 10, "available")
        ok("vault-withdraw", f"{avail}→{after['available']}")
    except Exception as e:
        fail("vault-withdraw", str(e))
    from solders.keypair import Keypair

    sess = Keypair()
    try:
        send(
            kp,
            "open_session",
            owner=owner,
            authority=str(sess.pubkey()),
            expires_ts=int(time.time()) + 3_600,
            remaining_usdc=500,
        )
        ok("session-open", str(sess.pubkey())[:12])
    except Exception as e:
        if "SessionLive" in str(e) or "6020" in str(e):
            ok("session-open", "already live")
        else:
            fail("session-open", str(e))


def run_two_lp_rank(kp, lp2, stamp: str) -> None:
    cid = "auction-two-lp-rank"
    spec = {
        "id": cid,
        "family": 4,
        "op": "create_bernoulli",
        "topic": "ar",
        "tag": "rnk",
        "n": 2,
        "title": "Auction two-LP rank",
        "category": "binary",
        "c_m": 50,
        "n_layers": 2,
        "d_unit": 10,
        "auction": False,
        "buy": "cells",
        "cells": [1],
        "kind": 4,
        "value": 1,
        "expect_cell": 1,
        "expect_label": "YES",
        "hit": True,
    }
    owner = str(kp.pubkey())
    topic = f"{spec['topic']}{stamp}"
    close_ts = int(time.time()) + CLOSE_IN
    market = market_pda(4, topic, spec["tag"])
    flow.out(f"CASE {cid} market {market}")
    try:
        send(kp, **create_body(spec, owner, topic, spec["tag"], close_ts))
        send(kp, "fund_cm", owner=owner, market=market, amount=0)
        send(kp, "risk_open_book", owner=owner, market=market)
        flow.http_json(
            "POST",
            f"{flow.API}/v1/listings",
            {"market": market, "title": spec["title"], "category": spec["category"], "topic": topic, "tag": spec["tag"], "description": cid, "event": cid},
        )
        wait_json(f"/v1/markets/{market}/info", lambda b: b.get("family") == 4)
        send(kp, "risk_quote", owner=owner, market=market, layer=1, capacity=100, premium=50, profit_share_bps=1000)
        send(lp2, "risk_quote", owner=str(lp2.pubkey()), market=market, layer=1, capacity=100, premium=200, profit_share_bps=1000)
        send(kp, "risk_quote", owner=owner, market=market, layer=2, capacity=100, premium=80, profit_share_bps=1000)
    except Exception as e:
        fail(cid, f"setup {e}")
        return
    try:
        book = wait_json(
            f"/v1/markets/{market}/layers",
            lambda b: len(b.get("quotes") or []) >= 2 and len(b.get("layers") or []) >= 2,
        )
    except Exception as e:
        fail(cid, f"layers {e}")
        return
    layers = {int(l["id"]): l for l in book.get("layers") or []}
    expect_eq(cid, int(layers.get(1, {}).get("attachment") or -1), 0, "layer1 A=0")
    expect_eq(cid, int(layers.get(2, {}).get("attachment") or -1), 10, "layer2 A=d")
    l1 = sorted([q for q in book.get("quotes") or [] if int(q.get("layer") or 0) == 1], key=lambda q: int(q.get("unit_premium") or 0))
    if len(l1) < 2:
        fail(cid, f"want 2 layer-1 quotes got {l1}")
        return
    expect_eq(cid, l1[0].get("lp"), owner, "cheapest quote is first LP")
    if int(l1[0]["unit_premium"]) >= int(l1[1]["unit_premium"]):
        fail(cid, f"unit premium not ranked {l1[0]['unit_premium']} vs {l1[1]['unit_premium']}")
    if int(l1[0].get("filled") or 0) < 1:
        fail(cid, "cheapest quote should try_fill first")
    if int(l1[1].get("filled") or 0) != 0:
        fail(cid, f"dearer quote filled {l1[1].get('filled')} — matching should leave it standing")
    ok(cid, f"rank {l1[0]['unit_premium']}<{l1[1]['unit_premium']} A1=0 A2=10 filled0={l1[0].get('filled')}")

    mask = mask_hex([1], 2)
    lp2_owner = str(lp2.pubkey())
    try:
        send(kp, "buy_set", owner=owner, market=market, mask=mask, shares=80, nonce=1)
    except Exception as e:
        fail(cid, f"buy 80 {e}")
        return
    fill_or_skip(kp, cid, owner, market, 1)
    fill_or_skip(lp2, cid, lp2_owner, market, 1)
    fill_or_skip(kp, cid, owner, market, 2)
    try:
        book2 = wait_json(f"/v1/markets/{market}/layers", lambda b: int(b.get("c_r") or 0) >= 1)
    except Exception as e:
        fail(cid, f"layers after trade {e}")
        return
    cr_live = int(book2.get("c_r") or 0)
    cheap = quote_of(book2, owner, 1)
    dear = quote_of(book2, str(lp2.pubkey()), 1)
    if int(cheap.get("filled") or 0) < 1:
        fail(cid, "cheapest quote empty after post-trade fill_next")
    if cr_live < int(cheap.get("filled") or 0):
        fail(cid, f"C_R {cr_live} < cheapest filled {cheap.get('filled')}")
    ok(cid, f"post-trade match C_R={cr_live} cheap={cheap.get('filled')} dear={dear.get('filled')}")

    reserved_lp2 = int(vault_of(lp2_owner).get("reserved") or 0)
    if int(dear.get("filled") or 0) < int(dear.get("capacity") or 0):
        try:
            send(lp2, "cancel_unfilled", owner=lp2_owner, market=market, layer=1)
            wait_json(
                f"/v1/owners/{lp2_owner}/risk",
                lambda b: any(i.get("market") == market and i.get("cancelled") for i in b.get("items") or []),
            )
            reserved_after = int(vault_of(lp2_owner).get("reserved") or 0)
            if reserved_after >= reserved_lp2:
                fail(cid, f"cancel leftover did not unlock reserved {reserved_lp2}→{reserved_after}")
            else:
                ok(cid, f"cancel_unfilled reserved {reserved_lp2}→{reserved_after}")
        except Exception as e:
            fail(cid, f"cancel_unfilled {e}")

    remain = close_ts - time.time() + 2
    if remain > 0:
        time.sleep(remain)
    try:
        settled = resolve_to_settle(kp, owner, market, 4, 4, 1)
    except Exception as e:
        fail(cid, f"resolve/settle {e}")
        return
    L = int(settled.get("liability") or 0)
    c_max = int(settled.get("c_max_usdc") or 0)
    r_net = int(settled.get("r_net") or 0)
    c_m = int(settled.get("c_m") or 0)
    c_r = int(settled.get("c_r") or 0)
    c_p = int(settled.get("c_p_alloc") or 0)
    expect_eq(cid, c_max, r_net + c_r + c_p, "C_max=R_net+C_R+C_P")
    if c_r < 1:
        fail(cid, f"settled C_R {c_r} want ≥1 (include_risk)")
    try:
        send(kp, "payout", owner=owner, market=market, mask=mask)
    except Exception as e:
        fail(cid, f"payout {e}")
        return
    run_lp_after_settle(kp, cid, market, owner)
    want_h = h_hat(L, 50, int(cheap.get("filled") or 0))
    if L > 50 and want_h < 1 and int(cheap.get("filled") or 0) >= 1:
        fail(cid, f"L={L}>A=50 filled={cheap.get('filled')} but Ĥ={want_h}")
    ok(cid, f"full auction L={L} C_R={c_r} C_max={c_max} Ĥ={want_h} ρ={settled.get('rho_bps')}")
    RESULTS.append({"id": cid, "market": market, "layers": 2, "c_r": c_r, "liability": L, "h": want_h})


def run_void_release(kp, stamp: str) -> None:
    cid = "auction-void-release"
    spec = {
        "id": cid,
        "family": 4,
        "op": "create_bernoulli",
        "topic": "av",
        "tag": "void",
        "n": 2,
        "title": "Auction VOID unlock",
        "category": "binary",
        "c_m": 50,
        "auction": False,
        "buy": "cells",
        "cells": [1],
        "kind": 4,
        "value": 1,
        "expect_label": "YES",
        "hit": True,
    }
    owner = str(kp.pubkey())
    topic = f"{spec['topic']}{stamp}"
    close_ts = int(time.time()) + CLOSE_IN
    market = market_pda(4, topic, spec["tag"])
    flow.out(f"CASE {cid} market {market}")
    try:
        send(kp, **create_body(spec, owner, topic, spec["tag"], close_ts))
        send(kp, "fund_cm", owner=owner, market=market, amount=0)
        send(kp, "risk_open_book", owner=owner, market=market)
        flow.http_json("POST", f"{flow.API}/v1/listings", {"market": market, "title": spec["title"], "category": spec["category"], "topic": topic, "tag": spec["tag"], "description": cid, "event": cid})
        wait_json(f"/v1/markets/{market}/info", lambda b: b.get("n") == 2)
        send(kp, "risk_quote", owner=owner, market=market, layer=1, capacity=100, premium=100, profit_share_bps=2000)
        mask = mask_hex([1], 2)
        send(kp, "buy_set", owner=owner, market=market, mask=mask, shares=1, nonce=1)
        remain = close_ts - time.time() + 2
        if remain > 0:
            time.sleep(remain)
        send(kp, "resolve_open", owner=owner, market=market)
        wait_json(f"/v1/markets/{market}/resolution", lambda b: b.get("phase") == 0)
        send(kp, "void_resolution", owner=owner, market=market)
        rec = wait_json(f"/v1/markets/{market}/resolution", lambda b: int(b.get("phase") or 0) >= 5 or b.get("refunds_due"))
        expect_eq(cid, bool(rec.get("refunds_due")), True, "refunds_due")
        reserved0 = int(vault_of(owner).get("reserved") or 0)
        send(kp, "begin_refund", owner=owner, market=market)
        wait_json(f"/v1/markets/{market}/info", lambda b: int(b.get("board_phase") or 0) == 2)
        pos = wait_json(
            f"/v1/owners/{owner}/positions?q={market}",
            lambda b: any(i.get("market") == market and not i.get("claimed") for i in b.get("items") or []),
        )
        ticket = next(i for i in pos["items"] if i["market"] == market)
        send(kp, "refund", owner=owner, market=market, position=ticket["position"])
        claimed = wait_json(
            f"/v1/owners/{owner}/positions?q={market}",
            lambda b: any(i.get("market") == market and i.get("claimed") for i in b.get("items") or []),
        )
        row = next(i for i in claimed["items"] if i["market"] == market)
        expect_eq(cid, row.get("prompt"), "refunded", "void ticket prompt")
        expect_eq(cid, int(row.get("paid_usdc") or 0), int(ticket.get("cost_paid") or row.get("paid_usdc") or 0), "refund = cost_paid")
        send(kp, "release_lp", owner=owner, market=market, layer=1)
        reserved1 = int(vault_of(owner).get("reserved") or 0)
        if reserved1 >= reserved0 and reserved0 > 0:
            fail(cid, f"release_lp reserved stayed {reserved0}→{reserved1}")
        ok(cid, f"void phase={rec.get('phase')} refunded={row.get('paid_usdc')} reserved {reserved0}→{reserved1}")
    except Exception as e:
        fail(cid, str(e))
        return
    RESULTS.append({"id": cid, "market": market, "void": True})


def run_pool_tap(kp, stamp: str) -> None:
    cid = "pool-tap-settle"
    owner = str(kp.pubkey())
    try:
        send(kp, "init_pool", owner=owner)
    except Exception as e:
        flow.out(f"init_pool {e}")
    try:
        send(kp, "fund_pool", owner=owner, amount=200)
        ok(cid, "fund_pool 200")
    except Exception as e:
        fail(cid, f"fund_pool {e}")
        return
    spec = {
        "id": cid,
        "family": 4,
        "op": "create_bernoulli",
        "topic": "cp",
        "tag": "tap",
        "n": 2,
        "title": "C_P tap settle",
        "category": "binary",
        "c_m": 10,
        "auction": False,
        "buy": "cells",
        "cells": [1],
        "kind": 4,
        "value": 1,
        "expect_cell": 1,
        "expect_label": "YES",
        "hit": True,
    }
    topic = f"{spec['topic']}{stamp}"
    close_ts = int(time.time()) + CLOSE_IN
    market = market_pda(4, topic, spec["tag"])
    flow.out(f"CASE {cid} market {market}")
    mask = mask_hex([1], 2)
    try:
        send(kp, **create_body(spec, owner, topic, spec["tag"], close_ts))
        send(kp, "fund_cm", owner=owner, market=market, amount=0)
        send(kp, "risk_open_book", owner=owner, market=market)
        send(kp, "set_tap", owner=owner, market=market, amount=20)
        flow.http_json("POST", f"{flow.API}/v1/listings", {"market": market, "title": spec["title"], "category": spec["category"], "topic": topic, "tag": spec["tag"], "description": cid, "event": cid})
        wait_json(f"/v1/markets/{market}/info", lambda b: b.get("n") == 2)
        send(kp, "buy_set", owner=owner, market=market, mask=mask, shares=80, nonce=1)
        remain = close_ts - time.time() + 2
        if remain > 0:
            time.sleep(remain)
        send(kp, "resolve_open", owner=owner, market=market)
        wait_json(f"/v1/markets/{market}/resolution", lambda b: b.get("phase") == 0)
        send(kp, "submit_result", owner=owner, market=market, family=4, kind=4, value=1)
        wait_json(f"/v1/markets/{market}/resolution", lambda b: b.get("phase") == 1)
        time.sleep(CHALLENGE + 1)
        send(kp, "finalize", owner=owner, market=market)
        wait_json(f"/v1/markets/{market}/resolution", lambda b: b.get("phase") == 3)
        send(kp, "begin_settle", owner=owner, market=market, include_pool=True)
        settled = wait_json(f"/v1/markets/{market}/info", lambda b: int(b.get("board_phase") or 0) >= 1)
    except Exception as e:
        fail(cid, str(e))
        return
    L = int(settled.get("liability") or 0)
    c_m = int(settled.get("c_m") or 0)
    c_r = int(settled.get("c_r") or 0)
    r_net = int(settled.get("r_net") or 0)
    c_p = int(settled.get("c_p_alloc") or 0)
    c_max = int(settled.get("c_max_usdc") or 0)
    expect_eq(cid, c_max, r_net + c_r + c_p, "C_max with pool")
    own = r_net + c_r
    if L > own and c_p == 0:
        fail(cid, f"shortfall L={L} own={own} but C_P^alloc=0")
    if L > own and c_p < 1:
        fail(cid, f"C_P tap did not allocate L={L} own={own}")
    if L > own:
        expect_range(cid, c_p, 1, 20, "C_P^alloc from tap=20")
    ok(cid, f"L={L} C_R={c_r} C_P={c_p} C_max={c_max} ρ={settled.get('rho_bps')}")
    RESULTS.append({"id": cid, "market": market, "liability": L, "c_p_alloc": c_p, "c_max": c_max, "rho_bps": settled.get("rho_bps")})


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    only = os.environ.get("CASES", "all")
    kp = None
    if only in ("all", "prior"):
        flow.out("=== prior / compose cases ===")
        run_prior_cases()
        run_compose_milli_case()
    if only in ("all", "signed", "extras", "football"):
        flow.out("=== signed family flows ===")
        kp = flow.fund_chain()
        try:
            send(kp, "deposit", owner=str(kp.pubkey()), amount=8000)
            ok("deposit", "8000")
        except Exception as e:
            fail("deposit", str(e))
            return 1
    if kp is None:
        kp = flow.fund_chain()
    stamp = str(int(time.time()) % 10_000_000)
    if only in ("all", "signed"):
        flow.out("=== family create/trade/auction/settle/claim ===")
        pick = os.environ.get("CASE")
        for spec in FLOW_CASES:
            if pick and spec["id"] != pick:
                continue
            run_flow_case(kp, spec, stamp)
    if only in ("all", "extras"):
        flow.out("=== vault / session / auction lifecycle / void / C_P ===")
        run_vault_session(kp)
        try:
            lp2 = fund_second(kp)
            run_two_lp_rank(kp, lp2, stamp)
        except Exception as e:
            fail("lp2-setup", str(e))
        run_void_release(kp, stamp)
        run_pool_tap(kp, stamp)
    if only in ("all", "football"):
        fb_spec = importlib.util.spec_from_file_location("football_cases", ROOT / "scripts" / "phase6-football-cases.py")
        football = importlib.util.module_from_spec(fb_spec)
        assert fb_spec.loader
        fb_spec.loader.exec_module(football)
        football.run_all_football(kp, stamp)
    (OUT / "results.json").write_text(json.dumps(RESULTS, indent=2), encoding="utf-8")
    (OUT / "findings.json").write_text(json.dumps(FINDINGS, indent=2), encoding="utf-8")
    flow.out("FINDINGS " + str(len(FINDINGS)))
    for row in FINDINGS:
        flow.out("- " + row)
    return 1 if FINDINGS else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as e:
        flow.out("CASES_FATAL " + str(e))
        raise
