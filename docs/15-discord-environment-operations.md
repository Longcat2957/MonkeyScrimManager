# Discord Application 환경 분리

MonkeyBot은 Discord Developer Portal application 두 개를 유지한다. 로컬과 VPS dev는
`MonkeyBot[dev]`, main은 `MonkeyBot`만 사용한다. 한 process가 다른 환경 application의 credential로
시작하거나 명령어를 등록하는 것은 배포 실패로 처리한다.

## 환경 계약

| 실행 환경 | Discord application | Public origin               | OAuth callback                             | Command scope |
| --------- | ------------------- | --------------------------- | ------------------------------------------ | ------------- |
| 로컬 PC   | `MonkeyBot[dev]`    | `http://localhost:5173`     | `http://localhost:5173/oauth/callback`     | 개발 guild    |
| VPS dev   | `MonkeyBot[dev]`    | `https://dev.monkeybot.lol` | `https://dev.monkeybot.lol/oauth/callback` | 개발 guild    |
| VPS main  | `MonkeyBot`         | `https://monkeybot.lol`     | `https://monkeybot.lol/oauth/callback`     | global        |

`MonkeyBot[dev]` Portal redirect 목록에는 로컬과 VPS dev callback만 둔다. `MonkeyBot`에는 main callback만
둔다. 개발 guild에는 dev Bot만 설치하고 production guild에는 main Bot만 설치한다.

## 변수 소유권

`MONKEYBOT_DISCORD_APPLICATION_ID`가 API OAuth client ID와 Bot application ID의 단일 기준값이다. 이는
public snowflake이므로 deployment env가 소유하고 Compose가 API와 Bot 모두에 주입한다.

```text
MONKEYBOT_DISCORD_APPLICATION_ID
MONKEYBOT_BOT_COMMAND_SCOPE=guild|global
MONKEYBOT_BOT_DEV_GUILD_ID=<development only>
MONKEYBOT_BOT_INSTANCE_ID=monkeybot-<dev|main>-<instance>
```

Discord client secret은 API runtime file에만, Discord Bot token은 Bot runtime file에만 둔다. Bot service
token처럼 process 사이에 공유해야 하는 내부 secret도 같은 환경의 runtime file 사이에서만 공유한다.
기존 `MONKEYBOT_DISCORD_CLIENT_ID`, `MONKEYBOT_BOT_APPLICATION_ID`, Bot secret file의
`MONKEYBOT_BOT_DEV_GUILD_ID`와 `MONKEYBOT_BOT_INSTANCE_ID`는 migration 기간에만 읽는다. 새 deployment
기준값과 함께 존재하면 정확히 같아야 한다.

dev와 main은 다음 값도 공유하지 않는다.

- Discord client secret과 Bot token
- `MONKEYBOT_OAUTH_ENCRYPTION_KEY`
- `MONKEYBOT_BOT_SERVICE_TOKEN`
- `MONKEYBOT_RIOT_SERVICE_TOKEN`
- PostgreSQL database/role URL

API와 Bot 사이의 `MONKEYBOT_BOT_SERVICE_TOKEN`만 같은 환경 안에서 동일해야 한다.

## 로컬 파일

실제 값은 ignored `.local/env/dev` 아래에 mode `600`으로 둔다.

```text
.local/env/dev/discord.env   public application/guild identity
.local/env/dev/api.env       API와 OAuth secret
.local/env/dev/bot.env       Bot token
.local/env/dev/migration.env migration database role
```

템플릿은 `deploy/local/*.env.example`이다. repository root `.env.local`은 최종적으로
`deploy/local/web.env.example`의 same-origin proxy 값만 가진다. API/Bot secret을 Vite env directory에
두지 않는다.

```bash
install -d -m 700 .local/env/dev
install -m 600 deploy/local/discord.env.example .local/env/dev/discord.env
install -m 600 deploy/local/api.env.example .local/env/dev/api.env
install -m 600 deploy/local/bot.env.example .local/env/dev/bot.env
install -m 600 deploy/local/migration.env.example .local/env/dev/migration.env
```

기존 local API user service는 `deploy/systemd/monkeybot-api-local.service.example`로 교체해야 한다. 이
unit은 `.env.local`을 읽지 않고 `discord.env`, `api.env`만 순서대로 읽는다.

지속 실행하는 local dev Bot은 `deploy/systemd/monkeybot-bot-local.service.example`을 user service로
설치한다. 이 unit도 `discord.env`, `bot.env`만 읽으며 빌드된 `apps/bot/dist/index.js`를 실행한다.

`scripts/local-discord-preflight.sh`는 파일 mode, placeholder, application/guild ID, legacy ID 충돌과
API/Bot service token 일치를 검사하며 secret 값을 출력하지 않는다. Bot은 다음 wrapper로만 시작·등록한다.

```bash
scripts/local-bot.sh start
scripts/local-bot.sh register-guild
```

개발 application에 과거 global command가 등록된 경우에만 승인 후 다음을 한 번 실행한다.

```bash
scripts/local-bot.sh clear-legacy-global
```

이 cleanup command는 `local|development + guild scope` 조합 외에는 시작 자체를 거부한다.

## VPS 파일

환경별 디렉터리를 분리하고 모든 secret file은 mode `600`으로 유지한다.

```text
/root/monkeybot-infra/dev/api/runtime.env
/root/monkeybot-infra/dev/api/migration.env
/root/monkeybot-infra/dev/riot-api/runtime.env
/root/monkeybot-infra/dev/bot/runtime.env

/root/monkeybot-infra/main/api/runtime.env
/root/monkeybot-infra/main/api/migration.env
/root/monkeybot-infra/main/riot-api/runtime.env
/root/monkeybot-infra/main/bot/runtime.env
```

Preflight는 absolute runtime path와 symlink가 현재 deployment 이름의 디렉터리를 벗어나면 실패한다. Runtime과
migration database URL의 role/database suffix도 현재 deployment와 같아야 한다. 또한 canonical application
ID와 runtime file에 남은 legacy ID가 다르거나, dev/global 및 main/guild 조합이면 실패한다.

두 환경의 파일을 모두 만든 뒤 `scripts/vps-environment-isolation-audit.sh`를 실행한다. 이 감사는 dev/main의
application ID, Discord client secret과 Bot token, OAuth encryption key, 내부 service token, database URL 및
실제 runtime file이 서로 다른지 값을 노출하지 않고 비교한다.

## Runtime 검증

Bot은 Discord login 완료 직후 token이 반환한 실제 application ID를 canonical ID와 비교한다. 불일치 또는
잘못된 응답이면 readiness를 올리지 않고 종료한다. guild/global command 등록과 legacy cleanup도 Discord의
current application endpoint를 먼저 확인한 뒤 mutation을 수행한다.

API client secret이 해당 application과 짝인지 offline으로 안전하게 증명할 수 없으므로 배포의 마지막 gate는
실제 browser OAuth callback이다. callback URI는 `MONKEYBOT_PUBLIC_ORIGIN + /oauth/callback`으로만 만든다.

## Credential 회전 순서

1. Portal에서 대상 환경 application의 client secret을 회전한다.
2. 새 값은 채팅이나 shell history에 넣지 않고 해당 API secret file에 직접 기록한다.
3. local 또는 dev API만 재시작하고 새 OAuth 로그인을 완료한다.
4. dev 검증 전에는 main credential과 process를 변경하지 않는다.
5. 기존 secret으로 rollback하지 않는다. 이전 image로 돌아가더라도 회전된 새 secret을 사용한다.

Bot token은 노출 또는 application 재생성이 확인된 경우에만 별도로 회전한다. client secret 회전과 Bot token
회전을 한 작업으로 묶지 않는다.

## 배포 완료 조건

- local/dev OAuth authorize URL의 client ID가 `MonkeyBot[dev]`다.
- main OAuth authorize URL의 client ID가 `MonkeyBot`이다.
- dev application global command는 0개이며 개발 guild command만 존재한다.
- main application은 global command만 등록한다.
- Bot token/application mismatch에서 process와 command mutation이 모두 실패한다.
- preflight와 로그는 secret/token을 출력하지 않는다.
- dev 검증 뒤에만 main rollout을 진행한다.
