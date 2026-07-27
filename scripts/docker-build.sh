#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

REGISTRY="${REGISTRY:-longcat1132}"
IMAGE_PREFIX="${IMAGE_PREFIX:-monkeybot}"
IMAGE_TAG="${IMAGE_TAG:-$(git rev-parse --verify HEAD)}"
PUSH="${PUSH:-0}"
ALLOW_DIRTY="${ALLOW_DIRTY:-0}"
PULL_BASE_IMAGES="${PULL_BASE_IMAGES:-0}"
COMPONENTS="${COMPONENTS:-all}"

if [[ "${ALLOW_DIRTY}" != "1" ]] && [[ -n "$(git status --porcelain)" ]]; then
	echo "[docker-build] refusing a dirty worktree; commit first or set ALLOW_DIRTY=1" >&2
	exit 1
fi

if [[ ! "${IMAGE_TAG}" =~ ^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$ ]]; then
	echo "[docker-build] invalid Docker tag: ${IMAGE_TAG}" >&2
	exit 1
fi

case "${PUSH}" in
	0|1) ;;
	*)
		echo "[docker-build] PUSH must be 0 or 1" >&2
		exit 1
		;;
esac

declare -A selected_components=()
if [[ "${COMPONENTS}" == "all" ]]; then
	selected_components[web]=1
	selected_components[api]=1
	selected_components[riot-api]=1
	selected_components[bot]=1
else
	IFS=',' read -r -a requested_components <<<"${COMPONENTS}"
	if [[ "${#requested_components[@]}" -eq 0 ]]; then
		echo "[docker-build] COMPONENTS must be all or a comma-separated component list" >&2
		exit 1
	fi
	for component in "${requested_components[@]}"; do
		case "${component}" in
			web|api|riot-api|bot) selected_components["${component}"]=1 ;;
			*)
				echo "[docker-build] unsupported component: ${component}" >&2
				exit 1
				;;
		esac
	done
fi

components=()
for component in web api riot-api bot; do
	if [[ -n "${selected_components[$component]:-}" ]]; then
		components+=("${component}")
	fi
done

echo "[docker-build] registry: $REGISTRY"
echo "[docker-build] tag: $IMAGE_TAG"
echo "[docker-build] push: $PUSH"
echo "[docker-build] components: ${components[*]}"

build_component() {
	local component=$1
	local dockerfile=$2
	local image="$REGISTRY/$IMAGE_PREFIX-$component"
	local -a build_args=(
		build
		-f "$dockerfile"
		-t "$image:$IMAGE_TAG"
	)

	if [[ "${PULL_BASE_IMAGES}" == "1" ]]; then
		build_args+=(--pull)
	fi
	build_args+=(.)

	echo "[docker-build] building $image:$IMAGE_TAG"
	docker "${build_args[@]}"

	if [[ "${PUSH}" == "1" ]]; then
		echo "[docker-build] pushing $image:$IMAGE_TAG"
		docker push "$image:$IMAGE_TAG"
		docker manifest inspect "$image:$IMAGE_TAG" >/dev/null
	fi
}

for component in "${components[@]}"; do
	case "${component}" in
		web) build_component web apps/web/Dockerfile ;;
		api) build_component api apps/api/Dockerfile ;;
		riot-api) build_component riot-api apps/riot-api/Dockerfile ;;
		bot) build_component bot apps/bot/Dockerfile ;;
	esac
done

echo "[docker-build] completed image build for $IMAGE_TAG"
