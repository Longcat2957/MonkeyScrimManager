# VPS Deployment Runbook

이 디렉터리는 `monkeybot.lol` main과 필요할 때의 `dev.monkeybot.lol` 개발 스택을 정의한다. 실제 VPS에서
이미 동작하는 mookbot stack, nginx, PostgreSQL은 이 Compose가 생성하거나 재시작하지 않는다.

## Boundary

다음 경계는 고정이다.

- `/root/deploy`의 `nginx`, `web`, `api`, `bot`, `valkey`는 기존 mookbot 소유다. 이 디렉터리에서
  `docker compose up`, `down`, `restart`를 실행하지 않는다.
- `/root/monkeybot-infra/postgres/docker-compose.remote.yml`의 `monkeybot-postgres`가 MonkeyBot
  PostgreSQL cluster의 유일한 owner다. 포트는 `127.0.0.1:10010`만 사용한다.
- MonkeyBot app stack은 별도 Git checkout(권장: `/root/monkeybot-dev`)에서
  `deploy/docker-compose.dev.yml` 하나만 사용한다. Compose 파일만 따로 복사하지 않는다.
- main은 `/root/monkeybot-main` checkout에서 `deploy/docker-compose.main.yml`만 사용한다.

## Main Topology

| Service                   | Network                         | Host port         | Purpose                                  |
| ------------------------- | ------------------------------- | ----------------- | ---------------------------------------- |
| `monkeybot-web-main`      | `deploy_default`                | `127.0.0.1:20010` | SvelteKit web                            |
| `monkeybot-api-main`      | edge, data, app-owned Riot link | `127.0.0.1:20020` | Axum API and `/ws`                       |
| `monkeybot-bot-main`      | `deploy_default`                | `127.0.0.1:20030` | Discord command, notification, heartbeat |
| `monkeybot-riot-api-main` | app-owned Riot link             | none              | private Riot provider adapter            |
| `monkeybot-valkey-main`   | `deploy_data`                   | none              | MonkeyBot-only realtime Pub/Sub          |

Main uses the separate `monkeybot_main` database, `monkeybot_migrator_main` migration role, and
`monkeybot_runtime_main` application role. Do not copy test or dummy data from `monkeybot_dev` into main.

Copy `deploy/main.env.example` to `/root/monkeybot-main/deploy/main.env`. The four referenced runtime
env files belong under `/root/monkeybot-infra` with mode `600`. Main always uses
`MONKEYBOT_PUBLIC_ORIGIN=https://monkeybot.lol`, `MONKEYBOT_ENV=production`, and dummy users disabled.

Main release order:

1. Build and push all four images with the same full Git SHA.
2. Provision the separate main database and roles, set passwords, and create the main runtime env files.
3. Run `scripts/vps-main-preflight.sh`, then the one-shot `migrate-main` profile and `runtime-grants.sql`.
4. Start `valkey-main`, `riot-api-main`, `api-main`, `web-main`, and `bot-main`; register global Discord commands.
5. Install `deploy/nginx/sites/monkeybot.conf`, run `docker exec nginx nginx -t`, then reload nginx.
6. Run `scripts/vps-main-smoke.sh` and complete a real Discord OAuth callback.
7. Only after main passes, stop the dev services and remove `monkeybot-dev.conf` from nginx.

## Dev Topology

| Service                  | Network                         | Host port         | Purpose                                  |
| ------------------------ | ------------------------------- | ----------------- | ---------------------------------------- |
| existing nginx           | `deploy_default`                | `80`, `443`       | public edge, existing mookbot owner      |
| `monkeybot-web-dev`      | `deploy_default`                | `127.0.0.1:30010` | SvelteKit web                            |
| `monkeybot-api-dev`      | edge, data, app-owned Riot link | `127.0.0.1:30020` | Axum API and `/ws`                       |
| `monkeybot-bot-dev`      | `deploy_default`                | `127.0.0.1:30030` | Discord command, notification, heartbeat |
| `monkeybot-riot-api-dev` | app-owned Riot link             | none              | private Riot provider adapter            |
| `monkeybot-valkey-dev`   | `deploy_data`                   | none              | MonkeyBot-only realtime Pub/Sub          |
| `monkeybot-postgres`     | `deploy_data`                   | `127.0.0.1:10010` | existing PostgreSQL source of truth      |

MonkeyBot Valkey는 기존 mookbot Valkey와 별도 process다. 이는 Redis와 Valkey를 이중 운영하는
것이 아니라, 기존 서비스에 장애·메모리·Pub/Sub 영향이 전파되지 않도록 MonkeyBot의 단일 Valkey를
격리하는 것이다. PostgreSQL만 원장이고 Valkey는 outbox invalidate 전달만 담당한다.

`deploy_default`와 `deploy_data`는 VPS가 이미 가진 external Docker network다. app Compose는
둘을 삭제하거나 새로 만들지 않는다. `deploy_data`는 intentionally internal이므로 API와 Riot adapter는
app-owned `monkeybot-<environment>_riot-egress` bridge로만 서로 통신한다. Riot adapter는 이 network 하나만
쓰며, Riot HTTPS egress를 얻지만 host port나 mookbot access는 추가하지 않는다.

## Release Images

모든 배포 이미지는 **같은 Git commit의 full SHA tag**를 사용한다. `latest`와 `dev` tag는 배포
기준으로 사용하지 않는다.

| Image                                  | Dockerfile                 |
| -------------------------------------- | -------------------------- |
| `longcat1132/monkeybot-web:<sha>`      | `apps/web/Dockerfile`      |
| `longcat1132/monkeybot-api:<sha>`      | `apps/api/Dockerfile`      |
| `longcat1132/monkeybot-riot-api:<sha>` | `apps/riot-api/Dockerfile` |
| `longcat1132/monkeybot-bot:<sha>`      | `apps/bot/Dockerfile`      |

Docker Hub에 네 repository를 private로 만든 뒤, clean worktree에서 다음을 실행한다.

```bash
pnpm docker:build
PUSH=1 pnpm docker:build
```

`scripts/docker-build.sh`는 dirty worktree를 거부하고, 기본적으로 push하지 않는다. 필요한 경우만
`ALLOW_DIRTY=1`을 명시한다. VPS의 Docker credential file은 mode `600`이어야 한다.

## Runtime Files

실제 값은 Git에 넣지 않는다. 아래 파일은 모두 mode `600`이다.

```text
/root/monkeybot-dev/deploy/dev.env
/root/monkeybot-infra/api/runtime.env
/root/monkeybot-infra/api/migration.env
/root/monkeybot-infra/riot-api/runtime.env
/root/monkeybot-infra/bot/runtime.env
```

`deploy/dev.env.example`을 `/root/monkeybot-dev/deploy/dev.env`로 복사하고 다음을 채운다.

- `MONKEYBOT_RELEASE_TAG`: release commit의 40자리 SHA
- image repository 네 개
- `MONKEYBOT_PUBLIC_ORIGIN=https://dev.monkeybot.lol`
- `30010`, `30020`, `30030` loopback port
- `deploy_default`, `deploy_data` network 이름
- 네 runtime env의 absolute path

각 secret file은 인접한 `*.example`을 기초로 만든다.

- Axum runtime: DB runtime role, Discord OAuth client ID/secret, session encryption key, service-admin
  IDs, Bot service token, Riot internal token
- migration runtime: migration role DB URL만
- Riot runtime: Riot production key, same internal token, explicit season start timestamp
- Bot runtime: Discord bot token/application ID, same Bot service token, unique instance ID

`MONKEYBOT_RIOT_SERVICE_TOKEN`과 Riot service의 `MONKEYBOT_RIOT_INTERNAL_TOKEN`은 같은 32자 이상
secret이다. `MONKEYBOT_BOT_SERVICE_TOKEN`과 Bot runtime token도 같은 값이어야 한다. Riot developer
key는 Riot runtime file 외에는 절대 넣지 않는다.

dev Compose는 dummy-user route를 켜므로 `MONKEYBOT_SERVICE_ADMIN_IDS`에는 최소 한 명의 MonkeyBot
운영자 Discord ID를 넣는다. 빈 값 또는 `replace-with-*` 예시 값은 preflight가 배포 전에 거부한다.

## Nginx Contract

VPS에 설치할 파일은 [monkeybot-dev.conf](nginx/sites/monkeybot-dev.conf) 하나다. 기존 nginx의
`/root/deploy/nginx/conf.d/monkeybot-dev.conf`로만 복사한다. `deploy/nginx/conf.d` 전체를 VPS에
동기화하면 기존 mookbot 설정을 덮어쓸 수 있으므로 금지한다.

이 server block은 다음만 라우팅한다.

```text
/api/            -> monkeybot-api-dev
/oauth/callback  -> monkeybot-api-dev (access log disabled)
/oauth/          -> monkeybot-api-dev
/ws              -> monkeybot-api-dev (WebSocket upgrade)
everything else  -> monkeybot-web-dev
```

현재 Cloudflare origin encryption mode를 임의로 바꾸지 않는다. HTTP -> HTTPS redirect는 origin mode가
명시적으로 확정되기 전까지 nginx에 추가하지 않는다.

## Preflight

VPS의 app working directory에 release Compose와 env file을 둔 뒤, 시작 전에만 실행한다. 이 명령은
container를 만들거나 재시작하지 않는다.

```bash
cd /root/monkeybot-dev

MONKEYBOT_DEV_COMPOSE_FILE=/root/monkeybot-dev/deploy/docker-compose.dev.yml \
MONKEYBOT_DEV_ENV_FILE=/root/monkeybot-dev/deploy/dev.env \
MONKEYBOT_CHECK_IMAGES=1 \
scripts/vps-dev-preflight.sh
```

Preflight는 external network, runtime file 존재/권한, Compose interpolation, optional registry manifest를
검증한다. 현재 VPS audit 기준 `monkeybot_dev`는 migrations `1..19`가 모두 성공이고 runtime grant도
확인되어 있다. 새 release가 migration을 추가하면 backup/restore rehearsal과 migration compatibility
판단을 다시 수행한다.

## First Dev Rollout

아래는 실제 배포 승인 후에만 실행한다. 아직 배포 명령은 실행하지 않는다.

1. Git checkout을 `/root/monkeybot-dev`에 배치하고 `deploy/dev.env`만 checkout 밖의 secret file을
   가리키게 설정한다. secret file은 `/root/monkeybot-infra`에만 둔다.
2. preflight를 통과시킨다.
3. migration이 추가된 release면 backup/restore rehearsal을 통과하고, breaking migration이면
   `monkeybot-api-dev`만 중지한 뒤 one-shot migration과 runtime grant를 적용한다. 기존 mookbot API는
   중지하지 않는다.
4. 같은 SHA image를 pull한 뒤 `valkey-dev`, `riot-api-dev`, `api-dev`, `web-dev`, `bot-dev`를 시작한다.
   기존 `monkeybot-web-dev`는 같은 Compose project의 legacy MonkeyBot frontend이므로 이 단계에서만
   새 image/config로 교체된다. mookbot container에는 영향을 주지 않는다.
5. `127.0.0.1:30020/readyz`가 `200`인지 확인한 뒤 nginx config를 설치한다.
6. `docker exec nginx nginx -t`가 통과했을 때만 `docker exec nginx nginx -s reload`를 실행한다.
   nginx container restart는 필요 없다. upstream hostname은 reload 시 container IP를 다시 해석하므로,
   app container를 recreate하는 모든 release/rollback 뒤에도 이 검증+reload를 반드시 수행한다.
7. [vps-dev-smoke.sh](../scripts/vps-dev-smoke.sh)를 실행하고 브라우저에서 Discord OAuth callback을
   한 번 완료한다.

실제 앱 start command 예시:

```bash
docker compose --env-file /root/monkeybot-dev/deploy/dev.env \
  -f /root/monkeybot-dev/deploy/docker-compose.dev.yml pull

docker compose --env-file /root/monkeybot-dev/deploy/dev.env \
  -f /root/monkeybot-dev/deploy/docker-compose.dev.yml up -d \
  valkey-dev riot-api-dev api-dev web-dev bot-dev
```

Migration command는 release가 DB 변경을 포함할 때만 쓴다.

```bash
docker compose --env-file /root/monkeybot-dev/deploy/dev.env \
  -f /root/monkeybot-dev/deploy/docker-compose.dev.yml \
  --profile migration run --rm migrate-dev

docker exec -i monkeybot-postgres \
  psql -v ON_ERROR_STOP=1 -U monkeybot_migrator_dev -d monkeybot_dev \
  -v runtime_role=monkeybot_runtime_dev \
  < /root/monkeybot-dev/deploy/postgres/runtime-grants.sql
```

## Smoke And Rollback

```bash
MONKEYBOT_DEV_COMPOSE_FILE=/root/monkeybot-dev/deploy/docker-compose.dev.yml \
MONKEYBOT_DEV_ENV_FILE=/root/monkeybot-dev/deploy/dev.env \
scripts/vps-dev-smoke.sh
```

Smoke는 API liveness/readiness, Bot/Riot health, public landing/install, unauthenticated `/api/me`, `/ws`
route를 확인한다. Discord OAuth는 real browser callback으로 별도 확인한다.

Rollback은 `deploy/dev.env`의 `MONKEYBOT_RELEASE_TAG`를 직전 검증 SHA로 바꾼 뒤 app services만 다시 `up -d`하고,
`nginx -t`와 `nginx -s reload`를 수행하는 방식이다. schema down migration은 하지 않는다. migration이 이미
적용된 경우에는 verified restore 또는 forward fix를 사용한다.

`docker compose down`, `/root/deploy`에서의 Compose 실행, PostgreSQL volume 재생성, 기존 mookbot
container restart는 MonkeyBot deployment 절차에 포함하지 않는다.
