#!/usr/bin/env python3
"""Fund localnet wallet, then drive connect → SIWS → deposit → create → buy → auction."""

from __future__ import annotations

import base64
import hashlib
import json
import sys
import time
import urllib.error
import urllib.request
from pathlib import Path

from playwright.sync_api import TimeoutError as PwTimeout
from playwright.sync_api import expect, sync_playwright
from solders.compute_budget import request_heap_frame, set_compute_unit_limit
from solders.instruction import AccountMeta, Instruction
from solders.keypair import Keypair
from solders.pubkey import Pubkey
from solders.system_program import TransferParams, transfer
from solders.transaction import Transaction
from solana.rpc.api import Client
from solana.rpc.commitment import Confirmed
from solana.rpc.types import TxOpts

ROOT = Path(__file__).resolve().parents[1]
BASE = "http://127.0.0.1:3000"
API = "http://127.0.0.1:8080"
RPC = "http://127.0.0.1:8899"
OUT = ROOT / "tmp" / "phase6-flow"
KP_PATH = ROOT / "tmp" / "phase6-id.json"
AUTH_PATH = ROOT / "fixtures" / "usdc-mint-authority.json"

USDC = Pubkey.from_string("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")
TOKEN = Pubkey.from_string("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
ATA_PROG = Pubkey.from_string("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
SYS = Pubkey.from_string("11111111111111111111111111111111")
VAULT = Pubkey.from_string("VaULt11111111111111111111111111111111111111")

FINDINGS: list[str] = []


def out(msg: str) -> None:
    print(msg.encode("utf-8", "replace").decode("utf-8", "replace"))


def shot(page, name: str) -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    page.screenshot(path=str(OUT / f"{name}.png"), full_page=True)


def load_kp(path: Path) -> Keypair:
    return Keypair.from_bytes(bytes(json.loads(path.read_text())))


def ata(owner: Pubkey) -> Pubkey:
    return Pubkey.find_program_address([bytes(owner), bytes(TOKEN), bytes(USDC)], ATA_PROG)[0]


def vault_config() -> Pubkey:
    return Pubkey.find_program_address([b"vault"], VAULT)[0]


def http_json(method: str, url: str, body=None, timeout=30):
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(url, data=data, method=method, headers={"content-type": "application/json"})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            raw = r.read()
            return r.status, json.loads(raw) if raw else {}
    except urllib.error.HTTPError as e:
        raw = e.read()
        try:
            return e.code, json.loads(raw) if raw else {"error": str(e)}
        except json.JSONDecodeError:
            return e.code, {"error": raw.decode("utf-8", "replace")}


def compose(op: str, **kw):
    status, body = http_json("POST", f"{API}/v1/compose", {"op": op, **kw})
    if status != 200:
        raise RuntimeError(f"compose {op} {status} {body}")
    return Instruction(
        Pubkey.from_string(body["program_id"]),
        base64.b64decode(body["data_b64"]),
        [AccountMeta(Pubkey.from_string(k["pubkey"]), k["is_signer"], k["is_writable"]) for k in body["keys"]],
    )


def send_ixs(rpc: Client, payer: Keypair, ixs, extra=None) -> str:
    extra = extra or []
    bh = rpc.get_latest_blockhash(Confirmed).value.blockhash
    heap = request_heap_frame(256 * 1024)
    cu = set_compute_unit_limit(1_400_000)
    signers = [payer] + [s for s in extra if s.pubkey() != payer.pubkey()]
    tx = Transaction.new_signed_with_payer([heap, cu, *ixs], payer.pubkey(), signers, bh)
    sig = rpc.send_transaction(tx, opts=TxOpts(skip_preflight=True, preflight_commitment=Confirmed)).value
    rpc.confirm_transaction(sig, Confirmed)
    parsed = rpc.get_transaction(sig, commitment=Confirmed).value
    err = None if parsed is None else parsed.transaction.meta.err
    if err is not None:
        logs = parsed.transaction.meta.log_messages or []
        raise RuntimeError(f"tx {sig} failed {err} | " + " · ".join(logs[-8:]))
    return str(sig)


def transfer_sol_ix(src: Pubkey, dest: Pubkey, lamports: int) -> Instruction:
    return transfer(TransferParams(from_pubkey=src, to_pubkey=dest, lamports=lamports))


def mint_to_ix(authority: Pubkey, dest: Pubkey, amount: int) -> Instruction:
    return Instruction(
        TOKEN,
        bytes([7]) + amount.to_bytes(8, "little"),
        [
            AccountMeta(USDC, False, True),
            AccountMeta(dest, False, True),
            AccountMeta(authority, True, False),
        ],
    )


def vault_init_ix(payer: Pubkey) -> Instruction:
    cfg = vault_config()
    disc = hashlib.sha256(b"global:initialize").digest()[:8]
    return Instruction(
        VAULT,
        disc,
        [
            AccountMeta(payer, True, True),
            AccountMeta(USDC, False, False),
            AccountMeta(cfg, False, True),
            AccountMeta(ata(cfg), False, True),
            AccountMeta(TOKEN, False, False),
            AccountMeta(ATA_PROG, False, False),
            AccountMeta(SYS, False, False),
        ],
    )


def fund_chain() -> Keypair:
    if not KP_PATH.exists():
        KP_PATH.parent.mkdir(parents=True, exist_ok=True)
        kp = Keypair()
        KP_PATH.write_text(json.dumps(list(bytes(kp))))
    kp = load_kp(KP_PATH)
    rpc = Client(RPC, commitment=Confirmed)
    print("rpc slot", rpc.get_slot().value)
    bal = rpc.get_balance(kp.pubkey()).value
    print("payer", kp.pubkey(), "lamports", bal)
    if bal < 2_000_000_000:
        for _ in range(4):
            try:
                sig = rpc.request_airdrop(kp.pubkey(), 2_000_000_000).value
                rpc.confirm_transaction(sig, Confirmed)
            except Exception as e:
                print("airdrop", e)
            bal = rpc.get_balance(kp.pubkey()).value
            if bal >= 2_000_000_000:
                break
        print("lamports after airdrop", bal)
    if bal < 50_000_000:
        raise RuntimeError(f"payer has no SOL ({bal}); localnet faucet failed")

    mint = rpc.get_account_info(USDC).value
    if mint is None:
        raise RuntimeError("Circle USDC mint missing on localnet — validator fixture not loaded")

    try:
        send_ixs(rpc, kp, [compose("create_ata", owner=str(kp.pubkey()))])
        print("ata ok", ata(kp.pubkey()))
    except Exception as e:
        print("create_ata", e)

    auth = load_kp(AUTH_PATH)
    dest = ata(kp.pubkey())
    info = rpc.get_account_info(dest).value
    have = 0
    if info and len(info.data) >= 72:
        have = int.from_bytes(bytes(info.data)[64:72], "little")
    print("ata usdc", have)
    if have < 1_000_000:
        sig = send_ixs(rpc, kp, [mint_to_ix(auth.pubkey(), dest, 1_000_000_000)], extra=[auth])
        print("mint", sig)

    cfg = vault_config()
    if rpc.get_account_info(cfg).value is None:
        try:
            sig = send_ixs(rpc, kp, [compose("vault_init", owner=str(kp.pubkey()))])
            print("vault_init compose", sig)
        except Exception as e:
            print("vault_init compose", e)
            sig = send_ixs(rpc, kp, [vault_init_ix(kp.pubkey())])
            print("vault_init raw", sig)
    else:
        print("vault already live", cfg)
    try:
        send_ixs(rpc, kp, [compose("init_committee", owner=str(kp.pubkey()), members=[str(kp.pubkey())], m=1)])
        print("committee init", kp.pubkey())
    except Exception as e:
        print("committee init", e)
    return kp


def connect_wallet(page, secret: list[int]) -> None:
    page.add_init_script(
        f"window.__cpmPendingKeypair = {json.dumps(secret)};"
        "try{localStorage.setItem('walletName', JSON.stringify('Localnet keypair'));}catch(e){}"
    )
    page.goto(BASE + "/", wait_until="networkidle")
    # next dev compiles verify after challenge and drops the in-memory nonce.
    page.evaluate(
        """() => fetch('/api/siws/verify', {method:'POST', headers:{'content-type':'application/json'}, body:'{}'}).catch(() => {})"""
    )
    page.wait_for_timeout(400)
    if page.get_by_role("button", name="Select Wallet").count():
        page.get_by_role("button", name="Select Wallet").click()
        page.locator(".wallet-adapter-modal-list button").first.click()
        page.wait_for_timeout(800)
    if page.get_by_role("button", name="Sign in").count():
        page.get_by_role("button", name="Sign in").click()
        try:
            page.get_by_role("button", name="Open session").wait_for(timeout=15_000)
        except PwTimeout:
            FINDINGS.append("SIWS did not reach Open session: " + page.locator("body").inner_text()[:400])
            shot(page, "siws-fail")
            raise
    elif page.get_by_role("button", name="Open session").count():
        out("already SIWS")
    else:
        FINDINGS.append("wallet connect produced neither Sign in nor Open session")
        shot(page, "connect-fail")
        raise RuntimeError("wallet not connected")


def wait_text(page, needle: str, timeout_ms=20_000) -> str:
    page.wait_for_function(
        """(n) => document.body && document.body.innerText.includes(n)""",
        arg=needle,
        timeout=timeout_ms,
    )
    return page.locator("body").inner_text()


def note_or_err(page) -> str:
    bits = []
    for loc in page.locator("p").all():
        t = loc.inner_text().strip()
        if t:
            bits.append(t)
    return " | ".join(bits[-8:])


def run_ui(kp: Keypair) -> int:
    secret = list(bytes(kp))
    stamp = str(int(time.time()) % 10_000_000)
    OUT.mkdir(parents=True, exist_ok=True)
    failed = 0

    with sync_playwright() as p:
        browser = p.chromium.launch(headless=True)
        page = browser.new_page(viewport={"width": 1400, "height": 900})
        page.on("pageerror", lambda e: FINDINGS.append(f"pageerror {e}") if "Minified React error #418" not in str(e) else None)
        connect_wallet(page, secret)
        shot(page, "01-connected")

        page.get_by_role("link", name="Portfolio").click()
        page.wait_for_load_state("networkidle")
        expect(page.get_by_role("heading", name="Portfolio")).to_be_visible()
        page.locator("label:has-text('USDC amount') + input, input[type='number']").first.fill("1000")
        page.get_by_role("button", name="Deposit", exact=True).click()
        try:
            wait_text(page, "deposit ", timeout_ms=45_000)
            out("deposit ok")
        except PwTimeout:
            msg = note_or_err(page)
            FINDINGS.append(f"deposit did not confirm: {msg}")
            out("deposit FAIL " + msg)
            failed += 1
        shot(page, "02-deposit")

        page.get_by_role("link", name="Create", exact=True).click()
        page.wait_for_load_state("networkidle")
        page.get_by_label("n_grid / atoms").select_option("8")
        page.get_by_label("Topic / series (on-chain id)").fill(f"pw{stamp}")
        page.get_by_label("Tag / release").fill("flow")
        page.get_by_label("Close in seconds").fill("180")
        page.get_by_role("button", name="Create Gaussian prediction market").click()
        g_market = None
        try:
            wait_text(page, "create_gaussian", timeout_ms=45_000)
            body = page.locator("body").inner_text()
            if "failed" in body.lower() and "create_gaussian" not in body:
                raise RuntimeError(body[-400:])
            out("create gaussian " + body.split("create_gaussian")[-1][:90])
            link = page.get_by_role("link", name="Open market").first
            expect(link).to_be_visible()
            href = link.get_attribute("href") or ""
            g_market = href.split("/m/")[-1]
        except Exception as e:
            FINDINGS.append(f"create gaussian failed: {e} | {note_or_err(page)}")
            out("create gaussian FAIL " + str(e))
            failed += 1
        shot(page, "03-create-g")

        if g_market:
            page.get_by_role("link", name="Open market").first.click()
            page.wait_for_load_state("networkidle")
            page.get_by_role("button", name="Open session").wait_for(timeout=15_000)
            for _ in range(20):
                if page.locator("button[title^='cell ']").count() >= 2:
                    break
                page.wait_for_timeout(500)
                page.reload(wait_until="networkidle")
            if page.locator("button[title^='cell ']").count() < 2:
                FINDINGS.append(f"gaussian board {g_market} has no PDF bars after create")
                failed += 1
            else:
                page.locator("button[title^='cell ']").nth(0).click()
                page.locator("button[title^='cell ']").nth(1).click()
                try:
                    page.wait_for_function(
                        """() => document.body.innerText.includes('C_S(q)') && !document.body.innerText.includes('C_S(q)\\n—')""",
                        timeout=15_000,
                    )
                except PwTimeout:
                    FINDINGS.append("quote C_S never populated after selecting cells")
                page.get_by_role("button", name="Buy set").click()
                try:
                    wait_text(page, "confirmed ", timeout_ms=45_000)
                    out("buy_set ok")
                except PwTimeout:
                    msg = note_or_err(page)
                    FINDINGS.append("buy_set did not confirm: " + msg.encode("ascii", "replace").decode())
                    out("buy_set FAIL")
                    failed += 1
            shot(page, "04-buy-g")

            page.get_by_role("link", name="Auction", exact=True).click()
            page.wait_for_load_state("networkidle")
            page.get_by_role("button", name="Quote layer").click()
            try:
                wait_text(page, "quoted ", timeout_ms=45_000)
                out("risk_quote ok")
            except PwTimeout:
                msg = note_or_err(page)
                FINDINGS.append("risk_quote did not confirm: " + msg.encode("ascii", "replace").decode())
                out("risk_quote FAIL")
                failed += 1
            shot(page, "05-auction")

            page.goto(BASE + f"/resolve/{g_market}", wait_until="networkidle")
            page.get_by_role("button", name="Open window").click()
            page.wait_for_timeout(3_000)
            resolve_note = note_or_err(page)
            out("resolve_open")
            if "resolve_open" not in resolve_note and "failed" not in resolve_note.lower():
                FINDINGS.append(f"resolve_open silent: {resolve_note}")
            shot(page, "06-resolve")

        page.goto(BASE + "/create", wait_until="networkidle")
        page.get_by_role("button", name="Skellam Football", exact=False).click()
        page.get_by_label("Topic / series (on-chain id)").fill(f"sk{stamp}")
        page.get_by_label("Close in seconds").fill("180")
        page.get_by_role("button", name="Create Skellam prediction market").click()
        s_market = None
        try:
            wait_text(page, "create_skellam", timeout_ms=45_000)
            href = page.get_by_role("link", name="Open market").first.get_attribute("href") or ""
            s_market = href.split("/m/")[-1]
            out("create skellam " + str(s_market))
        except Exception as e:
            FINDINGS.append(f"create skellam failed: {e} | {note_or_err(page)}")
            out("create skellam FAIL " + str(e))
            failed += 1
        shot(page, "07-create-s")

        if s_market:
            page.get_by_role("link", name="Open market").first.click()
            page.wait_for_load_state("networkidle")
            page.get_by_role("button", name="Open session").wait_for(timeout=15_000)
            for _ in range(20):
                if page.get_by_role("button", name="1X2 Home").count():
                    break
                page.wait_for_timeout(500)
                page.reload(wait_until="networkidle")
            if page.get_by_role("button", name="1X2 Home").count():
                page.get_by_role("button", name="1X2 Home").click()
                page.wait_for_timeout(800)
                btn = page.get_by_role("button", name="Buy line")
                if btn.count() == 0:
                    FINDINGS.append("typed 1X2 Home did not switch button to Buy line")
                    failed += 1
                else:
                    btn.click()
                    try:
                        wait_text(page, "confirmed ", timeout_ms=45_000)
                        out("buy_skellam ok")
                    except PwTimeout:
                        FINDINGS.append("buy_skellam_set did not confirm")
                        out("buy_skellam FAIL")
                        failed += 1
            else:
                FINDINGS.append("skellam board missing typed-line buttons")
                failed += 1
            shot(page, "08-buy-s")

        page.goto(BASE + "/lp", wait_until="networkidle")
        expect(page.get_by_role("heading", name="Risk LP")).to_be_visible()
        shot(page, "09-lp")
        page.goto(BASE + "/ops", wait_until="networkidle")
        expect(page.get_by_role("heading", name="Ops")).to_be_visible()
        shot(page, "10-ops")
        page.goto(BASE + "/portfolio", wait_until="networkidle")
        shot(page, "11-portfolio")

        browser.close()

    (OUT / "findings.json").write_text(json.dumps(FINDINGS, indent=2), encoding="utf-8")
    out("FINDINGS " + str(len(FINDINGS)))
    for f in FINDINGS:
        out("- " + f.encode("ascii", "replace").decode())
    return 1 if failed or FINDINGS else 0


def main() -> int:
    OUT.mkdir(parents=True, exist_ok=True)
    kp = fund_chain()
    return run_ui(kp)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as e:
        out("FLOW_FATAL " + str(e).encode("ascii", "replace").decode())
        OUT.mkdir(parents=True, exist_ok=True)
        (OUT / "fatal.txt").write_text(str(e), encoding="utf-8")
        raise
