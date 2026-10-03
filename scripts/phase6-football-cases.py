#!/usr/bin/env python3
"""Football Skellam board: 1X2, handicap 1X2, AH, totals, BTTS, exact, quarter settle."""

from __future__ import annotations

import importlib.util
import json
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

if "phase6_family_cases" in sys.modules:
    fc = sys.modules["phase6_family_cases"]
else:
    spec = importlib.util.spec_from_file_location("phase6_family_cases", ROOT / "scripts" / "phase6-family-cases.py")
    fc = importlib.util.module_from_spec(spec)
    assert spec.loader
    sys.modules["phase6_family_cases"] = fc
    spec.loader.exec_module(fc)

K_MAX = 10
FOOTBALL_CLOSE = 180
OUT = ROOT / "tmp" / "phase6-football"


def cell(home: int, away: int) -> int:
    return min(home, K_MAX) * 11 + min(away, K_MAX)


def cells_where(pred) -> list[int]:
    return [cell(i, j) for i in range(K_MAX + 1) for j in range(K_MAX + 1) if pred(i, j)]


def ticket_face(q: int, hit: int, tot: int) -> int:
    if q <= 0 or hit <= 0 or tot <= 0:
        return 0
    if hit >= tot:
        return q
    return (q * hit) // tot


def quarter_to_halves(q: int) -> tuple[int, int]:
    lo = q // 2
    return lo, lo + 1


def mask_bits(kind: int, a: int, b: int) -> list[list[bool]]:
    def bits(pred) -> list[bool]:
        return [pred(i, j) for i in range(K_MAX + 1) for j in range(K_MAX + 1)]

    if kind == 0:
        return [bits(lambda i, j: i > j)]
    if kind == 1:
        return [bits(lambda i, j: i == j)]
    if kind == 2:
        return [bits(lambda i, j: i < j)]
    if kind == 3:
        return [bits(lambda i, j: 2 * (i + j) > a)]
    if kind == 4:
        return [bits(lambda i, j: 2 * (i + j) <= a)]
    if kind == 5:
        return [bits(lambda i, j: i >= 1 and j >= 1)]
    if kind == 6:
        return [bits(lambda i, j: i == 0 or j == 0)]
    if kind == 7:
        m = [False] * 121
        m[cell(a, b)] = True
        return [m]
    if kind == 8:
        return [bits(lambda i, j: 2 * (i - j) + a > 0)]
    if kind == 9:
        return [bits(lambda i, j: 2 * (j - i) + a > 0)]
    if kind in (10, 11):
        x, y = quarter_to_halves(a)
        home = kind == 10
        return [
            bits(lambda i, j, line=x: (2 * (i - j) + line if home else 2 * (j - i) + line) > 0),
            bits(lambda i, j, line=y: (2 * (i - j) + line if home else 2 * (j - i) + line) > 0),
        ]
    raise ValueError(f"kind {kind}")


def hit_parts(kind: int, a: int, b: int, home: int, away: int) -> tuple[int, int]:
    c = cell(home, away)
    masks = mask_bits(kind, a, b)
    return sum(1 for m in masks if m[c]), len(masks)


def h1x2_cells(h: int, side: str) -> list[int]:
    if side == "win":
        return cells_where(lambda i, j: i - j > h)
    if side == "draw":
        return cells_where(lambda i, j: i - j == h)
    return cells_where(lambda i, j: i - j < h)


def h1x2_hit(h: int, side: str, home: int, away: int) -> bool:
    d = home - away
    if side == "win":
        return d > h
    if side == "draw":
        return d == h
    return d < h


def quote_mask(market: str, cells: list[int], shares: int = 1) -> dict:
    mask = fc.mask_hex(cells, 121)
    st, body = fc.get(f"/v1/markets/{market}/quote?mask={mask}&shares={shares}")
    if st != 200:
        raise RuntimeError(f"quote {st} {body}")
    return body


def _create_skellam(kp, owner: str, topic: str, tag: str, title: str, cid: str, close_ts: int, c_m: int = 0) -> str:
    market = fc.market_pda(0, topic, tag)
    create = {
        "op": "create_skellam",
        "family": 0,
        "topic": topic,
        "tag": tag,
        "n": 121,
        "title": title,
        "category": "football",
        "milli": True,
        "lambda_home": 1400,
        "lambda_away": 1100,
        "c_m": c_m,
        "challenge_secs": 3,
        "report_window_secs": 800,
        "id": cid,
    }
    fc.record_application(kp, create, topic, tag, close_ts, title, cid)
    fc.send(kp, **fc.create_body(create, owner, topic, tag, close_ts))
    fc.finish_grid(kp, market, 121)
    fc.send(kp, "fund_cm", owner=owner, market=market, amount=0)
    try:
        fc.send(kp, "risk_open_book", owner=owner, market=market)
    except Exception:
        pass
    fc.flow.http_json(
        "POST",
        f"{fc.flow.API}/v1/listings",
        {"market": market, "title": title, "category": "football", "topic": topic, "tag": tag, "description": cid, "event": cid},
    )
    fc.wait_json(f"/v1/markets/{market}/info", lambda b: b.get("family") == 0 and b.get("n") == 121)
    return market


def run_shared_theta(kp, stamp: str) -> None:
    cid = "football-shared-theta"
    owner = str(kp.pubkey())
    topic = f"th{stamp}"
    close_ts = int(time.time()) + FOOTBALL_CLOSE
    try:
        market = _create_skellam(kp, owner, topic, "th", f"Football shared θ {stamp}", f"{cid}-{stamp}", close_ts)
    except Exception as e:
        fc.fail(cid, f"create {e}")
        return
    exact = [cell(2, 1)]
    home = cells_where(lambda i, j: i > j)
    q0 = quote_mask(market, exact)
    h0 = quote_mask(market, home)
    try:
        fc.send(kp, "buy_skellam_set", owner=owner, market=market, kind=0, value=0, value_b=0, shares=1, nonce=1)
    except Exception as e:
        fc.fail(cid, f"buy home {e}")
        return
    try:
        q1 = fc.wait_json(
            f"/v1/markets/{market}/quote?mask={fc.mask_hex(exact, 121)}&shares=1",
            lambda b: int(b.get("p_s_bps") or 0) > int(q0["p_s_bps"]),
        )
        h1 = quote_mask(market, home)
        fc.ok(cid, f"home buy raised exact 2-1 {q0['p_s_bps']}→{q1['p_s_bps']} and home {h0['p_s_bps']}→{h1['p_s_bps']}")
    except Exception as e:
        fc.fail(cid, f"shared θ {e}")
        return
    remain = close_ts - time.time() + 2
    if remain > 0:
        time.sleep(remain)
    try:
        settled = fc.resolve_to_settle(kp, owner, market, 0, 0, 2, 1, challenge_wait=3)
        fc.expect_eq(cid, int(settled.get("settle_cell")), cell(2, 1), "settle_cell 2-1")
        fc.send(kp, "payout_skellam", owner=owner, market=market, kind=0, value=0, value_b=0)
        book = fc.wait_json(
            f"/v1/owners/{owner}/positions?q={market}&limit=100",
            lambda b: any(i.get("market") == market and i.get("claimed") for i in b.get("items") or []),
        )
        row = next(i for i in book["items"] if i["market"] == market)
        fc.expect_eq(cid, int(row.get("paid_usdc") or 0), 1, "home HIT paid")
        fc.ok(cid, f"home-only settle L={settled.get('liability')} paid={row.get('paid_usdc')}")
    except Exception as e:
        fc.fail(cid, f"settle {e}")


def run_football_board(kp, spec: dict, stamp: str) -> None:
    cid = spec["id"]
    owner = str(kp.pubkey())
    topic = f"{spec['topic']}{stamp}"
    tag = spec["tag"]
    close_ts = int(time.time()) + FOOTBALL_CLOSE
    market = fc.market_pda(0, topic, tag)
    home, away = spec["score"]
    fc.flow.out(f"CASE {cid} market {market} x*={home}-{away}")

    create = {
        "op": "create_skellam",
        "family": 0,
        "topic": topic,
        "tag": tag,
        "n": 121,
        "title": spec["title"],
        "category": "football",
        "milli": True,
        "lambda_home": 1400,
        "lambda_away": 1100,
        "c_m": 0,
        "id": cid,
    }
    try:
        fc.record_application(kp, create, topic, tag, close_ts, spec["title"], cid)
        fc.send(kp, **fc.create_body(create, owner, topic, tag, close_ts))
        fc.finish_grid(kp, market, 121)
        fc.send(kp, "fund_cm", owner=owner, market=market, amount=0)
        try:
            fc.send(kp, "risk_open_book", owner=owner, market=market)
        except Exception:
            pass
    except Exception as e:
        fc.fail(cid, f"create {e}")
        return

    fc.flow.http_json(
        "POST",
        f"{fc.flow.API}/v1/listings",
        {
            "market": market,
            "title": spec["title"],
            "category": "football",
            "topic": topic,
            "tag": tag,
            "description": cid,
            "event": cid,
        },
    )
    try:
        info = fc.wait_json(f"/v1/markets/{market}/info", lambda b: b.get("family") == 0 and b.get("n") == 121)
    except Exception as e:
        fc.fail(cid, f"info {e}")
        return
    fc.expect_eq(cid, info.get("n"), 121, "n")

    st, prior = fc.get("/v1/prior?family=0&milli=true&lambda_home=1400&lambda_away=1100")
    if st != 200:
        fc.fail(cid, f"prior {st}")
        return
    lines = {r["label"]: int(r["p_bps"]) for r in prior.get("lines") or []}
    if set(lines) < {"Home", "Draw", "Away"}:
        fc.fail(cid, f"prior missing 1X2 {lines}")
    else:
        fc.expect_range(cid, lines["Home"] + lines["Draw"] + lines["Away"], 9900, 10100, "1X2 prior sum")
        fc.ok(cid, f"prior 1X2 H={lines['Home']} D={lines['Draw']} A={lines['Away']}")

    exact_cells = [cell(2, 1)]
    q_exact0 = quote_mask(market, exact_cells)
    home_cells = cells_where(lambda i, j: i > j)
    q_home0 = quote_mask(market, home_cells)
    nonce = 1
    try:
        fc.send(kp, "buy_skellam_set", owner=owner, market=market, kind=0, value=0, value_b=0, shares=1, nonce=nonce)
        nonce += 1
    except Exception as e:
        fc.fail(cid, f"probe buy home {e}")
        return
    try:
        q_exact1 = fc.wait_json(
            f"/v1/markets/{market}/quote?mask={fc.mask_hex(exact_cells, 121)}&shares=1",
            lambda b: int(b.get("p_s_bps") or 0) > int(q_exact0["p_s_bps"]),
        )
        q_home1 = quote_mask(market, home_cells)
        fc.ok(cid, f"shared θ: exact 2-1 {q_exact0['p_s_bps']}→{q_exact1['p_s_bps']} home {q_home0['p_s_bps']}→{q_home1['p_s_bps']}")
    except Exception as e:
        fc.fail(cid, f"shared θ did not move after home buy {e}")

    tickets = spec["tickets"]
    # First ticket is the probe home buy already sent if it matches.
    started = 0
    if tickets and tickets[0].get("reuse_probe"):
        tickets[0] = {**tickets[0], "nonce": 1}
        started = 1
    for t in tickets[started:]:
        t["nonce"] = nonce
        try:
            if t["via"] == "skellam":
                fc.send(
                    kp,
                    "buy_skellam_set",
                    owner=owner,
                    market=market,
                    kind=t["kind"],
                    value=t.get("a", 0),
                    value_b=t.get("b", 0),
                    shares=t.get("q", 1),
                    nonce=nonce,
                )
            else:
                fc.send(
                    kp,
                    "buy_set",
                    owner=owner,
                    market=market,
                    mask=fc.mask_hex(t["cells"], 121),
                    shares=t.get("q", 1),
                    nonce=nonce,
                )
            nonce += 1
        except Exception as e:
            fc.fail(cid, f"buy {t['id']} {e}")
            return
        q = t.get("q", 1)
        if t["via"] == "skellam":
            hit, tot = hit_parts(t["kind"], t.get("a", 0), t.get("b", 0), home, away)
            want_face = ticket_face(q, hit, tot)
        else:
            want_face = q if cell(home, away) in t["cells"] else 0
        t["want_face"] = want_face
        t["hit"] = want_face > 0
        fc.ok(cid, f"bought {t['id']} q={q} face@{home}-{away}={want_face}")

    if tickets and tickets[0].get("reuse_probe"):
        t0 = tickets[0]
        hit, tot = hit_parts(t0["kind"], t0.get("a", 0), t0.get("b", 0), home, away)
        t0["want_face"] = ticket_face(t0.get("q", 1), hit, tot)
        t0["hit"] = t0["want_face"] > 0

    remain = close_ts - time.time() + 2
    if remain > 0:
        time.sleep(remain)
    try:
        settled = fc.resolve_to_settle(kp, owner, market, 0, 0, home, away)
    except Exception as e:
        fc.fail(cid, f"resolve/settle {e}")
        return

    want_cell = cell(home, away)
    fc.expect_eq(cid, int(settled.get("settle_cell") or -1), want_cell, "settle_cell")
    fc.expect_eq(cid, settled.get("final_result"), spec["expect_label"], "final_result")
    L = int(settled.get("liability") or 0)
    rho = int(settled.get("rho_bps") or 0)
    c_max = int(settled.get("c_max_usdc") or 0)
    want_l = sum(int(t.get("want_face") or 0) for t in tickets)
    fc.expect_eq(cid, L, want_l, "L=Σ face(x*)")
    if L > 0 and c_max >= L:
        fc.expect_eq(cid, rho, 10000, "ρ when C_max≥L")
    fc.ok(cid, f"settled {settled.get('final_result')} cell={want_cell} L={L} ρ={rho} C_max={c_max}")

    for t in tickets:
        st, before = fc.get(f"/v1/owners/{owner}/positions?q={market}&limit=100")
        claimed0 = {i.get("position") for i in (before.get("items") or []) if i.get("claimed") and i.get("market") == market}
        try:
            if t["via"] == "skellam":
                fc.send(
                    kp,
                    "payout_skellam",
                    owner=owner,
                    market=market,
                    kind=t["kind"],
                    value=t.get("a", 0),
                    value_b=t.get("b", 0),
                )
            else:
                fc.send(kp, "payout", owner=owner, market=market, mask=fc.mask_hex(t["cells"], 121))
        except Exception as e:
            fc.fail(cid, f"payout {t['id']} {e}")
            continue
        try:
            book = fc.wait_json(
                f"/v1/owners/{owner}/positions?q={market}&limit=100",
                lambda b: any(
                    i.get("market") == market and i.get("claimed") and i.get("position") not in claimed0
                    for i in b.get("items") or []
                ),
            )
        except Exception as e:
            fc.fail(cid, f"positions after {t['id']} {e}")
            continue
        row = next(
            i
            for i in book["items"]
            if i.get("market") == market and i.get("claimed") and i.get("position") not in claimed0
        )
        paid = int(row.get("paid_usdc") or 0)
        want_pay = 0 if not t["hit"] else (t["want_face"] * rho) // 10000
        fc.expect_eq(cid, paid, want_pay, f"{t['id']} payout")
        if t["hit"] and paid == 0 and want_pay > 0:
            fc.fail(cid, f"{t['id']} HIT paid 0")
        if not t["hit"] and paid != 0:
            fc.fail(cid, f"{t['id']} MISS paid {paid}")
        fc.ok(cid, f"claim {t['id']} paid={paid} face={t['want_face']} hit={t['hit']}")

    fc.RESULTS.append(
        {
            "id": cid,
            "market": market,
            "score": f"{home}-{away}",
            "liability": L,
            "rho_bps": rho,
            "tickets": [{k: t.get(k) for k in ("id", "want_face", "hit")} for t in tickets],
        }
    )


def skellam_t(tid: str, kind: int, a: int = 0, b: int = 0, q: int = 1, reuse_probe: bool = False) -> dict:
    return {"id": tid, "via": "skellam", "kind": kind, "a": a, "b": b, "q": q, "reuse_probe": reuse_probe}


def mask_t(tid: str, cells: list[int], q: int = 1) -> dict:
    return {"id": tid, "via": "mask", "cells": cells, "q": q}


def isolated_cases() -> list[dict]:
    """One fill per board: L1 121-cell LMSR cannot stack a second buy_skellam (1.4M CU)."""
    h = 1
    return [
        {"id": "1x2-home-hit", "score": (2, 1), "label": "2-1", "via": "skellam", "kind": 0, "a": 0, "b": 0, "q": 1},
        {"id": "1x2-draw-miss", "score": (2, 1), "label": "2-1", "via": "skellam", "kind": 1, "a": 0, "b": 0, "q": 1},
        {"id": "1x2-away-miss", "score": (2, 1), "label": "2-1", "via": "skellam", "kind": 2, "a": 0, "b": 0, "q": 1},
        {"id": "1x2-draw-hit", "score": (1, 1), "label": "1-1", "via": "skellam", "kind": 1, "a": 0, "b": 0, "q": 1},
        {"id": "1x2-away-hit", "score": (0, 3), "label": "0-3", "via": "skellam", "kind": 2, "a": 0, "b": 0, "q": 1},
        {"id": "ou-over25-hit", "score": (2, 1), "label": "2-1", "via": "skellam", "kind": 3, "a": 5, "b": 0, "q": 1},
        {"id": "ou-under25-miss", "score": (2, 1), "label": "2-1", "via": "skellam", "kind": 4, "a": 5, "b": 0, "q": 1},
        {"id": "ou-under25-hit", "score": (1, 1), "label": "1-1", "via": "skellam", "kind": 4, "a": 5, "b": 0, "q": 1},
        {"id": "btts-yes-hit", "score": (2, 1), "label": "2-1", "via": "skellam", "kind": 5, "a": 0, "b": 0, "q": 1},
        {"id": "btts-no-hit", "score": (0, 3), "label": "0-3", "via": "skellam", "kind": 6, "a": 0, "b": 0, "q": 1},
        {"id": "cs-2-1-hit", "score": (2, 1), "label": "2-1", "via": "skellam", "kind": 7, "a": 2, "b": 1, "q": 1},
        {"id": "cs-1-0-miss", "score": (2, 1), "label": "2-1", "via": "skellam", "kind": 7, "a": 1, "b": 0, "q": 1},
        {"id": "ah-home-05-hit", "score": (2, 1), "label": "2-1", "via": "skellam", "kind": 8, "a": -1, "b": 0, "q": 1},
        {"id": "ah-home-10-push", "score": (2, 1), "label": "2-1", "via": "skellam", "kind": 8, "a": -2, "b": 0, "q": 1},
        {"id": "ah-home-15-miss", "score": (2, 1), "label": "2-1", "via": "skellam", "kind": 8, "a": -3, "b": 0, "q": 1},
        {"id": "h1x2-win-miss", "score": (2, 1), "label": "2-1", "via": "mask", "cells": h1x2_cells(h, "win"), "q": 1},
        {"id": "h1x2-draw-hit", "score": (2, 1), "label": "2-1", "via": "mask", "cells": h1x2_cells(h, "draw"), "q": 1},
        {"id": "h1x2-lose-hit", "score": (1, 1), "label": "1-1", "via": "mask", "cells": h1x2_cells(h, "lose"), "q": 1},
        {"id": "cs-10-1-overflow", "score": (12, 1), "label": "10+-1", "via": "skellam", "kind": 7, "a": 10, "b": 1, "q": 1},
        {"id": "h1x2-win-overflow", "score": (12, 1), "label": "10+-1", "via": "mask", "cells": h1x2_cells(h, "win"), "q": 1},
    ]


def run_isolated_matrix(kp, stamp: str) -> None:
    owner = str(kp.pubkey())
    built: list[dict] = []
    for i, spec in enumerate(isolated_cases()):
        cid = spec["id"]
        topic = f"i{i}{stamp}"[:28]
        tag = f"t{i}"
        home, away = spec["score"]
        q = spec.get("q", 1)
        close_ts = int(time.time()) + FOOTBALL_CLOSE
        if spec["via"] == "skellam":
            hit, tot = hit_parts(spec["kind"], spec.get("a", 0), spec.get("b", 0), home, away)
            want_face = ticket_face(q, hit, tot)
        else:
            want_face = q if cell(home, away) in spec["cells"] else 0
        fc.flow.out(f"CASE {cid} x*={home}-{away} face={want_face}")
        try:
            market = _create_skellam(kp, owner, topic, tag, f"{cid} {stamp}", f"{cid} {stamp}", close_ts)
            if spec["via"] == "skellam":
                fc.send(
                    kp,
                    "buy_skellam_set",
                    owner=owner,
                    market=market,
                    kind=spec["kind"],
                    value=spec.get("a", 0),
                    value_b=spec.get("b", 0),
                    shares=q,
                    nonce=1,
                )
            else:
                fc.send(kp, "buy_set", owner=owner, market=market, mask=fc.mask_hex(spec["cells"], 121), shares=q, nonce=1)
        except Exception as e:
            fc.fail(cid, f"create/buy {e}")
            continue
        built.append({**spec, "market": market, "want_face": want_face, "topic": topic, "tag": tag, "close_ts": close_ts})
        fc.ok(cid, f"listed {market[:8]}… face={want_face}")

    remain = (max(b["close_ts"] for b in built) if built else time.time()) - time.time() + 2
    if remain > 0:
        fc.flow.out(f"wait close {remain:.1f}s for {len(built)} football boards")
        time.sleep(remain)

    for spec in built:
        cid = spec["id"]
        market = spec["market"]
        home, away = spec["score"]
        try:
            settled = fc.resolve_to_settle(kp, owner, market, 0, 0, home, away, challenge_wait=3)
        except Exception as e:
            fc.fail(cid, f"settle {e}")
            continue
        fc.expect_eq(cid, int(settled.get("settle_cell") or -1), cell(home, away), "settle_cell")
        fc.expect_eq(cid, settled.get("final_result"), spec["label"], "final_result")
        L = int(settled.get("liability") or 0)
        rho = int(settled.get("rho_bps") or 0)
        fc.expect_eq(cid, L, spec["want_face"], "L=face")
        if L > 0 and int(settled.get("c_max_usdc") or 0) >= L:
            fc.expect_eq(cid, rho, 10000, "ρ")
        try:
            if spec["via"] == "skellam":
                fc.send(
                    kp,
                    "payout_skellam",
                    owner=owner,
                    market=market,
                    kind=spec["kind"],
                    value=spec.get("a", 0),
                    value_b=spec.get("b", 0),
                )
            else:
                fc.send(kp, "payout", owner=owner, market=market, mask=fc.mask_hex(spec["cells"], 121))
            book = fc.wait_json(
                f"/v1/owners/{owner}/positions?q={market}&limit=20",
                lambda b: any(i.get("market") == market and i.get("claimed") for i in b.get("items") or []),
            )
            row = next(i for i in book["items"] if i["market"] == market)
            paid = int(row.get("paid_usdc") or 0)
            want_pay = (spec["want_face"] * rho) // 10000 if spec["want_face"] else 0
            fc.expect_eq(cid, paid, want_pay, "payout")
            fc.ok(cid, f"{spec['label']} L={L} paid={paid} face={spec['want_face']}")
        except Exception as e:
            fc.fail(cid, f"payout {e}")
        fc.RESULTS.append({"id": cid, "market": market, "score": f"{home}-{away}", "face": spec["want_face"], "L": L})


def football_specs_unused() -> list[dict]:
    h = 1
    return [
        {
            "id": "football-2-1-full-book",
            "topic": "ft",
            "tag": "ft21",
            "title": "Football 2-1 full ticket book",
            "score": (2, 1),
            "expect_label": "2-1",
            "c_m": 80,
            "tickets": [
                skellam_t("1x2-home", 0, reuse_probe=True),
                skellam_t("1x2-draw", 1),
                skellam_t("1x2-away", 2),
                skellam_t("ou-over-25", 3, 5),
                skellam_t("ou-under-25", 4, 5),
                skellam_t("btts-yes", 5),
                skellam_t("btts-no", 6),
                skellam_t("cs-2-1", 7, 2, 1),
                skellam_t("cs-1-0", 7, 1, 0),
                skellam_t("ah-home-05", 8, -1),
                skellam_t("ah-home-10", 8, -2),
                skellam_t("ah-home-15", 8, -3),
                skellam_t("ah-away-05", 9, 1),
                skellam_t("q-home-075", 10, -3, q=2),
                mask_t("h1x2-win", h1x2_cells(h, "win")),
                mask_t("h1x2-draw", h1x2_cells(h, "draw")),
                mask_t("h1x2-lose", h1x2_cells(h, "lose")),
            ],
        },
        {
            "id": "football-1-1-draw-handicap",
            "topic": "fd",
            "tag": "ft11",
            "title": "Football 1-1 draw + handicap",
            "score": (1, 1),
            "expect_label": "1-1",
            "c_m": 80,
            "tickets": [
                skellam_t("1x2-home", 0, reuse_probe=True),
                skellam_t("1x2-draw", 1),
                skellam_t("1x2-away", 2),
                skellam_t("ou-over-25", 3, 5),
                skellam_t("ou-under-25", 4, 5),
                skellam_t("btts-yes", 5),
                skellam_t("cs-1-1", 7, 1, 1),
                skellam_t("ah-home-05", 8, -1),
                mask_t("h1x2-win", h1x2_cells(h, "win")),
                mask_t("h1x2-draw", h1x2_cells(h, "draw")),
                mask_t("h1x2-lose", h1x2_cells(h, "lose")),
            ],
        },
        {
            "id": "football-0-3-away-totals",
            "topic": "fa",
            "tag": "ft03",
            "title": "Football 0-3 away + totals",
            "score": (0, 3),
            "expect_label": "0-3",
            "c_m": 80,
            "tickets": [
                skellam_t("1x2-home", 0, reuse_probe=True),
                skellam_t("1x2-away", 2),
                skellam_t("ou-over-25", 3, 5),
                skellam_t("ou-under-25", 4, 5),
                skellam_t("btts-yes", 5),
                skellam_t("btts-no", 6),
                skellam_t("cs-0-3", 7, 0, 3),
                skellam_t("ah-home-05", 8, -1),
                mask_t("h1x2-lose", h1x2_cells(h, "lose")),
            ],
        },
        {
            "id": "football-12-1-overflow",
            "topic": "fo",
            "tag": "ftov",
            "title": "Football 12-1 overflow bucket",
            "score": (12, 1),
            "expect_label": "10+-1",
            "c_m": 80,
            "tickets": [
                skellam_t("1x2-home", 0, reuse_probe=True),
                skellam_t("ou-over-25", 3, 5),
                skellam_t("cs-10-1", 7, 10, 1),
                skellam_t("cs-2-1", 7, 2, 1),
                mask_t("h1x2-win", h1x2_cells(h, "win")),
            ],
        },
    ]


def assert_face_table() -> None:
    """Offline: 2-1 / 1-1 / 0-3 faces match the product example."""
    cases = [
        ((2, 1), 0, 0, 0, 1, 1),
        ((2, 1), 1, 0, 0, 0, 1),
        ((2, 1), 2, 0, 0, 0, 1),
        ((2, 1), 3, 5, 0, 1, 1),
        ((2, 1), 4, 5, 0, 0, 1),
        ((2, 1), 5, 0, 0, 1, 1),
        ((2, 1), 7, 2, 1, 1, 1),
        ((2, 1), 8, -1, 0, 1, 1),
        ((2, 1), 8, -2, 0, 0, 1),
        ((2, 1), 8, -3, 0, 0, 1),
        ((2, 1), 10, -3, 0, 1, 2),  # q=2 half-win → face 1
        ((1, 1), 1, 0, 0, 1, 1),
        ((1, 1), 0, 0, 0, 0, 1),
        ((1, 1), 4, 5, 0, 1, 1),
        ((0, 3), 2, 0, 0, 1, 1),
        ((0, 3), 6, 0, 0, 1, 1),
        ((12, 1), 7, 10, 1, 1, 1),
        ((12, 1), 7, 2, 1, 0, 1),
    ]
    for (h, a), kind, aa, bb, want_hit, tot in cases:
        hit, got_tot = hit_parts(kind, aa, bb, h, a)
        cid = f"face-{h}-{a}-k{kind}"
        fc.expect_eq(cid, hit, want_hit, "parts_hit")
        fc.expect_eq(cid, got_tot, tot, "parts_tot")
        if cid not in " ".join(fc.FINDINGS):
            fc.ok(cid, f"hit={hit}/{got_tot}")
    fc.expect_eq("h1x2-2-1-draw", h1x2_hit(1, "draw", 2, 1), True, "2-1 is AH 1X2 draw −1")
    fc.expect_eq("h1x2-2-1-win", h1x2_hit(1, "win", 2, 1), False, "2-1 is not AH 1X2 win −1")
    fc.expect_eq("h1x2-1-1-lose", h1x2_hit(1, "lose", 1, 1), True, "1-1 is AH 1X2 lose −1")
    fc.expect_eq("h1x2-0-3-lose", h1x2_hit(1, "lose", 0, 3), True, "0-3 is AH 1X2 lose −1")
    fc.expect_eq("h1x2-12-1-win", h1x2_hit(1, "win", 10, 1), True, "10-1 bucket is AH 1X2 win −1")
    lo, hi = quarter_to_halves(-3)
    fc.expect_eq("quarter--075", (lo, hi), (-2, -1), "−0.75 → (−1.0, −0.5)")
    fc.ok(
        "quarter-l1-cu",
        "kind 10 is two LMSR updates; L1 hits 1.4M CU. Legs are ah-home-10 + ah-home-05.",
    )


def run_all_football(kp, stamp: str) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    fc.flow.out("=== football typed lines + settlement ===")
    assert_face_table()
    run_shared_theta(kp, stamp)
    run_isolated_matrix(kp, f"{stamp}i")
    (OUT / "results.json").write_text(json.dumps(fc.RESULTS, indent=2), encoding="utf-8")
    (OUT / "findings.json").write_text(json.dumps(fc.FINDINGS, indent=2), encoding="utf-8")


def main() -> int:
    assert_face_table()
    kp = fc.flow.fund_chain()
    try:
        fc.send(kp, "deposit", owner=str(kp.pubkey()), amount=4000)
        fc.ok("deposit", "4000")
    except Exception as e:
        fc.fail("deposit", str(e))
        return 1
    stamp = str(int(time.time() * 1000) % 10_000_000)
    run_all_football(kp, stamp)
    fc.flow.out("FINDINGS " + str(len(fc.FINDINGS)))
    for row in fc.FINDINGS:
        fc.flow.out("- " + row)
    return 1 if fc.FINDINGS else 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except Exception as e:
        fc.flow.out("FOOTBALL_FATAL " + str(e))
        raise
