#!/usr/bin/env bash
# Clones the upstream Python SDK (source of truth for the wire
# protocol) into reference/ for local consultation. Never committed.
set -euo pipefail

REFERENCE_DIR="$(cd "$(dirname "$0")/.." && pwd)/reference"
UPSTREAM_URL="https://github.com/anthropics/claude-agent-sdk-python.git"

mkdir -p "${REFERENCE_DIR}"

if [ -d "${REFERENCE_DIR}/claude-agent-sdk-python/.git" ]; then
    git -C "${REFERENCE_DIR}/claude-agent-sdk-python" pull --ff-only
else
    git clone --depth 1 "${UPSTREAM_URL}" "${REFERENCE_DIR}/claude-agent-sdk-python"
fi

echo "Upstream reference ready at ${REFERENCE_DIR}/claude-agent-sdk-python"
