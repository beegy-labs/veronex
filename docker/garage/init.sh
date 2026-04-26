#!/bin/sh
# Garage single-node bootstrap — idempotent.
#
# Garage's daemon starts before the cluster has a layout; until a layout is
# applied the S3 API returns 503. This script:
#   1. Downloads the matching `garage` CLI (the official image is distroless
#      so we can't reuse its binary directly from a sibling container)
#   2. Waits for the RPC to answer
#   3. Assigns the local node into a single-zone layout
#   4. Creates the two veronex buckets
#   5. Imports a deterministic access/secret pair (matches S3_ACCESS_KEY /
#      S3_SECRET_KEY env on the veronex API container)
#   6. Grants read+write on the buckets to that key
#   7. Marks the images bucket as web-served (anonymous GET) — equivalent to
#      MinIO `mc anonymous set download` for S3_IMAGE_PUBLIC_URL
set -eu

ACCESS_KEY="${S3_ACCESS_KEY:-veronex}"
SECRET_KEY="${S3_SECRET_KEY:-veronex123}"
GARAGE_VERSION="${GARAGE_VERSION:-v1.0.1}"
KEY_NAME="veronex-app"

# Install deps + binary.
apk add --no-cache curl >/dev/null
ARCH=$(uname -m)
case "$ARCH" in
  x86_64)  GBIN="garage-x86_64-unknown-linux-musl" ;;
  aarch64) GBIN="garage-aarch64-unknown-linux-musl" ;;
  *)       echo "unsupported arch: $ARCH"; exit 1 ;;
esac
echo "downloading garage ${GARAGE_VERSION} for ${ARCH}..."
curl -fsSL "https://garagehq.deuxfleurs.fr/_releases/${GARAGE_VERSION}/${GBIN##garage-}/garage" -o /usr/local/bin/garage \
  || curl -fsSL "https://garagehq.deuxfleurs.fr/_releases/${GARAGE_VERSION}/$(echo ${GBIN##garage-} | cut -d- -f1)/garage" -o /usr/local/bin/garage
chmod +x /usr/local/bin/garage
garage --version

# Wait for RPC.
echo "waiting for garage RPC..."
until garage status >/dev/null 2>&1; do sleep 1; done
echo "garage RPC up"

# Layout — assign this node into zone dc1 with 1G capacity (single node).
NODE_ID=$(garage status | awk '/^[0-9a-f]{16,}/ {print $1; exit}')
if [ -z "$NODE_ID" ]; then
  echo "could not detect node id"
  garage status
  exit 1
fi

if garage layout show 2>/dev/null | grep -q "Current cluster layout version: 0"; then
  garage layout assign -z dc1 -c 1G "$NODE_ID"
  garage layout apply --version 1
  echo "layout applied"
else
  echo "layout already applied — skip"
fi

# Buckets — `bucket create` is non-idempotent, swallow "already exists".
for B in veronex-messages veronex-images; do
  garage bucket create "$B" 2>/dev/null || echo "bucket $B exists"
done

# Key — import is deterministic so we can reuse the same access/secret across
# restarts. Garage requires the key ID to be `GK<24 hex chars>` — invalid
# format errors should NOT be swallowed (they mean the env value is broken).
# Only "already exists" should be ignored on subsequent runs.
if ! garage key list 2>/dev/null | grep -q "$ACCESS_KEY"; then
  garage key import --yes -n "$KEY_NAME" "$ACCESS_KEY" "$SECRET_KEY"
else
  echo "key $ACCESS_KEY already imported — skip"
fi

# Permissions — grant read+write on both buckets to the app key.
# `bucket allow --key` matches on KEY_ID (= the access key string we imported),
# not on the human-friendly key NAME. Use $ACCESS_KEY here.
for B in veronex-messages veronex-images; do
  garage bucket allow --read --write --key "$ACCESS_KEY" "$B" || true
done

# Anonymous read for images via the web port (3902). The S3 API port (3900)
# always requires Sigv4; veronex-images public URLs hit the web port instead.
garage bucket website --allow veronex-images || echo "veronex-images website already allowed"

echo "garage init complete"
