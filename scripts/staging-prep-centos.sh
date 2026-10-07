#!/usr/bin/env bash
# One-time staging host prep for CentOS / RHEL / Rocky / Alma 8 or 9 only.
# Do not run on Ubuntu — use scripts/staging-prep-ubuntu.sh instead.
#
# Usage (on the CentOS-family server, as a sudo-capable deploy user):
#   bash scripts/staging-prep-centos.sh
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
  centos:*|rhel:*|rocky:*|almalinux:*|*:rhel*|*:fedora*|*:centos*) ;;
  *)
    echo "this script is for CentOS/RHEL/Rocky/Alma only (got ID=${ID:-unknown}). Use scripts/staging-prep-ubuntu.sh"
    exit 1
    ;;
esac

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CPM_HOME="${CPM_HOME:-/opt/cpm}"

PKG=dnf
if ! command -v dnf >/dev/null 2>&1; then
  PKG=yum
fi

echo "== CentOS/RHEL packages ($PKG) =="
sudo "$PKG" -y groupinstall "Development Tools" || sudo "$PKG" -y group install "Development Tools" || true
sudo "$PKG" -y install \
  gcc gcc-c++ make pkgconfig openssl-devel clang cmake git curl ca-certificates \
  rsync openssh-clients postgresql-server postgresql-contrib libpq-devel

if ! command -v node >/dev/null 2>&1 || [[ "$(node -v 2>/dev/null | sed 's/^v//;s/\..*//')" -lt 20 ]]; then
  echo "== Node.js 20 (NodeSource) =="
  curl -fsSL https://rpm.nodesource.com/setup_20.x | sudo bash -
  sudo "$PKG" -y install nodejs
fi

if ! sudo -u postgres psql -c 'SELECT 1' >/dev/null 2>&1; then
  echo "== init PostgreSQL data directory =="
  if command -v postgresql-setup >/dev/null 2>&1; then
    sudo postgresql-setup --initdb 2>/dev/null || sudo postgresql-setup initdb || true
  fi
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

echo "CENTOS_PREP_OK"
echo "  SELinux: if binds fail after deploy, check ausearch / audit.log"
echo "  PATH tip: export PATH=\"\$HOME/.local/share/solana/install/active_release/bin:\$HOME/.cargo/bin:\$PATH\""
