#!/usr/bin/env bash
set -euo pipefail

TAG="26.08.1"
URL="https://github.com/ankitects/anki"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${ROOT}/vendor/anki"
PATCH="${ROOT}/patches/anki_proto-rerun-fix.patch"

if [[ ! -d "${DEST}/.git" ]]; then
    git -c advice.detachedHead=false clone -q --depth 1 --branch "${TAG}" "${URL}" "${DEST}"
fi

if git -C "${DEST}" apply --check "${PATCH}" 2>/dev/null; then
    git -C "${DEST}" apply "${PATCH}"
else
    git -C "${DEST}" apply --reverse --check "${PATCH}"
fi
