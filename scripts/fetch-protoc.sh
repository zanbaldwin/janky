#!/usr/bin/env bash
set -euo pipefail

VERSION=31.1
SHA256=96553041f1a91ea0efee963cb16f462f5985b4d65365f3907414c360044d8065
URL="https://github.com/protocolbuffers/protobuf/releases/download/v${VERSION}/protoc-${VERSION}-linux-x86_64.zip"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="${ROOT}/tools/protoc"

if [[ -x "${DEST}/bin/protoc" ]] && "${DEST}/bin/protoc" --version | grep -qx "libprotoc ${VERSION}"; then
    exit 0
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -sSfL -o "$tmp/protoc.zip" "${URL}"
echo "${SHA256}  $tmp/protoc.zip" | sha256sum -c --quiet -
rm -rf "${DEST}"
mkdir -p "${DEST}"
unzip -q "$tmp/protoc.zip" -d "${DEST}"
"${DEST}/bin/protoc" --version
