#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

usage() {
  cat <<'EOF'
Usage: ./run.sh [--release] [-- <args passed to hank-cli>]

Examples:
  ./run.sh
  ./run.sh --release
  ./run.sh -- --help
EOF
}

if ! command -v cargo >/dev/null 2>&1; then
  echo "Error: cargo is not installed or not in PATH." >&2
  exit 1
fi

release=0
binary_args=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      release=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      binary_args=("$@")
      break
      ;;
    *)
      binary_args+=("$1")
      shift
      ;;
  esac
done

if [[ -f "$ROOT_DIR/.env" ]]; then
  set -a
  # shellcheck disable=SC1091
  source "$ROOT_DIR/.env"
  set +a
fi

# Keep .env.example compatible with the Rust config loader.
if [[ -n "${MODEL_ID:-}" && -z "${ANTHROPIC_MODEL:-}" ]]; then
  export ANTHROPIC_MODEL="$MODEL_ID"
fi

profile="debug"
build_args=(build --bin hank-cli)

if [[ "$release" -eq 1 ]]; then
  profile="release"
  build_args+=(--release)
fi

echo "Building hank-cli ($profile)..."
cargo "${build_args[@]}"

echo "Running hank-cli..."
if [[ ${#binary_args[@]} -gt 0 ]]; then
  exec "$ROOT_DIR/target/$profile/hank-cli" "${binary_args[@]}"
else
  exec "$ROOT_DIR/target/$profile/hank-cli"
fi
