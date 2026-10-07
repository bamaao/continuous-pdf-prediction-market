# Staging deploy (test L1 + MagicBlock test ER)

Target host is **Linux only**. Build **on the staging server** from synced source. Services use a **test Solana L1** and a **MagicBlock test ER**. Windows is not supported.

OS package install is **split by distro**. Build/restart is shared (same `cargo` / `npm` on both).

## Scripts

| Script | Where | Distro |
| --- | --- | --- |
| `scripts/staging-prep-ubuntu.sh` | Staging server (or via SSH) | **Ubuntu / Debian only** |
| `scripts/staging-prep-centos.sh` | Staging server (or via SSH) | **CentOS / RHEL / Rocky / Alma only** |
| `scripts/deploy-staging.sh` | Linux admin host | any (ssh + rsync/git; can remote-run prep) |
| `scripts/staging-on-server.sh` | Staging server | any (after prep) |

| Config | Role |
| --- | --- |
| `deploy/staging.env.example` | Admin SSH / remote path → copy to `deploy/staging.env` |
| `deploy/runtime.env.example` | Server `RPC_URL` / `ER_RPC` / secrets → copy to `deploy/runtime.env` on the host |

## 1) One-time prep (pick one)

### From the Linux admin host (SSH)

Deploy user needs **passwordless sudo** for apt/dnf on the staging host.

```bash
# Ubuntu staging server
bash scripts/deploy-staging.sh --prep-ubuntu --prep-only

# CentOS / RHEL / Rocky / Alma staging server
bash scripts/deploy-staging.sh --prep-centos --prep-only

# prep then continue into build/restart in one shot:
bash scripts/deploy-staging.sh --prep-ubuntu
```

### Or login on the staging server

```bash
bash scripts/staging-prep-ubuntu.sh   # Ubuntu only
bash scripts/staging-prep-centos.sh   # CentOS family only
```

Each prep script refuses the other family (`/etc/os-release` check).

Then on the server:

1. Place deployer keypair at `/opt/cpm/keys/deployer.json` (or path in `runtime.env`).
2. Copy `deploy/runtime.env.example` → `deploy/runtime.env` and set `RPC_URL`, `ER_RPC`, `PLATFORM_PUBKEY`, `SIWS_SECRET`, `PUBLIC_*`.
3. Keep `VAULT_FEATURES_DEVNET=1` if the test L1 uses Circle **devnet** USDC.

## 2) Deploy / rebuild (shared)

From a Linux admin host:

```bash
cp deploy/staging.env.example deploy/staging.env
# edit STAGING_SSH=user@host  STAGING_REMOTE_DIR=/opt/cpm

bash scripts/deploy-staging.sh
```

Or on the server after `git pull` / rsync:

```bash
bash scripts/staging-on-server.sh
```

Flags for `deploy-staging.sh`:

```bash
bash scripts/deploy-staging.sh --sync-only
bash scripts/deploy-staging.sh --build-only
bash scripts/deploy-staging.sh --restart-only
bash scripts/deploy-staging.sh --with-programs
bash scripts/deploy-staging.sh --bootstrap-chain
```

## Program IDs

Default IDs match the repo (`VaULt111…`, `Market111…`, …). The test cluster must already host them, or provide `keys/{vault,market,resolution,risk}-keypair.json` and pass `--with-programs`.

## Logs / PIDs

- Logs: `$STAGING_REMOTE_DIR/var/log/{market-api,gateway,web}.log`
- PIDs: `$STAGING_REMOTE_DIR/var/run/*.pid`
