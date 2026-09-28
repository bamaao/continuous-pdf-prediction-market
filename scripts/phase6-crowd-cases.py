#!/usr/bin/env python3
"""From-scratch book scale: 5 families, 200 traders, multi-LP cheapest-first auction, two-round vote.

Tickets follow a live desk, not a pile-on: 1X2 / O-U / ranges / slight favorites.
L_max = max_k E_k after that mix. D* = L_max. C_R_cap = n_layers * d_unit
is sized for a spread book (tens of USDC), not 200 shares on one atom.
"""

from __future__ import annotations

import importlib.util
import json
import os
import random
import sys
import time
from pathlib import Path

from solders.keypair import Keypair
from solders.pubkey import Pubkey

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("phase6_family_cases", ROOT / "scripts" / "phase6-family-cases.py")
fc = importlib.util.module_from_spec(spec)
assert spec.loader
sys.modules["phase6_family_cases"] = fc
spec.loader.exec_module(fc)

OUT = ROOT / "tmp" / "phase6-crowd"
COMMITTEE_N = 12
COMMITTEE_M = 7
TRADERS = 200
CLOSE_IN = 480
REPORT = 800
CHALLENGE = 20
DEPOSIT = 25
N_LPS = 10
N_LAYERS = 3
D_UNIT = 30
# Spread book after ~200 mixed tickets: L_max is usually tens, not ~200.
# C_R_cap = 3 * 30 = 90 covers a typical live D* = L_max.
LP_DEPOSIT = 200
COVERAGE_LOG: list[dict] = []


def pk(kp) -> str:
    return str(kp.pubkey())


def fund_wallets(main, n: int, deposit: int, label: str) -> list:
    rpc = fc.flow.Client(fc.flow.RPC, commitment=fc.flow.Confirmed)
    auth = fc.flow.load_kp(fc.flow.AUTH_PATH)
    out = []
    batch = 6
    pending: list = []
    for i in range(n):
        kp = Keypair()
        pending.append(kp)
        if len(pending) < batch and i + 1 < n:
            continue
        ixs = [fc.flow.transfer_sol_ix(main.pubkey(), w.pubkey(), 80_000_000) for w in pending]
        fc.flow.send_ixs(rpc, main, ixs)
        for w in pending:
            try:
                fc.send(w, "create_ata", owner=pk(w))
            except Exception as e:
                if "already in use" not in str(e).lower():
                    raise
        mint_ixs = [fc.flow.mint_to_ix(auth.pubkey(), fc.flow.ata(w.pubkey()), (deposit + 5) * 1_000_000) for w in pending]
        fc.flow.send_ixs(rpc, main, mint_ixs, extra=[auth])
        for w in pending:
            fc.send(w, "deposit", owner=pk(w), amount=deposit)
            out.append(w)
        fc.flow.out(f"funded {label} {len(out)}/{n}")
        pending = []
    return out


def fund_committee(main) -> list:
    rpc = fc.flow.Client(fc.flow.RPC, commitment=fc.flow.Confirmed)
    out = []
    pending = []
    for i in range(COMMITTEE_N):
        kp = Keypair()
        pending.append(kp)
        if len(pending) < 6 and i + 1 < COMMITTEE_N:
            continue
        fc.flow.send_ixs(rpc, main, [fc.flow.transfer_sol_ix(main.pubkey(), w.pubkey(), 80_000_000) for w in pending])
        out.extend(pending)
        pending = []
    fc.flow.out(f"committee {len(out)} wallets")
    return out


def d_required(l_max: int) -> int:
    return int(l_max)


def coverage_of(market: str, spec: dict) -> dict:
    st, info = fc.get(f"/v1/markets/{market}/info")
    info = info if st == 200 else {}
    book = fc.layers_of(market)
    filled_sum = sum(int(q.get("filled") or 0) for q in book.get("quotes") or [])
    l_max = int(info.get("l_max_usdc") or book.get("l_max_usdc") or 0)
    c_r = max(int(info.get("c_r") or 0), int(book.get("c_r") or 0), filled_sum)
    d_star = d_required(l_max)
    n_layers = int(spec.get("n_layers") or N_LAYERS)
    d_unit = int(spec.get("d_unit") or D_UNIT)
    cap = n_layers * d_unit
    return {
        "info": info,
        "l_max": l_max,
        "c_r": c_r,
        "d_star": d_star,
        "c_r_cap": cap,
        "target": min(d_star, cap),
    }


def unit_prem(q: dict) -> float:
    cap = max(int(q.get("capacity") or 1), 1)
    return int(q.get("premium") or 0) / cap


def create_crowd(main, spec: dict, stamp: str) -> str:
    topic = f"{spec['topic']}{stamp}"
    tag = spec.get("tag", "crowd")
    close_ts = int(time.time()) + CLOSE_IN
    market = fc.market_pda(spec["family"], topic, tag)
    body = fc.create_body(
        {
            **spec,
            "c_m": 0,
            "n_layers": spec.get("n_layers", N_LAYERS),
            "d_unit": spec.get("d_unit", D_UNIT),
            "challenge_secs": CHALLENGE,
            "report_window_secs": REPORT,
            "risk_lock_extra": spec.get("risk_lock_extra", 0),
        },
        pk(main),
        topic,
        tag,
        close_ts,
    )
    fc.send(main, **body)
    fc.send(main, "fund_cm", owner=pk(main), market=market, amount=0)
    fc.send(main, "risk_open_book", owner=pk(main), market=market)
    fc.flow.http_json(
        "POST",
        f"{fc.flow.API}/v1/listings",
        {
            "market": market,
            "title": spec.get("title") or cid_title(spec),
            "category": spec.get("category") or "crowd",
            "tags": spec.get("tags") or [spec.get("category") or "crowd"],
            "topic": topic,
            "tag": tag,
            "description": spec["id"],
            "event": spec["id"],
        },
    )
    fc.wait_json(f"/v1/markets/{market}/info", lambda b: b.get("family") == spec["family"] and b.get("n") == spec["n"])
    return market, close_ts, topic, tag


def cid_title(spec: dict) -> str:
    return spec.get("title") or spec["id"]


def auction_posts(lps: list, spec: dict) -> list[tuple]:
    """10 LPs, 4 published layers. Cheapest cluster sits on layer 1."""
    d_unit = int(spec.get("d_unit") or D_UNIT)
    # (lp, layer, capacity, premium) — unit premium = premium/capacity, lowest first.
    return [
        (lps[0], 1, d_unit, 40),
        (lps[1], 1, d_unit, 55),
        (lps[2], 1, d_unit, 70),
        (lps[3], 1, d_unit, 95),
        (lps[4], 2, d_unit, 50),
        (lps[5], 2, d_unit, 80),
        (lps[6], 2, d_unit, 110),
        (lps[7], 3, d_unit, 60),
        (lps[8], 3, d_unit, 100),
        (lps[9], 3, d_unit, 85),
    ]


def open_crowd_auction(cid: str, lps: list, market: str, spec: dict) -> None:
    posts = auction_posts(lps, spec)
    for lp, layer, cap, prem in posts:
        reserved0 = int(fc.vault_of(pk(lp)).get("reserved") or 0)
        try:
            fc.send(lp, "risk_quote", owner=pk(lp), market=market, layer=layer, capacity=cap, premium=prem, profit_share_bps=2_000)
        except Exception as e:
            fc.fail(cid, f"risk_quote lp={pk(lp)[:8]} layer={layer} {e}")
            return
        reserved1 = int(fc.vault_of(pk(lp)).get("reserved") or 0)
        fc.expect_eq(cid, reserved1, reserved0 + cap, f"lp {pk(lp)[:8]} reserved +D")
    want = len(posts)
    try:
        book = fc.wait_json(
            f"/v1/markets/{market}/layers",
            lambda b: len(b.get("quotes") or []) >= want,
        )
    except Exception as e:
        fc.fail(cid, f"layers after {want} quotes {e}")
        return
    layer1 = [q for q in book.get("quotes") or [] if int(q.get("layer") or 0) == 1]
    cheapest = min(layer1, key=unit_prem, default={})
    fc.expect_eq(cid, cheapest.get("lp"), pk(lps[0]), "layer1 cheapest LP")
    cov = coverage_of(market, spec)
    fc.ok(
        cid,
        f"auction posted quotes={want} C_R={int(book.get('c_r') or 0)} "
        f"L_max={cov['l_max']} D*={cov['d_star']} C_R_cap={cov['c_r_cap']}",
    )
    st, catalog = fc.get("/v1/auctions?limit=200")
    if st != 200 or not any(i.get("market") == market for i in (catalog.get("items") or [])):
        fc.fail(cid, "market missing from /v1/auctions")


def assert_cheapest_first(cid: str, market: str, target: int) -> None:
    book = fc.layers_of(market)
    by_layer: dict[int, list] = {}
    for q in book.get("quotes") or []:
        by_layer.setdefault(int(q.get("layer") or 0), []).append(q)
    for layer, qs in sorted(by_layer.items()):
        qs = sorted(qs, key=unit_prem)
        fills = [int(q.get("filled") or 0) for q in qs]
        fc.ok(cid, f"layer{layer} prem={[q.get('premium') for q in qs]} filled={fills}")
        if target < 1 or layer != 1:
            continue
        if max(fills, default=0) < 1:
            fc.fail(cid, f"layer1 unfilled while D* target={target}")
        elif fills and fills[0] < 1:
            fc.fail(cid, f"cheapest layer1 filled=0 while dearer filled={fills}")


def drain_to_coverage(cid: str, lps: list, market: str, spec: dict, label: str) -> dict:
    cov = coverage_of(market, spec)
    target = cov["target"]
    cr0 = cov["c_r"]
    fc.ok(cid, f"{label} size L_max={cov['l_max']} D*={cov['d_star']} target=min(D*,C_R_cap)={target} C_R={cr0}")
    if target < 1:
        fc.ok(cid, f"{label} D*=0 — no extra risk capital required")
        return cov
    by_pk = {pk(lp): lp for lp in lps}
    progressed = True
    rounds = 0
    while progressed and rounds < 80:
        progressed = False
        rounds += 1
        book = fc.layers_of(market)
        cr_now = max(
            int(book.get("c_r") or 0),
            sum(int(q.get("filled") or 0) for q in book.get("quotes") or []),
        )
        if cr_now >= target:
            break
        quotes = sorted(
            book.get("quotes") or [],
            key=lambda q: (int(q.get("layer") or 0), unit_prem(q)),
        )
        for q in quotes:
            if cr_now >= target:
                break
            leftover = int(q.get("capacity") or 0) - int(q.get("filled") or 0)
            if leftover <= 0:
                continue
            lp = by_pk.get(q.get("lp") or "")
            if lp is None:
                continue
            layer = int(q.get("layer") or 1)
            fc.fill_or_skip(lp, cid, pk(lp), market, layer)
        time.sleep(0.6)
        book = fc.layers_of(market)
        cr_after = max(
            int(book.get("c_r") or 0),
            sum(int(q.get("filled") or 0) for q in book.get("quotes") or []),
        )
        if cr_after > cr_now:
            progressed = True
    cov1 = coverage_of(market, spec)
    cr1 = cov1["c_r"]
    if cr1 < cr0:
        fc.fail(cid, f"{label} fill_next shrank C_R {cr0}→{cr1}")
    elif target >= 1 and cr1 < 1:
        fc.fail(cid, f"{label} C_R still 0 after fill (D*={cov1['d_star']})")
    elif target >= 1 and cr1 < max(1, int(target * 0.7)):
        fc.fail(cid, f"{label} C_R={cr1} < 70% of target={target} (L_max={cov1['l_max']} D*={cov1['d_star']})")
    else:
        fc.ok(cid, f"{label} fill_next C_R {cr0}→{cr1} target={target} rounds={rounds}")
    assert_cheapest_first(cid, market, target)
    return cov1


def settle_lps(cid: str, lps: list, market: str) -> None:
    for lp in lps:
        st, risk = fc.get(f"/v1/owners/{pk(lp)}/risk")
        rows = [i for i in (risk.get("items") or []) if i.get("market") == market] if st == 200 else []
        if not rows or all(int(i.get("filled") or 0) == 0 for i in rows):
            fc.ok(cid, f"lp {pk(lp)[:8]} unfilled — skip draw")
            continue
        fc.run_lp_after_settle(lp, cid, market, pk(lp))


def buy_many(cid: str, traders, market: str, tickets: list[dict], on_wave=None) -> list[dict]:
    filled = []
    failed = 0
    first_err = ""
    for i, t in enumerate(traders):
        spec = tickets[i % len(tickets)]
        try:
            if spec.get("skellam") is not None:
                k, a, b = spec["skellam"]
                fc.send(
                    t,
                    "buy_skellam_set",
                    owner=pk(t),
                    market=market,
                    kind=k,
                    value=a,
                    value_b=b,
                    shares=spec.get("shares", 1),
                    nonce=1,
                )
            else:
                fc.send(
                    t,
                    "buy_set",
                    owner=pk(t),
                    market=market,
                    mask=spec["mask"],
                    shares=spec.get("shares", 1),
                    nonce=1,
                )
            filled.append({"trader": t, **spec})
        except Exception as e:
            failed += 1
            if not first_err:
                first_err = str(e)[:220]
            if failed <= 3:
                fc.flow.out(f"{cid} fill#{i} fail {e}")
            if failed >= 8 and len(filled) < 10:
                break
        if (i + 1) % 40 == 0:
            fc.flow.out(f"{cid} fills {len(filled)}/{i+1} fail={failed}")
        if on_wave and (i + 1) % 50 == 0:
            on_wave(i + 1, filled)
    if first_err and len(filled) < TRADERS:
        fc.flow.out(f"{cid} first fill error: {first_err}")
    return filled


def wait_close(close_ts: int) -> None:
    remain = close_ts - time.time() + 1.2
    if remain > 0:
        fc.flow.out(f"wait close {remain:.0f}s")
        time.sleep(remain)


def wait_until(ts: int, label: str) -> None:
    remain = ts - time.time() + 1.4
    if remain > 0:
        fc.flow.out(f"wait {label} {remain:.0f}s")
        time.sleep(remain)


def vote_round(cid: str, committee, market: str, proposed: dict, challenged: dict, n_votes: int, extensions: int) -> dict:
    proposer = committee[0 if extensions == 0 else 2]
    challenger = committee[1 if extensions == 0 else 3]
    fc.send(proposer, "submit_result", owner=pk(proposer), market=market, **proposed)
    rec = fc.wait_json(f"/v1/markets/{market}/resolution", lambda b: b.get("phase") == 1)
    if rec.get("proposer") != pk(proposer):
        fc.fail(cid, f"r{extensions} proposer {rec.get('proposer')}")
    fc.send(challenger, "challenge", owner=pk(challenger), market=market, **challenged)
    rec = fc.wait_json(f"/v1/markets/{market}/resolution", lambda b: b.get("phase") == 2)
    if rec.get("challenger") != pk(challenger):
        fc.fail(cid, f"r{extensions} challenger {rec.get('challenger')}")
    # Keep-proposal slate; skip the challenger so we do not accidentally pass the challenge.
    slate = [m for m in committee if pk(m) != pk(challenger)][:n_votes]
    if len(slate) != n_votes:
        fc.fail(cid, f"r{extensions} voter slate {len(slate)} want {n_votes}")
    for v in slate:
        fc.send(v, "vote", owner=pk(v), market=market, for_challenge=False, extensions=extensions)
    rec = fc.wait_json(
        f"/v1/markets/{market}/resolution",
        lambda b: int(b.get("votes_proposal") or 0) >= n_votes,
    )
    fc.expect_eq(cid, rec.get("votes_proposal"), n_votes, f"r{extensions} votes_proposal")
    return rec


def resolve_two_rounds(cid: str, main, committee, market: str, proposed: dict, challenged: dict) -> dict:
    fc.send(main, "resolve_open", owner=pk(main), market=market)
    rec = fc.wait_json(f"/v1/markets/{market}/resolution", lambda b: b.get("phase") == 0)
    members = rec.get("members") or []
    fc.expect_eq(cid, rec.get("m"), COMMITTEE_M, "quorum m")
    fc.expect_eq(cid, rec.get("n"), COMMITTEE_N, "committee n")
    if len(members) < COMMITTEE_N:
        fc.fail(cid, f"members {len(members)} want {COMMITTEE_N}")
    want = {pk(m) for m in committee}
    got = set(members[:COMMITTEE_N])
    if want != got:
        fc.fail(cid, f"roster mismatch missing={want-got} extra={got-want}")

    rec = vote_round(cid, committee, market, proposed, challenged, n_votes=3, extensions=0)
    vote_end = int(rec.get("vote_end") or 0)
    if vote_end <= 0:
        vote_end = int(time.time()) + CHALLENGE + 2
    wait_until(vote_end, "vote timeout")
    fc.send(committee[0], "finalize", owner=pk(committee[0]), market=market)
    rec = fc.wait_json(
        f"/v1/markets/{market}/resolution",
        lambda b: b.get("phase") == 0 and int(b.get("extensions") or 0) == 1,
    )
    fc.expect_eq(cid, rec.get("extensions"), 1, "ExtendOnce")
    fc.ok(cid, "round1 missed M — window extended, roster snapshot kept")

    rec = vote_round(cid, committee, market, proposed, challenged, n_votes=COMMITTEE_M, extensions=1)
    fc.send(committee[2], "finalize", owner=pk(committee[2]), market=market)
    rec = fc.wait_json(f"/v1/markets/{market}/resolution", lambda b: b.get("phase") == 3)
    fc.expect_eq(cid, rec.get("final_outcome", {}).get("label"), proposed.get("expect_label"), "final x*")
    fc.expect_eq(cid, rec.get("extensions"), 1, "final after second window")
    fc.send(main, "begin_settle", owner=pk(main), market=market)
    return rec


def mask_has_cell(mask_hex: str, cell: int) -> bool:
    raw = bytes.fromhex(mask_hex)
    if cell < 0:
        return False
    i, bit = divmod(int(cell), 8)
    return i < len(raw) and bool(raw[i] & (1 << bit))


def ticket_hits(row: dict, settle_cell: int, hit_pred) -> bool:
    if row.get("mask"):
        return mask_has_cell(row["mask"], settle_cell)
    return bool(hit_pred(row))


def payout_sample(cid: str, market: str, filled: list[dict], settle_cell: int, hit_pred, op: str, extra_kw) -> None:
    hits = [row for row in filled if ticket_hits(row, settle_cell, hit_pred)]
    misses = [row for row in filled if not ticket_hits(row, settle_cell, hit_pred)]
    sample = hits[:20] + misses[:10]
    paid_hits = 0
    zero_miss = 0
    for row in sample:
        t = row["trader"]
        vault0 = int(fc.vault_of(pk(t)).get("available") or 0)
        try:
            if row.get("skellam") is not None:
                k, a, b = row["skellam"]
                fc.send(t, "payout_skellam", owner=pk(t), market=market, kind=k, value=a, value_b=b)
            else:
                fc.send(t, "payout", owner=pk(t), market=market, mask=row["mask"], **extra_kw)
        except Exception as e:
            fc.fail(cid, f"payout {pk(t)[:8]} {e}")
            continue
        vault1 = int(fc.vault_of(pk(t)).get("available") or 0)
        delta = vault1 - vault0
        if ticket_hits(row, settle_cell, hit_pred):
            if delta <= 0:
                fc.fail(cid, f"winner paid {delta}")
            else:
                paid_hits += 1
        else:
            if delta != 0:
                fc.fail(cid, f"loser paid {delta}")
            else:
                zero_miss += 1
    fc.ok(cid, f"payout sample winners={paid_hits} losers={zero_miss} pool hit={len(hits)} miss={len(misses)} cell={settle_cell}")


def run_family(main, committee, traders, lps, spec: dict, stamp: str) -> None:
    cid = spec["id"]
    fc.flow.out(f"=== {cid} {TRADERS} traders / {COMMITTEE_N} committee / {len(lps)} LPs ===")
    try:
        market, close_ts, _, _ = create_crowd(main, spec, stamp)
    except Exception as e:
        fc.fail(cid, f"create {e}")
        return
    open_crowd_auction(cid, lps, market, spec)

    def mid_wave(n, _filled):
        drain_to_coverage(cid, lps, market, spec, f"mid@{n}")

    tickets = spec["tickets"](market)
    log_book_shape(cid, tickets, spec["n"])
    filled = buy_many(cid, traders, market, tickets, on_wave=mid_wave)
    cov = drain_to_coverage(cid, lps, market, spec, "post-trade")
    if len(filled) < spec["min_fills"]:
        fc.fail(cid, f"only {len(filled)} fills want ≥{spec['min_fills']}")
        return
    try:
        info = fc.wait_json(
            f"/v1/markets/{market}/info",
            lambda b: int(b.get("traders") or 0) >= min(len(filled), spec["min_fills"]) // 2,
            timeout=40,
        )
        fc.ok(cid, f"indexed traders={info.get('traders')} fills={len(filled)}")
        if int(info.get("traders") or 0) < spec["min_fills"] // 2:
            fc.fail(cid, f"indexed traders {info.get('traders')} after {len(filled)} fills")
    except Exception as e:
        fc.fail(cid, f"trader index {e}")
    wait_close(close_ts)
    try:
        resolve_two_rounds(cid, main, committee, market, spec["proposed"], spec["challenged"])
        settled = fc.wait_json(f"/v1/markets/{market}/info", lambda b: int(b.get("board_phase") or 0) >= 1)
        L = int(settled.get("liability") or 0)
        c_max = int(settled.get("c_max_usdc") or 0)
        rho = int(settled.get("rho_bps") or 0)
        r_net = int(settled.get("r_net") or 0)
        c_m = 0
        c_r = int(settled.get("c_r") or 0)
        c_p = int(settled.get("c_p_alloc") or 0)
        l_max = int(settled.get("l_max_usdc") or cov["l_max"])
        fc.expect_eq(cid, c_max, r_net + c_r + c_p, "C_max")
        if L > l_max + 1:
            fc.fail(cid, f"settle L={L} > L_max={l_max}")
        if cov["d_star"] >= 1 and c_r < 1:
            fc.fail(cid, f"C_R={c_r} after auction+settle D*={cov['d_star']}")
        if L > 0 and c_max >= L:
            fc.expect_eq(cid, rho, 10000, "ρ")
        elif L > 0:
            want = int(c_max * 10000 / L)
            fc.expect_near(cid, rho, want, 50, "ρ=C_max/L")
        row = {
            "id": cid,
            "market": market,
            "fills": len(filled),
            "l_max": l_max,
            "d_star": d_required(l_max),
            "c_r": c_r,
            "c_r_cap": cov["c_r_cap"],
            "L": L,
            "c_max": c_max,
            "rho_bps": rho,
            "final": settled.get("final_result"),
        }
        COVERAGE_LOG.append(row)
        fc.ok(cid, f"settled L={L}≤L_max={l_max} D*={row['d_star']} C_R={c_r}/{cov['c_r_cap']} C_max={c_max} ρ={rho} x*={settled.get('final_result')}")
        settle_lps(cid, lps, market)
    except Exception as e:
        fc.fail(cid, f"committee settle {e}")
        return
    payout_sample(cid, market, filled, int(settled.get("settle_cell") or 0), spec["hit"], spec["payout_op"], spec.get("payout_kw") or {})


def book_take(tickets: list[dict], n: int = TRADERS, seed: int = 1) -> list[dict]:
    if len(tickets) < n:
        raise RuntimeError(f"book short {len(tickets)} < {n}")
    rng = random.Random(seed)
    out = list(tickets)
    rng.shuffle(out)
    return out[:n]


def clip_range(lo: int, hi: int, n: int) -> list[int]:
    return list(range(max(0, lo), min(n, hi + 1)))


def cells_of(row: dict, n: int) -> list[int]:
    if row.get("cells") is not None:
        return [int(c) for c in row["cells"] if 0 <= int(c) < n]
    if row.get("mask"):
        raw = bytes.fromhex(row["mask"])
        return [i for i in range(n) if i // 8 < len(raw) and raw[i // 8] & (1 << (i % 8))]
    return []


def log_book_shape(cid: str, tickets: list[dict], n: int) -> None:
    singleton = [0] * n
    covered = [0] * n
    for row in tickets:
        cells = cells_of(row, n)
        if len(cells) == 1:
            singleton[cells[0]] += 1
        for c in cells:
            covered[c] += 1
    top_s = sorted(enumerate(singleton), key=lambda x: -x[1])[:4]
    top_c = sorted(enumerate(covered), key=lambda x: -x[1])[:4]
    fc.ok(cid, f"book mix singleton_top={top_s} cover_top={top_c} n={n}")
    if n >= 8 and max(singleton) >= int(TRADERS * 0.35):
        fc.fail(cid, f"unrealistic singleton pile {max(singleton)}/{TRADERS} on one cell")


def handicap_home_minus1() -> list[int]:
    return [i * 11 + j for i in range(11) for j in range(11) if i - j > 1]


def draw_cells() -> list[int]:
    return [i * 11 + i for i in range(11)]


def away_cells() -> list[int]:
    return [i * 11 + j for i in range(11) for j in range(11) if i < j]


def over25_cells() -> list[int]:
    return [i * 11 + j for i in range(11) for j in range(11) if i + j >= 3]


def under25_cells() -> list[int]:
    return [i * 11 + j for i in range(11) for j in range(11) if i + j <= 2]


def btts_yes_cells() -> list[int]:
    return [i * 11 + j for i in range(1, 11) for j in range(1, 11)]


def btts_no_cells() -> list[int]:
    return [i * 11 + j for i in range(11) for j in range(11) if i == 0 or j == 0]


def football_tickets(_market: str) -> list[dict]:
    # Match-desk mix: 1X2 + O/U + a few exacts + one AH + BTTS. No 50-home pile.
    exacts = [
        (2, 1), (1, 0), (1, 1), (0, 0), (0, 1), (3, 1), (2, 0), (0, 2), (3, 2),
        (4, 1), (1, 2), (2, 2), (3, 0), (0, 3), (4, 0), (3, 3), (2, 3), (4, 2),
    ]
    tickets: list[dict] = []
    tickets += [{"skellam": (0, 0, 0), "line": "home", "cells": fc.home_cells()}] * 38
    tickets += [{"skellam": (1, 0, 0), "line": "draw", "cells": draw_cells()}] * 28
    tickets += [{"skellam": (2, 0, 0), "line": "away", "cells": away_cells()}] * 30
    tickets += [{"skellam": (3, 5, 0), "line": "over25", "cells": over25_cells()}] * 28
    tickets += [{"skellam": (4, 5, 0), "line": "under25", "cells": under25_cells()}] * 26
    tickets += [{"skellam": (7, a, b), "line": f"exact-{a}-{b}", "cells": [a * 11 + b]} for a, b in exacts]
    tickets += [{"mask": fc.mask_hex(handicap_home_minus1(), 121), "line": "ah-1-home", "skellam": None, "cells": handicap_home_minus1()}] * 14
    tickets += [{"mask": fc.mask_hex(btts_yes_cells(), 121), "line": "btts-yes", "skellam": None, "cells": btts_yes_cells()}] * 12
    tickets += [{"mask": fc.mask_hex(btts_no_cells(), 121), "line": "btts-no", "skellam": None, "cells": btts_no_cells()}] * 6
    return book_take(tickets, seed=5)


def binary_tickets() -> list[dict]:
    # Slight favorite, not 70/30 everyone-on-YES.
    tickets = [{"mask": fc.mask_hex([1], 2), "side": "yes", "cells": [1]}] * 108
    tickets += [{"mask": fc.mask_hex([0], 2), "side": "no", "cells": [0]}] * 92
    return book_take(tickets, seed=11)


def gauss_tickets(_market: str) -> list[dict]:
    # CPI-style: a few pins, a band, over/under strikes that miss the mode, two tails.
    _st, prior = fc.get("/v1/prior?family=1&milli=true&mu=2400&sigma=350&x_min=-2000&x_max=12000&n=32")
    peak = int((prior or {}).get("peak") or 0)
    n = 32
    tickets: list[dict] = []

    def add(cells: list[int], side: str, k: int) -> None:
        if not cells or k < 1:
            return
        tickets.extend([{"mask": fc.mask_hex(cells, n), "side": side, "cells": cells, "peak": peak}] * k)

    add([peak], "pin", 16)
    add(clip_range(peak - 1, peak + 1, n), "near", 22)
    add(clip_range(peak - 3, peak + 3, n), "band", 28)
    add(clip_range(peak + 1, n - 1, n), "over", 42)
    add(clip_range(0, peak - 1, n), "under", 38)
    add(clip_range(peak + 8, n - 1, n), "right-tail", 18)
    add(clip_range(0, peak - 8, n), "left-tail", 16)
    others = [i for i in range(n) if abs(i - peak) >= 2]
    for c in others[:20]:
        add([c], f"bin-{c}", 1)
    return book_take(tickets, seed=21)


def logn_tickets(_market: str) -> list[dict]:
    _st, prior = fc.get("/v1/prior?family=2&milli=true&mu=11080&sigma=250&x_min=10000000&x_max=250000000&n=8")
    peak = int((prior or {}).get("peak") or 0)
    n = 8
    tickets: list[dict] = []

    def add(cells: list[int], side: str, k: int) -> None:
        if not cells or k < 1:
            return
        tickets.extend([{"mask": fc.mask_hex(cells, n), "side": side, "cells": cells, "peak": peak}] * k)

    add([peak], "pin", 18)
    add(clip_range(peak - 1, peak + 1, n), "near", 24)
    add(clip_range(peak + 1, n - 1, n), "over", 48)
    add(clip_range(0, peak - 1, n), "under", 47)
    right = clip_range(peak + 2, n - 1, n) or [n - 1]
    left = clip_range(0, max(0, peak - 2), n) or [0]
    add(right, "right-tail", 22)
    add(left, "left-tail", 20)
    for c in range(n):
        if c != peak:
            add([c], f"bin-{c}", 3)
    return book_take(tickets, seed=31)


def dirichlet_tickets(_market: str) -> list[dict]:
    # Election desk: favorite, two chasers, field, plus "anyone but fav" / top-2.
    tickets = [{"mask": fc.mask_hex([0], 4), "side": "a0", "cells": [0]}] * 44
    tickets += [{"mask": fc.mask_hex([1], 4), "side": "a1", "cells": [1]}] * 38
    tickets += [{"mask": fc.mask_hex([2], 4), "side": "a2", "cells": [2]}] * 32
    tickets += [{"mask": fc.mask_hex([3], 4), "side": "a3", "cells": [3]}] * 28
    tickets += [{"mask": fc.mask_hex([1, 2, 3], 4), "side": "not-fav", "cells": [1, 2, 3]}] * 22
    tickets += [{"mask": fc.mask_hex([0, 1], 4), "side": "top2", "cells": [0, 1]}] * 20
    tickets += [{"mask": fc.mask_hex([2, 3], 4), "side": "bottom2", "cells": [2, 3]}] * 16
    return book_take(tickets, seed=41)


def football_hit(row: dict) -> bool:
    line = row.get("line") or ""
    if line in ("home", "over25", "exact-2-1", "btts-yes"):
        return True
    if line.startswith("exact-"):
        return line == "exact-2-1"
    return False


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    kp = fc.flow.fund_chain()
    rpc = fc.flow.Client(fc.flow.RPC, commitment=fc.flow.Confirmed)
    for _ in range(20):
        bal = rpc.get_balance(kp.pubkey()).value
        if bal >= 25_000_000_000:
            break
        try:
            sig = rpc.request_airdrop(kp.pubkey(), 2_000_000_000).value
            rpc.confirm_transaction(sig, fc.flow.Confirmed)
        except Exception as e:
            fc.flow.out(f"airdrop {e}")
    auth = fc.flow.load_kp(fc.flow.AUTH_PATH)
    try:
        fc.flow.send_ixs(rpc, kp, [fc.flow.mint_to_ix(auth.pubkey(), fc.flow.ata(kp.pubkey()), 80_000_000_000)], extra=[auth])
    except Exception as e:
        fc.flow.out(f"mint main {e}")
    try:
        fc.send(kp, "deposit", owner=pk(kp), amount=20_000)
    except Exception as e:
        fc.flow.out(f"main deposit {e}")
    stamp = str(int(time.time()) % 10_000_000)
    committee = fund_committee(kp)
    try:
        fc.send(kp, "init_committee", owner=pk(kp), members=[pk(m) for m in committee], m=COMMITTEE_M)
    except Exception:
        fc.send(kp, "set_roster", owner=pk(kp), members=[pk(m) for m in committee], m=COMMITTEE_M)
    traders = fund_wallets(kp, TRADERS, DEPOSIT, "traders")
    lps = fund_wallets(kp, N_LPS, LP_DEPOSIT, "lps")
    (OUT / "roster.json").write_text(
        json.dumps(
            {
                "committee": [pk(m) for m in committee],
                "traders": [pk(t) for t in traders],
                "lps": [pk(m) for m in lps],
                "m": COMMITTEE_M,
            },
            indent=2,
        ),
        encoding="utf-8",
    )

    cases = [
        {
            "id": "crowd-bernoulli",
            "family": 4,
            "op": "create_bernoulli",
            "topic": "cbe",
            "tag": "yes",
            "n": 2,
            "c_m": 40,
            "n_layers": N_LAYERS,
            "d_unit": D_UNIT,
            "title": "Crowd Bernoulli YES",
            "category": "binary",
            "tags": ["binary", "test"],
            "min_fills": 180,
            "tickets": lambda _m: binary_tickets(),
            "proposed": {"family": 4, "kind": 4, "value": 1, "value_b": 0, "expect_label": "YES"},
            "challenged": {"family": 4, "kind": 4, "value": 0, "value_b": 0},
            "hit": lambda row: row.get("side") == "yes",
            "payout_op": "payout",
        },
        {
            "id": "crowd-gaussian",
            "family": 1,
            "op": "create_gaussian",
            "topic": "cga",
            "tag": "yoy",
            "n": 32,
            "milli": True,
            "x_min": -2000,
            "x_max": 12000,
            "mu": 2400,
            "sigma": 350,
            "c_m": 40,
            "n_layers": N_LAYERS,
            "d_unit": D_UNIT,
            "title": "Crowd Gaussian μ",
            "category": "macro",
            "tags": ["macro", "cpi"],
            "min_fills": 180,
            "tickets": gauss_tickets,
            "proposed": {"family": 1, "kind": 1, "value": 2400, "value_b": 0, "milli": True, "expect_label": "2.400"},
            "challenged": {"family": 1, "kind": 1, "value": 1000, "value_b": 0, "milli": True},
            "hit": lambda row: False,
            "payout_op": "payout",
        },
        {
            "id": "crowd-lognormal",
            "family": 2,
            "op": "create_lognormal",
            "topic": "cln",
            "tag": "spot",
            "n": 8,
            "milli": True,
            "x_min": 10_000_000,
            "x_max": 250_000_000,
            "mu": 11080,
            "sigma": 250,
            "c_m": 40,
            "n_layers": N_LAYERS,
            "d_unit": D_UNIT,
            "title": "Crowd Lognormal",
            "category": "price",
            "tags": ["price", "btc"],
            "min_fills": 180,
            "tickets": logn_tickets,
            "proposed": {"family": 2, "kind": 1, "value": 64880, "value_b": 0, "expect_label": "64880"},
            "challenged": {"family": 2, "kind": 1, "value": 50000, "value_b": 0},
            "hit": lambda row: False,
            "payout_op": "payout",
        },
        {
            "id": "crowd-dirichlet",
            "family": 3,
            "op": "create_dirichlet",
            "topic": "cdi",
            "tag": "win",
            "n": 4,
            "c_m": 30,
            "n_layers": N_LAYERS,
            "d_unit": D_UNIT,
            "title": "Crowd Dirichlet",
            "category": "election",
            "tags": ["election"],
            "min_fills": 180,
            "tickets": dirichlet_tickets,
            "proposed": {"family": 3, "kind": 2, "value": 0, "value_b": 0, "expect_label": "atom 0"},
            "challenged": {"family": 3, "kind": 2, "value": 1, "value_b": 0},
            "hit": lambda row: row.get("side") == "a0",
            "payout_op": "payout",
        },
        {
            "id": "crowd-football",
            "family": 0,
            "op": "create_skellam",
            "topic": "cfb",
            "tag": "ft",
            "n": 121,
            "milli": True,
            "lambda_home": 1400,
            "lambda_away": 1100,
            "c_m": 25,
            "n_layers": N_LAYERS,
            "d_unit": D_UNIT,
            "title": "Crowd Football 2-1",
            "category": "football",
            "tags": ["football", "test"],
            "min_fills": 180,
            "tickets": football_tickets,
            "proposed": {"family": 0, "kind": 0, "value": 2, "value_b": 1, "expect_label": "2-1"},
            "challenged": {"family": 0, "kind": 0, "value": 1, "value_b": 1},
            "hit": football_hit,
            "payout_op": "payout_skellam",
        },
    ]
    only = os.environ.get("CASES", "all")
    for spec in cases:
        if only != "all" and spec["id"] != only:
            continue
        run_family(kp, committee, traders, lps, spec, stamp)

    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "coverage.json").write_text(json.dumps(COVERAGE_LOG, indent=2), encoding="utf-8")
    (OUT / "findings.json").write_text(json.dumps(fc.FINDINGS, indent=2), encoding="utf-8")
    fc.flow.out("COVERAGE " + json.dumps(COVERAGE_LOG))
    fc.flow.out("FINDINGS " + str(len(fc.FINDINGS)))
    for row in fc.FINDINGS:
        fc.flow.out("- " + row)
    return 1 if fc.FINDINGS else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as e:
        fc.flow.out("CROWD_FATAL " + str(e))
        raise
