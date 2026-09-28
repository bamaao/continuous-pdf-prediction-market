#!/usr/bin/env python3
"""Local Market API fixture for Phase 6 Playwright. Not the ledger."""

from http.server import BaseHTTPRequestHandler, HTTPServer
from urllib.parse import parse_qs, urlparse
import json
import os

MARKET = "Seed111111111111111111111111111111111111111"
SKELLAM = "Skel111111111111111111111111111111111111111"
N = 8
N2 = 121
P0 = "1" + "0" * 18  # dummy raw strings; UI only needs shape


def book(market: str, family: int, n: int):
    zeros = ["0"] * n
    p0 = [P0] * n
    return {
        "market": market,
        "family": family,
        "status": 1,
        "n": n,
        "slot": 7,
        "beta_raw": "100",
        "p0_raw": p0,
        "theta_raw": zeros,
        "exposure_raw": zeros,
        "trading_revenue": 4,
        "premium_payable": 0,
        "c_m": 5,
        "c_r": 0,
        "fee_bps": 0,
    }


def preview(market: str, n: int):
    return {
        "market": market,
        "slot": 7,
        "n": n,
        "p_s_raw": "1",
        "p_s_bps": 1250,
        "c_s_raw": "1",
        "c_s_usdc": 1,
        "coverage_bps": 9000,
        "rho_hat_bps": 9000,
        "l_max_usdc": 10,
        "c_max_usdc": 9,
        "r_net": 4,
        "fee_bps": 0,
        "fee_usdc": 0,
        "pay_usdc": 1,
        "face_usdc": 1,
        "payout_if_hit_usdc": 1,
        "payout_if_miss_usdc": 0,
        "net_if_hit": 0,
        "ev_if_p_s": -1,
    }


FAMILIES = {0: "skellam", 1: "gaussian", 2: "lognormal", 3: "dirichlet", 4: "bernoulli"}
STATUSES = {1: "trading", 2: "halted", 3: "settled", 4: "void"}


def catalog_rows():
    rows = [
        {
            "market": MARKET,
            "family": 1,
            "status": 1,
            "n": N,
            "slot": 9,
            "traders": 3,
            "stake_usdc": 12,
            "l_max_usdc": 10,
            "c_r": 0,
        },
        {
            "market": SKELLAM,
            "family": 0,
            "status": 1,
            "n": N2,
            "slot": 8,
            "traders": 11,
            "stake_usdc": 40,
            "l_max_usdc": 10,
            "c_r": 2,
        },
    ]
    for i in range(3, 8):
        rows.append(
            {
                "market": f"Demo{i}11111111111111111111111111111111111111",
                "family": 1 + (i % 3),
                "status": 1,
                "n": 8,
                "slot": i,
                "traders": i,
                "stake_usdc": i * 2,
                "l_max_usdc": 10,
                "c_r": 0,
            }
        )
    return rows


def catalog(qs: str):
    q = parse_qs(qs)
    needle = (q.get("q", [""])[0] or "").strip().lower()
    family = q.get("family", [None])[0]
    status = q.get("status", [None])[0]
    page = max(1, int(q.get("page", ["1"])[0] or 1))
    limit = min(100, max(1, int(q.get("limit", ["20"])[0] or 20)))
    rows = []
    for row in catalog_rows():
        if family not in (None, "") and str(row["family"]) != str(family):
            continue
        if status not in (None, "") and str(row["status"]) != str(status):
            continue
        hay = " ".join(
            [
                row["market"].lower(),
                FAMILIES.get(row["family"], ""),
                STATUSES.get(row["status"], ""),
            ]
        )
        if needle and needle not in hay:
            continue
        rows.append(row)
    rows.sort(key=lambda r: (-r["slot"], r["market"]))
    total = len(rows)
    pages = 1 if total == 0 else (total + limit - 1) // limit
    start = (page - 1) * limit
    return {
        "page": page,
        "limit": limit,
        "total": total,
        "pages": pages,
        "q": needle,
        "family": int(family) if family not in (None, "") else None,
        "status": int(status) if status not in (None, "") else None,
        "items": rows[start : start + limit],
    }


def which(path: str):
    if SKELLAM in path:
        return SKELLAM, 0, N2
    return MARKET, 1, N


def cells(n: int):
    return [{"cell": i, "p_bps": 1250 + (400 if i == 0 else 0), "e": 10 if i == 0 else 0} for i in range(n)]


def info(market: str, family: int, n: int):
    return {
        "market": market,
        "family": family,
        "status": 1,
        "n": n,
        "slot": 7,
        "traders": 3 if family == 1 else 11,
        "tickets": 5 if family == 1 else 18,
        "stake_usdc": 12 if family == 1 else 40,
        "trading_revenue": 4,
        "l_max_usdc": 10,
        "c_r": 2 if family == 0 else 0,
        "c_m": 5,
        "r_net": 4,
        "c_max_usdc": 9 if family == 1 else 11,
        "coverage_bps": 9000,
        "rho_hat_bps": 9000,
        "fee_bps": 0,
        "cells": cells(n),
    }


class Handler(BaseHTTPRequestHandler):
    def log_message(self, fmt, *args):
        return

    def _cors(self):
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "*")

    def do_OPTIONS(self):
        self.send_response(204)
        self._cors()
        self.end_headers()

    def do_GET(self):
        path = self.path.split("?")[0]
        if path == "/v1/health":
            body = {"ok": True, "slot": 7}
        elif path == "/v1/markets":
            body = catalog(urlparse(self.path).query)
        elif path.endswith("/info"):
            mid, family, n = which(path)
            body = info(mid, family, n)
        elif path.endswith("/book"):
            mid, family, n = which(path)
            body = book(mid, family, n)
        elif path.endswith("/pdf"):
            mid, family, n = which(path)
            body = {"market": mid, "slot": 7, "cells": cells(n)}
        elif path.endswith("/preview") or "/quote" in path:
            mid, _family, n = which(path)
            body = preview(mid, n)
        elif "/owners/" in path and path.endswith("/positions"):
            owner = path.split("/owners/")[1].split("/")[0]
            body = {
                "owner": owner,
                "page": 1,
                "limit": 20,
                "total": 2,
                "pages": 1,
                "claimable": 1,
                "paid_tickets": 1,
                "paid_usdc": 3,
                "net_claimed": 2,
                "items": [
                    {
                        "position": "PosPaid11111111111111111111111111111111111",
                        "market": MARKET,
                        "family": 1,
                        "status": 3,
                        "board_phase": 1,
                        "set_hash": "aa",
                        "shares": 2,
                        "cost_paid": 1,
                        "claimed": True,
                        "paid_usdc": 3,
                        "net_usdc": 2,
                        "rho_hat_bps": 10000,
                        "settle_cell": 0,
                        "prompt": "paid",
                    },
                    {
                        "position": "PosOpen11111111111111111111111111111111111",
                        "market": SKELLAM,
                        "family": 0,
                        "status": 1,
                        "board_phase": 0,
                        "set_hash": "bb",
                        "shares": 1,
                        "cost_paid": 1,
                        "claimed": False,
                        "paid_usdc": 0,
                        "net_usdc": None,
                        "rho_hat_bps": 0,
                        "settle_cell": 0,
                        "prompt": "unclaimed_settle",
                    },
                ],
            }
        else:
            self.send_response(404)
            self._cors()
            self.end_headers()
            return
        raw = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self._cors()
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_POST(self):
        if self.path.split("?")[0] != "/v1/compose":
            self.send_response(404)
            self._cors()
            self.end_headers()
            return
        n = int(self.headers.get("Content-Length", "0"))
        self.rfile.read(n)
        body = {
            "program_id": "VaULt11111111111111111111111111111111111111",
            "keys": [],
            "data_b64": "AA==",
        }
        raw = json.dumps(body).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self._cors()
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)


if __name__ == "__main__":
    port = int(os.environ.get("PORT", "8080"))
    HTTPServer(("127.0.0.1", port), Handler).serve_forever()
