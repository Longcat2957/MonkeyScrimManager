# VPS Configuration

아직 배포하지 않는 준비용 설정이다. Rust/Axum API image와 dev service까지 정의했지만 실제
배포와 main service 연결은 PostgreSQL foundation 이후에 진행한다.

## Port contract

| Port    | Target        |
| ------- | ------------- |
| `10010` | PostgreSQL    |
| `20010` | frontend main |
| `20020` | backend main  |
| `30010` | frontend dev  |
| `30020` | backend dev   |

`monkeybot.lol`은 main port로, `dev.monkeybot.lol`은 dev port로 라우팅한다. `/api`,
`/oauth`, `/ws`만 backend로 보내고 나머지는 frontend로 보낸다.

테스트 frontend는 독립 Compose로 실행하되 VPS의 공용 nginx를 진입점으로 사용한다.
web은 nginx의 `deploy_default` edge network만 사용한다. API는 edge와 internal
`deploy_data` network를 함께 사용하고, PostgreSQL, Valkey, migration job은 data network에만
연결한다. 기존 v3 service와 nginx 설정은 변경하지 않는다. Cloudflare DNS는 일반 proxied A
record만 사용하며 Origin Rule은 사용하지 않는다.

기본 Compose에는 nginx, PostgreSQL, Valkey만 정의한다. Dev overlay의 `api-dev`가 Axum `/healthz`를
직접 검사하며 loopback `30020`에만 공개된다. PostgreSQL은 VPS loopback의 `10010`에만 공개한다.
`pg_isready`는 PostgreSQL process health만 확인하며 schema와 runtime 권한은 API `/readyz`와 아래
검증 절차가 담당한다.

### Local frontend/API integration

Local browser requests also use the same-origin `/api/**` and `/oauth/**` routes. SvelteKit reads
`API_BASE_URL` and `OAUTH_BASE_URL` from the repository-root ignored `.env.local` and proxies those
requests to Axum; browser code does not call port `30020` directly and does not need CORS.
`apps/web/vite.config.ts`의 `envDir`와 root `.env.example`을 같이 유지한다.

```text
PUBLIC_WEB_ORIGIN=http://localhost:5173
API_BASE_URL=http://127.0.0.1:30020/api/
OAUTH_BASE_URL=http://127.0.0.1:30020/
MONKEYBOT_PUBLIC_ORIGIN=http://localhost:5173
```

`/ws`도 같은 origin으로 Axum에 proxy된다. Development API는 private `deploy_data` network의
`redis://valkey:6379/`을 사용해 outbox invalidate를 모든 API instance에 fan-out한다. 로컬 API는
`MONKEYBOT_VALKEY_URL`이 비어 있으면 단일 process hub를 사용하므로, 여러 API instance 또는 shared
development traffic을 함께 검증할 때는 Valkey URL을 명시해야 한다. Valkey는 원장이나 draft를 저장하지
않고 Pub/Sub delivery에만 사용한다.

Proxy target은 Axum bind와 같은 IPv4 loopback을 명시해 `localhost`가 IPv6 `::1`로 해석되는 환경의
connection failure를 피한다. `API_BASE_URL`은 `/api/`와 trailing slash를 모두 포함하고 OAuth base는
root trailing slash로 끝나야 한다. 따라서 frontend의
`/api/riot/records?gameName=...&tagLine=...&platform=KR` 요청은 upstream의 같은 `/api/riot/records`
route에 도착하며 `/api/api/...`로 중복되지 않는다.

Axum은 `MONKEYBOT_RIOT_SERVICE_URL`과 `MONKEYBOT_RIOT_SERVICE_TOKEN`으로 private Riot integration
service만 호출한다. Riot production/development key는 그 별도 service의
`MONKEYBOT_RIOT_API_KEY`에만 두고 `PUBLIC_*`, web/Axum environment, response, log에는 넣지 않는다.
Riot service의 `MONKEYBOT_RIOT_INTERNAL_TOKEN`은 Axum service token과 같은 값이어야 한다. Integration
service가 없거나 key가 유효하지 않으면 Riot endpoint만 `riot_unavailable` 계열의 normalized `503`을
반환하며 guild 내부 전적/MMR은 계속 동작해야 한다.

Local Riot service는 Bot health의 `30030`과 충돌하지 않는 `127.0.0.1:30040`, Axum base URL은
trailing slash를 포함한 `http://127.0.0.1:30040/internal/riot/`를 사용한다. Dev Compose service 이름은 `riot-api-dev`,
container port는 `8081`, Axum base URL은 `http://riot-api-dev:8081/internal/riot/`다. 공개 nginx는
`/internal/riot/**`를 route하지 않으며 `riot-api-dev` port를 host/public network에 publish하지 않는다.

```text
# Axum runtime
MONKEYBOT_RIOT_SERVICE_URL=http://127.0.0.1:30040/internal/riot/
MONKEYBOT_RIOT_SERVICE_TOKEN=<shared-internal-token>

# Riot integration service runtime only
MONKEYBOT_RIOT_API_KEY=<riot-key>
MONKEYBOT_RIOT_INTERNAL_TOKEN=<shared-internal-token>
MONKEYBOT_RIOT_SEASON_START_UNIX_SECONDS=1767841200
```

`MONKEYBOT_RIOT_SEASON_START_UNIX_SECONDS`는 해당 platform의 연간 ranked season 시작 시각이다. Riot
season 전환 때 명시적으로 갱신하며, records 응답의 `sample.championAggregates.seasonStartUnixSeconds`로
같은 값을 노출한다.

### Remote development database

개발 데이터의 기준은 VPS의 `monkeybot-postgres` 컨테이너다. PostgreSQL 포트를 인터넷에 직접
공개하지 않고 VPS loopback `127.0.0.1:10010`에만 바인딩한다. 로컬 API와 migration command는
SSH 터널의 별도 포트 `127.0.0.1:15432`로 접근한다.

```sh
scripts/vps-db-tunnel.sh
```

로컬 ignored `.env.local`의 두 URL은 터널 포트를 명시한다.

```text
MONKEYBOT_DATABASE_URL=postgresql://monkeybot_runtime_dev:...@127.0.0.1:15432/monkeybot_dev?sslmode=disable
MONKEYBOT_MIGRATION_DATABASE_URL=postgresql://monkeybot_migrator_dev:...@127.0.0.1:15432/monkeybot_dev?sslmode=disable
```

원격 DB는 `deploy/postgres/docker-compose.remote.yml`만 사용한다. 이 Compose는 기존
`mookbot_v3`의 `/root/deploy` stack을 수정하거나 재시작하지 않는다.

실제 secret과 인증서는 커밋하지 않는다.

```txt
/root/monkeybot-infra/api/runtime.env
/root/monkeybot-infra/api/migration.env
/root/monkeybot-infra/riot-api/runtime.env
/root/monkeybot-infra/postgres/.env
/root/deploy/nginx/certs/origin.pem
/root/deploy/nginx/certs/origin.key
```

API의 공개 development 기본값은 `docker-compose.dev.yml`에 둔다. Runtime과 migration credential은
각각 `deploy/api/runtime.env`, `deploy/api/migration.env`에 두며 두 파일은 gitignored다. Runtime
service는 `runtime.env`만 읽고 one-shot migration service는 `migration.env`만 읽는다.

## PostgreSQL role provisioning

`POSTGRES_USER`는 cluster bootstrap과 role provisioning에만 사용한다. API와 migration job이 이
role을 사용하면 안 된다. Official image의 init environment는 빈 volume을 처음 만들 때만 적용되므로
기존 volume을 재생성하지 말고 현재 bootstrap admin으로 아래 절차를 실행한다.

1. `deploy/postgres/.env.example`을 `/root/monkeybot-infra/postgres/.env`로 만들고 mode `600`을
   설정한다.
2. Main/dev 각각 별도 database, migration login, runtime login을 만든다. Dev 예시는 다음과 같다.
   기존 volume의 bootstrap role 이름이 다르면 `monkeybot_bootstrap` 대신 그 role을 사용한다.
   Migration/runtime role 이름이 같으면 script는 database나 role을 만들기 전에 실패한다.

```sh
docker compose -f deploy/postgres/docker-compose.remote.yml exec -T postgres \
  psql -U monkeybot_bootstrap -d postgres \
  -v database_name=monkeybot_dev \
  -v migration_role=monkeybot_migrator_dev \
  -v runtime_role=monkeybot_runtime_dev \
  < deploy/postgres/provision-roles.sql
```

3. Password를 command argument나 SQL 파일에 넣지 말고 interactive `psql`에서 설정한다.

```sh
docker compose -f deploy/postgres/docker-compose.remote.yml exec postgres \
  psql -U monkeybot_bootstrap -d postgres
```

```text
\password monkeybot_migrator_dev
\password monkeybot_runtime_dev
```

4. `runtime.env.example`, `migration.env.example`을 각각 실제 ignored 파일로 만들고 mode `600`을
   설정한다. 두 password는 서로 달라야 하며 URL에 필요한 percent-encoding을 적용한다.
5. Backup을 완료한 뒤 one-shot migration을 실행한다. API startup은 migration을 실행하지 않는다.

```sh
docker compose \
  -f deploy/docker-compose.yml \
  -f deploy/docker-compose.dev.yml \
  --profile migration run --rm migrate-dev
```

`0012_user_identity_kinds.sql`은 실행 중인 API가 사용하는 `users.discord_id`를 rename하므로 일반적인
rolling migration으로 적용할 수 없다. 이 migration이 아직 적용되지 않은 database의 최초 전환은
아래 maintenance 순서를 사용한다. 구 API를 실행한 채 migration만 먼저 적용하거나, migration 전에
새 API를 먼저 기동하는 순서는 둘 다 금지한다.

```sh
# 새 API/migration image는 기존 API가 서비스 중일 때 미리 빌드한다.
docker compose \
  -f deploy/docker-compose.yml \
  -f deploy/docker-compose.dev.yml \
  --profile migration build api-dev migrate-dev

# backup/restore rehearsal 성공을 확인한 다음 API 전체를 내린다.
docker compose \
  -f deploy/docker-compose.yml \
  -f deploy/docker-compose.dev.yml stop api-dev

docker compose \
  -f deploy/docker-compose.yml \
  -f deploy/docker-compose.dev.yml \
  --profile migration run --rm migrate-dev
```

6. 모든 migration이 끝난 뒤 migration role로 runtime grant template을 실행한다.

```sh
docker compose -f deploy/postgres/docker-compose.remote.yml exec -T postgres \
  psql -U monkeybot_migrator_dev -d monkeybot_dev \
  -v runtime_role=monkeybot_runtime_dev \
  < deploy/postgres/runtime-grants.sql
```

`0012` maintenance 전환에서는 grant가 성공한 뒤에만 같은 release image의 API를 다시 기동한다.

```sh
docker compose \
  -f deploy/docker-compose.yml \
  -f deploy/docker-compose.dev.yml up -d --no-deps api-dev
```

기동 후 `/readyz`가 migration checksum까지 `available`로 확인되기 전에는 nginx upstream을 정상으로
간주하지 않는다. migration 또는 grant가 실패하면 구 binary를 재기동하지 말고 maintenance 상태를
유지한 채 backup 복구 또는 forward fix를 수행한다.

`runtime-grants.sql`은 runtime에 일반 DML만 부여하고 schema CREATE, table TRUNCATE,
`_sqlx_migrations` 변경, append-only ledger의 UPDATE/DELETE를 금지한다. 새 append-only table을 만드는
migration은 같은 release에서 grant template의 revoke 목록도 갱신해야 한다. Future table의 default
privilege는 fail-closed로 SELECT/INSERT만 허용하며, 이 grant transaction이 현재 mutable table의
UPDATE/DELETE를 연다.

Grant 확인 예시:

```sql
SELECT has_database_privilege('monkeybot_runtime_dev', 'monkeybot_dev', 'CREATE'); -- false
SELECT has_schema_privilege('monkeybot_runtime_dev', 'public', 'CREATE');         -- false
SELECT has_table_privilege('monkeybot_runtime_dev', 'public.users', 'TRUNCATE'); -- false
SELECT has_table_privilege(
  'monkeybot_runtime_dev', 'public.mmr_ledger_entries', 'UPDATE'
); -- false
SELECT has_table_privilege(
  'monkeybot_runtime_dev', 'public._sqlx_migrations', 'SELECT'
); -- true
```

## Backup/restore smoke

Password가 process argument에 나타나지 않도록 URL을 command에 넘기지 않는다. Source와 empty target을
각각 libpq service로 정의하고 service file과 pass file mode를 `600`으로 제한한다. 실제 파일
`deploy/postgres/pg_service.conf`, `deploy/postgres/pgpass`는 gitignored다.

```ini
# deploy/postgres/pg_service.conf
[monkeybot_backup_source]
host=127.0.0.1
port=10010
dbname=monkeybot_dev
user=monkeybot_migrator_dev

[monkeybot_restore_target]
host=127.0.0.1
port=10010
dbname=monkeybot_restore_rehearsal
user=monkeybot_migrator_restore
```

Password는 별도 `pgpass`에 기록한다. Source role은 schema와 모든 table을 dump할 read 권한이 있어야
하고 target role은 empty target database의 owner여야 한다.

```sh
PGSERVICEFILE=/root/deploy/postgres/pg_service.conf \
PGPASSFILE=/root/deploy/postgres/pgpass \
MONKEYBOT_BACKUP_SOURCE_SERVICE=monkeybot_backup_source \
MONKEYBOT_RESTORE_TARGET_SERVICE=monkeybot_restore_target \
scripts/postgres-backup-restore-smoke.sh
```

Script는 exported repeatable-read snapshot을 `pg_dump --snapshot`에 전달하고, restore 후 repository
migration SHA-384/version/success 전체 집합과 snapshot 시점의 핵심 row count를 비교한다.

인증서는 `monkeybot.lol`과 `dev.monkeybot.lol`을 모두 포함해야 한다.
