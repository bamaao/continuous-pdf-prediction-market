# Keeper active / standby

Production keepers SHALL run as two processes with a shared lease file (NFR-11).

```bash
# Host A (active candidate)
KEEPER_ID=keeper-a KEEPER_LEASE_PATH=/var/lib/cpm/keeper-lease.json \
  python scripts/keeper-standby.py --interval 10

# Host B (standby)
KEEPER_ID=keeper-b KEEPER_LEASE_PATH=/var/lib/cpm/keeper-lease.json \
  python scripts/keeper-standby.py --interval 10
```

Only the lease holder invokes `cpm keeper --once`. If the holder dies, the lease expires after `KEEPER_LEASE_SECS` (default 30) and the standby takes over.

Web `/ops` stays read-only (FR-UI-28). Halt / Commit / Undelegate stay on the CLI.
