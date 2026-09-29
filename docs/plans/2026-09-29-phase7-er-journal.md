# Phase 7 — ER lifecycle + fill journal

> **For Claude:** Use superpowers:executing-plans. Localnet first; MagicBlock `ephemeral-rollups-sdk` CPI is a later hook when an ER validator is on the machine.

**Goal:** Receipted fills survive killing one journal process; L1 Vault USDC is untouched; after Delegate, L1 `buy_set` is rejected.

**Architecture:** Local `solana-test-validator` has no MagicBlock ER. Phase 7 therefore ships the **protocol contract** on L1 (`delegate_book` / `commit_book` / `undelegate_book`, `trades_root`) plus a **dual-copy fill journal** (replica log + object-store append). Un-delegated markets still fill on L1 (tests). A delegated market refuses L1 fills (`Delegated`) until a real ER executes them. Commit is a checkpoint of the journal Merkle root, not the definition of “fill exists” (FR-DUR-02).

**Tech stack:** Rust lib `crates/journal`, Anchor `market` ixs, gateway append, CLI keeper close/undelegate.

---

## Done when

1. Two journal directories; delete one; `replay` yields the same `trades_root`.
2. `delegate_book` then L1 `buy_set` fails with `Delegated`.
3. `commit_book` writes `trades_root`; `undelegate_book` after `close_ts` or halt clears `delegated`.
4. Vault USDC accounts are not written by journal/commit (funds stay L1).

MagicBlock ER RPC / `#[ephemeral]` is **out of this slice** (needs their validator). Wire CPI when that binary exists.

---

## Tasks

1. `crates/journal`: chained SHA-256 root, append to two dirs, replay, tests.
2. `Market.delegated` / `trades_root` / `commit_ts`; three ixs; fill gate.
3. `crates/client` + compose + CLI `delegate` / `commit` / `undelegate`; `market close` also undelegate.
4. Gateway: on `confirmed`, append journal (second copy via `JOURNAL_OBJECT_DIR`).
5. `scripts/phase7-journal.py` kill-one-copy drill.
