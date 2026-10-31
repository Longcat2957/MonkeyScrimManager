# MonkeyBot Riot FastAPI service

This is the standalone Python replacement for `apps/riot-api`. It exposes the same
`/healthz` and four `/internal/riot/*` routes and keeps its upstream requests, records
cache and single-flight state within one process.

Required environment variables are `MONKEYBOT_RIOT_API_KEY`,
`MONKEYBOT_RIOT_INTERNAL_TOKEN` (at least 32 characters) and
`MONKEYBOT_RIOT_SEASON_START_UNIX_SECONDS` (positive Unix seconds). Optional settings
are `MONKEYBOT_RIOT_BIND_ADDR` (default `127.0.0.1:30040`),
`MONKEYBOT_RIOT_REQUEST_TIMEOUT_MS` (default 5000) and
`MONKEYBOT_RIOT_MATCH_COUNT` (default 10).

From the repository root:

```sh
uv sync --project apps/riot-api-python --group dev
uv run --project apps/riot-api-python pytest -q apps/riot-api-python/tests
uv run --project apps/riot-api-python ruff check apps/riot-api-python
uv run --project apps/riot-api-python mypy --config-file apps/riot-api-python/pyproject.toml apps/riot-api-python/src
```

The package command is `monkeybot-riot-api-python`. The Dockerfile uses the repository
root as its build context. Run one worker initially because the cache and request
coordination are process-local.
