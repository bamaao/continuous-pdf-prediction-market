#!/usr/bin/env bash
# Deploy to a Linux staging server from a Linux admin host (not Windows).
#
# Distro package install (also over SSH):
#   bash scripts/deploy-staging.sh --prep-ubuntu
#   bash scripts/deploy-staging.sh --prep-centos
# Or only prep (no build/restart):
#   bash scripts/deploy-staging.sh --prep-ubuntu --prep-only
#
# Flow:
#   1) Load deploy/staging.env (SSH host + remote dir)
#   2) Sync repo source to the server (rsync or git pull)
#   3) Optional: SSH staging-prep-{ubuntu,centos}.sh (needs passwordless sudo)
#   4) SSH: scripts/staging-on-server.sh builds on the host and restarts
#      market-api / gateway / web against test L1 RPC + MagicBlock test ER
#
# Usage:
#   cp deploy/staging.env.example deploy/staging.env   # edit
#   bash scripts/deploy-staging.sh
#   bash scripts/deploy-staging.sh --sync-only
#   bash scripts/deploy-staging.sh --build-only
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

STAGING_ENV="${STAGING_ENV:-$ROOT/deploy/staging.env}"
if [[ ! -f "$STAGING_ENV" ]]; then
  echo "missing $STAGING_ENV"
  echo "copy deploy/staging.env.example → deploy/staging.env and set STAGING_SSH / STAGING_REMOTE_DIR"
  exit 1
fi
# shellcheck disable=SC1090
set -a
source "$STAGING_ENV"
set +a

: "${STAGING_SSH:?STAGING_SSH required (user@host)}"
: "${STAGING_REMOTE_DIR:?STAGING_REMOTE_DIR required (e.g. /opt/cpm)}"

STAGING_SSH_PORT="${STAGING_SSH_PORT:-22}"
STAGING_SYNC="${STAGING_SYNC:-rsync}"
STAGING_REMOTE_BUILD="${STAGING_REMOTE_BUILD:-1}"
STAGING_REMOTE_RESTART="${STAGING_REMOTE_RESTART:-1}"
STAGING_DEPLOY_PROGRAMS="${STAGING_DEPLOY_PROGRAMS:-0}"
STAGING_CHAIN_BOOTSTRAP="${STAGING_CHAIN_BOOTSTRAP:-0}"

SYNC_ONLY=0
BUILD_ONLY=0
PREP_DISTRO=""
PREP_ONLY=0
for arg in "$@"; do
  case "$arg" in
    --sync-only) SYNC_ONLY=1; STAGING_REMOTE_BUILD=0; STAGING_REMOTE_RESTART=0 ;;
    --build-only) BUILD_ONLY=1; STAGING_REMOTE_RESTART=0 ;;
    --restart-only) STAGING_REMOTE_BUILD=0; STAGING_REMOTE_RESTART=1 ;;
    --with-programs) STAGING_DEPLOY_PROGRAMS=1 ;;
    --bootstrap-chain) STAGING_CHAIN_BOOTSTRAP=1 ;;
    --prep-ubuntu) PREP_DISTRO=ubuntu ;;
    --prep-centos) PREP_DISTRO=centos ;;
    --prep-only)
      PREP_ONLY=1
      STAGING_REMOTE_BUILD=0
      STAGING_REMOTE_RESTART=0
      ;;
    -h|--help)
      sed -n '2,25p' "$0"
      exit 0
      ;;
    *)
      echo "unknown arg: $arg"
      exit 1
      ;;
  esac
done

if [[ "$PREP_ONLY" == "1" && -z "$PREP_DISTRO" ]]; then
  echo "--prep-only requires --prep-ubuntu or --prep-centos"
  exit 1
fi

SSH_OPTS=(-p "$STAGING_SSH_PORT" -o StrictHostKeyChecking=accept-new)
if [[ -n "${STAGING_SSH_IDENTITY:-}" ]]; then
  SSH_OPTS+=(-i "$STAGING_SSH_IDENTITY")
fi

ssh_cmd() {
  ssh "${SSH_OPTS[@]}" "$STAGING_SSH" "$@"
}

echo "== staging deploy =="
echo "  ssh     $STAGING_SSH:$STAGING_SSH_PORT"
echo "  remote  $STAGING_REMOTE_DIR"
echo "  sync    $STAGING_SYNC"
echo "  prep    ${PREP_DISTRO:-none}  build=$STAGING_REMOTE_BUILD  restart=$STAGING_REMOTE_RESTART  programs=$STAGING_DEPLOY_PROGRAMS"

ssh_cmd "mkdir -p '$STAGING_REMOTE_DIR' '$STAGING_REMOTE_DIR/deploy' '$STAGING_REMOTE_DIR/keys' '$STAGING_REMOTE_DIR/var'"

# Keep server runtime.env if present; only push the example if missing.
if ! ssh_cmd "test -f '$STAGING_REMOTE_DIR/deploy/runtime.env'"; then
  echo "== seed runtime.env.example (edit on server before first restart) =="
  SCP_OPTS=(-P "$STAGING_SSH_PORT")
  if [[ -n "${STAGING_SSH_IDENTITY:-}" ]]; then
    SCP_OPTS+=(-i "$STAGING_SSH_IDENTITY")
  fi
  scp "${SCP_OPTS[@]}" \
    "$ROOT/deploy/runtime.env.example" \
    "$STAGING_SSH:$STAGING_REMOTE_DIR/deploy/runtime.env.example"
  echo "NOTE: create $STAGING_REMOTE_DIR/deploy/runtime.env from the example (RPC_URL, ER_RPC, SIWS_SECRET, PLATFORM_PUBKEY)."
fi

if [[ "$STAGING_SYNC" == "git" ]]; then
  echo "== git pull on server =="
  ssh_cmd "set -euo pipefail; cd '$STAGING_REMOTE_DIR'; git pull --ff-only"
elif [[ "$STAGING_SYNC" == "rsync" ]]; then
  need_rsync="$(command -v rsync || true)"
  if [[ -z "$need_rsync" ]]; then
    echo "rsync not found on this machine; install rsync or set STAGING_SYNC=git"
    exit 1
  fi
  echo "== rsync source → server =="
  RSYNC_SSH="ssh -p $STAGING_SSH_PORT"
  if [[ -n "${STAGING_SSH_IDENTITY:-}" ]]; then
    RSYNC_SSH="$RSYNC_SSH -i $STAGING_SSH_IDENTITY"
  fi
  rsync -az --delete \
    --exclude '.git/' \
    --exclude 'target/' \
    --exclude '**/node_modules/' \
    --exclude 'apps/web/.next/' \
    --exclude 'tmp/' \
    --exclude 'test-ledger/' \
    --exclude 'var/' \
    --exclude 'deploy/staging.env' \
    --exclude 'deploy/runtime.env' \
    --exclude 'keys/' \
    --exclude '.env' \
    --exclude 'apps/web/.env' \
    --exclude 'apps/web/.env.local' \
    --exclude '__pycache__/' \
    --exclude '*.pyc' \
    -e "$RSYNC_SSH" \
    "$ROOT/" "$STAGING_SSH:$STAGING_REMOTE_DIR/"
else
  echo "STAGING_SYNC must be rsync or git"
  exit 1
fi

if [[ "$SYNC_ONLY" == "1" ]]; then
  echo "SYNC_OK (prep/build/restart skipped)"
  exit 0
fi

ssh_cmd "chmod +x \
  '$STAGING_REMOTE_DIR/scripts/staging-on-server.sh' \
  '$STAGING_REMOTE_DIR/scripts/deploy-staging.sh' \
  '$STAGING_REMOTE_DIR/scripts/staging-prep-ubuntu.sh' \
  '$STAGING_REMOTE_DIR/scripts/staging-prep-centos.sh'"

if [[ -n "$PREP_DISTRO" ]]; then
  prep_script="scripts/staging-prep-${PREP_DISTRO}.sh"
  echo "== remote prep ($PREP_DISTRO) via SSH =="
  echo "    requires passwordless sudo on $STAGING_SSH for apt/dnf"
  ssh_cmd "set -euo pipefail
    cd '$STAGING_REMOTE_DIR'
    export CPM_HOME='$STAGING_REMOTE_DIR'
    bash '$prep_script'
  "
  if [[ "$PREP_ONLY" == "1" ]]; then
    echo "PREP_OK (build/restart skipped)"
    exit 0
  fi
fi

echo "== remote build / restart =="
ssh_cmd "set -euo pipefail
  export STAGING_REMOTE_BUILD='$STAGING_REMOTE_BUILD'
  export STAGING_REMOTE_RESTART='$STAGING_REMOTE_RESTART'
  export STAGING_DEPLOY_PROGRAMS='$STAGING_DEPLOY_PROGRAMS'
  export STAGING_CHAIN_BOOTSTRAP='$STAGING_CHAIN_BOOTSTRAP'
  cd '$STAGING_REMOTE_DIR'
  if [[ ! -f deploy/runtime.env ]]; then
    echo 'missing deploy/runtime.env on server — copy from deploy/runtime.env.example and fill test L1 + MagicBlock ER URLs'
    exit 1
  fi
  bash scripts/staging-on-server.sh
"

echo "DEPLOY_STAGING_DONE"
if [[ "$BUILD_ONLY" == "1" ]]; then
  echo "(restart skipped)"
fi
