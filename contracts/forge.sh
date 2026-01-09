#!/usr/bin/env bash
# Wrapper script that loads environment from parent directory
# Usage: ./forge.sh <forge-command> [args...]
# Example: ./forge.sh script script/DeployArbitrum.s.sol --rpc-url arbitrum --broadcast

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/../.env"

if [[ -f "$ENV_FILE" ]]; then
    set -a
    source "$ENV_FILE"
    set +a
else
    echo "Warning: $ENV_FILE not found"
fi

exec forge "$@"
