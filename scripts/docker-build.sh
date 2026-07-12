#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REGISTRY="${REGISTRY:-longcat1132}"
IMAGE_PREFIX="${IMAGE_PREFIX:-monkeybot}"
IMAGE_TAG="${IMAGE_TAG:-$(git rev-parse --short=12 HEAD)}"

echo "[docker-build] registry: $REGISTRY"
echo "[docker-build] tag: $IMAGE_TAG"

image="$REGISTRY/$IMAGE_PREFIX-web"
echo "[docker-build] building $image:$IMAGE_TAG"
docker build \
  -f apps/web/Dockerfile \
  -t "$image:$IMAGE_TAG" \
  -t "$image:latest" \
  .

echo "[docker-build] built $image"
