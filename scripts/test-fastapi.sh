#!/usr/bin/env bash
# Isolated integration and Docker-runtime verification. Never reads deployment credentials.
set -euo pipefail
case "${1:-}" in
    ""|--docker-only) ;;
    *) echo "Usage: scripts/test-fastapi.sh [--docker-only]" >&2; exit 2 ;;
esac
repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
cd "$repo_root"

python3 scripts/fastapi-baseline.py
uv sync --project apps/api-python --locked
uv run --project apps/api-python --locked ruff check apps/api-python
uv run --project apps/api-python --locked mypy --config-file apps/api-python/pyproject.toml apps/api-python/src

# SQLx remains the sole DDL owner during the coexistence phase.
cargo build --locked --package monkeybot-api --bin monkeybot-migrate --example fastapi-auth-compat
export FASTAPI_AUTH_COMPAT_EXE="$repo_root/target/debug/examples/fastapi-auth-compat"

test_name="monkeybot-fastapi-test-$$"
api_name="${test_name}-api"
cleanup() {
    docker rm --force "$api_name" >/dev/null 2>&1 || true
    docker rm --force --volumes "$test_name" >/dev/null 2>&1 || true
}
trap cleanup EXIT
docker run --detach --rm --name "$test_name" \
    --publish 127.0.0.1::5432 --publish 127.0.0.1::8080 \
    --env POSTGRES_PASSWORD=isolated-test-only \
    --env POSTGRES_DB=monkeybot_fastapi_test \
    postgres:18.4-alpine3.23 >/dev/null

ready=0
for _ in $(seq 1 30); do
    if docker exec "$test_name" pg_isready --username postgres --dbname monkeybot_fastapi_test >/dev/null 2>&1; then
        ready=1
        break
    fi
    sleep 1
done
if [[ "$ready" != 1 ]]; then
    echo 'Isolated PostgreSQL did not become ready.' >&2
    exit 1
fi
db_port=$(docker port "$test_name" 5432/tcp)
db_port=${db_port##*:}
api_port=$(docker port "$test_name" 8080/tcp)
api_port=${api_port##*:}
export FASTAPI_TEST_OWNER_URL="postgresql://postgres:isolated-test-only@127.0.0.1:${db_port}/monkeybot_fastapi_test?sslmode=disable"
export FASTAPI_TEST_RUNTIME_URL="postgresql://fastapi_test_runtime:isolated-runtime-only@127.0.0.1:${db_port}/monkeybot_fastapi_test?sslmode=disable"
MONKEYBOT_MIGRATION_DATABASE_URL="$FASTAPI_TEST_OWNER_URL" target/debug/monkeybot-migrate

docker exec --interactive "$test_name" psql --username postgres --dbname monkeybot_fastapi_test --set ON_ERROR_STOP=1 <<'SQL'
CREATE ROLE fastapi_test_runtime LOGIN PASSWORD 'isolated-runtime-only';
SQL
docker exec --interactive "$test_name" psql --username postgres --dbname monkeybot_fastapi_test \
    --set runtime_role=fastapi_test_runtime < deploy/postgres/runtime-grants.sql

if [[ "${1:-}" != --docker-only ]]; then
    uv run --project apps/api-python --locked pytest apps/api-python/tests
fi

docker build --file apps/api-python/Dockerfile --tag monkeybot-api-python:test \
    --build-arg "REVISION=$(git rev-parse HEAD)-working-tree" .
docker run --detach --rm --name "$api_name" --network "container:$test_name" \
    --read-only --tmpfs /tmp --cap-drop ALL --security-opt no-new-privileges:true \
    --env MONKEYBOT_ENV=test --env MONKEYBOT_PUBLIC_ORIGIN=http://localhost:5173 \
    --env MONKEYBOT_DATABASE_URL=postgresql://fastapi_test_runtime:isolated-runtime-only@127.0.0.1:5432/monkeybot_fastapi_test?sslmode=disable \
    --env MONKEYBOT_DISCORD_APPLICATION_ID=123456789012345678 \
    --env MONKEYBOT_DISCORD_CLIENT_SECRET=integration-test-only \
    --env MONKEYBOT_BOT_SERVICE_TOKEN=isolated-bot-service-token-32-characters \
    --env MONKEYBOT_OAUTH_ENCRYPTION_KEY=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA \
    monkeybot-api-python:test >/dev/null

api_ready=0
for _ in $(seq 1 30); do
    if curl --silent --fail --max-time 2 "http://127.0.0.1:${api_port}/readyz" >/dev/null; then
        api_ready=1
        break
    fi
    sleep 1
done
if [[ "$api_ready" != 1 ]]; then
    docker logs "$api_name" >&2
    echo 'FastAPI Docker candidate did not become ready.' >&2
    exit 1
fi
uv run --project apps/api-python --locked python scripts/fastapi-docker-smoke.py "http://127.0.0.1:${api_port}"
docker exec "$api_name" python -c 'import os; assert os.getuid() == 10001'
docker stop --time 25 "$api_name" >/dev/null
if [[ "${1:-}" == --docker-only ]]; then
    echo 'FastAPI Docker runtime smoke passed (pytest skipped).'
else
    echo 'FastAPI isolated PostgreSQL tests and Docker runtime smoke passed.'
fi
