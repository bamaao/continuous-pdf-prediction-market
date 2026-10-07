#!/usr/bin/env bash
# One-time staging host prep for Ubuntu 22.04 / 24.04 only.
# Do not run on CentOS/RHEL — use scripts/staging-prep-centos.sh instead.
#
# Usage (on the Ubuntu server, as a sudo-capable deploy user):
#   bash scripts/staging-prep-ubuntu.sh
#   # then copy deploy/runtime.env.example → deploy/runtime.env and fill secrets
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "Linux only"
  exit 1
fi
if [[ ! -f /etc/os-release ]]; then
  echo "missing /etc/os-release"
  exit 1
fi
# shellcheck disable=SC1091
. /etc/os-release
case "${ID:-}:${ID_LIKE:-}" in
  ubuntu:*|*:debian*|debian:*) ;;
  *)
    echo "this script is for Ubuntu/Debian only (got ID=${ID:-unknown}). Use scripts/staging-prep-centos.sh"
    exit 1
    ;;
esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CPM_HOME="${CPM_HOME:-/opt/cpm}"

echo "== Ubuntu packages =="
sudo apt-get update
sudo DEBIAN_FRONTEND=noninteractive apt-get install -y \
  build-essential pkg-config libssl-dev clang cmake git curl ca-certificates \
  rsync openssh-client postgresql postgresql-contrib libpq-dev

if ! command -v node >/dev/null 2>&1 || [[ "$(node -v 2>/dev/null | sed 's/^v//;s/\..*//')" -lt 20 ]]; then
  echo "== Node.js 20 (NodeSource) =="
  curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
  sudo DEBIAN_FRONTEND=noninteractive apt-get install -y nodejs
fi

sudo systemctl enable --now postgresql || true

echo "== dirs $CPM_HOME =="
sudo mkdir -p "$CPM_HOME" "$CPM_HOME/keys" "$CPM_HOME/var" "$CPM_HOME/deploy"
sudo chown -R "$(id -u):$(id -g)" "$CPM_HOME"

echo "== rustup =="
if ! command -v rustc >/dev/null 2>&1; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
fi
# shellcheck disable=SC1091
source "$HOME/.cargo/env"

echo "== Solana CLI (cargo-build-sbf) =="
if ! command -v cargo-build-sbf >/dev/null 2>&1; then
  sh -c "$(curl -sSfL https://release.anza.xyz/stable/install)"
fi
export PATH="$HOME/.local/share/solana/install/active_release/bin:$HOME/.cargo/bin:$PATH"

echo "== verify =="
rustc --version
cargo --version
cargo-build-sbf --version
node --version
npm --version
psql --version || true

if [[ -f "$ROOT/infra/local-pg-role.sql" ]]; then
  echo "== postgres role/db (infra/local-pg-*.sql) =="
  sudo -u postgres psql -v ON_ERROR_STOP=1 -f "$ROOT/infra/local-pg-role.sql" || true
  sudo -u postgres psql -v ON_ERROR_STOP=1 -f "$ROOT/infra/local-pg-database.sql" || true
  sudo -u postgres psql -v ON_ERROR_STOP=1 -f "$ROOT/infra/local-pg-grant.sql" || true
fi

if [[ ! -f "$CPM_HOME/deploy/runtime.env" && -f "$ROOT/deploy/runtime.env.example" ]]; then
  cp "$ROOT/deploy/runtime.env.example" "$CPM_HOME/deploy/runtime.env.example"
  echo "next: copy $CPM_HOME/deploy/runtime.env.example → $CPM_HOME/deploy/runtime.env and fill RPC_URL / ER_RPC / secrets"
fi

echo "UBUNTU_PREP_OK"
echo "  PATH tip: export PATH=\"\$HOME/.local/share/solana/install/active_release/bin:\$HOME/.cargo/bin:\$PATH\""
