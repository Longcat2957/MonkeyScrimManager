# MonkeyBot 데이터 가져오기 JSON 규격 V1

이 문서는 외부 시스템이 MonkeyBot 길드의 현재 시즌 lane MMR을 가져오기 전에 생성할 수 있는
`MEMBER_SEASON_MMR_SEED` JSON 아카이브 규격을 정의한다. 이 파일은 사람이 검토하고 수정할 수 있는
plain JSON이며, V1에는 암호화, 서명, 토큰 또는 데이터베이스 dump 형식이 포함되지 않는다.

이 규격은 **파일 형식**이다. 현재 import API는 외부 서비스용 API key 또는 service-to-service 인증을
제공하지 않는다. 외부 시스템은 이 형식의 파일을 만들고, 대상 길드의 활성 운영자가 MonkeyBot Web의
검토 및 적용 절차를 수행해야 한다. API를 직접 호출하는 클라이언트도 같은 browser session, CSRF,
Origin 및 운영자 권한을 충족해야 한다.

## 범위

V1은 대상 길드의 **현재 ACTIVE 시즌**에 있는 활성 Discord 멤버의 five-lane MMR만 seed 또는 교체한다.
`discordId`로 대상 멤버를 매칭하며, 파일의 `displayName`은 대상 멤버의 이름을 변경하지 않는다.

| 지원 | 지원하지 않음 |
| --- | --- |
| 현재 ACTIVE 시즌의 `TOP`, `JUNGLE`, `MID`, `BOTTOM`, `SUPPORT` MMR | 사용자, 길드 멤버십, Discord 역할 또는 채널 생성/복구 |
| MMR 값을 운영자가 검토 후 편집한 JSON | Riot 계정/PUUID, privacy, 선호 챔피언 |
| source guild/season 표기 metadata | dummy user, dummy profile 또는 INACTIVE 멤버 |
| 대상 길드에 이미 존재하는 활성 Discord 멤버의 MMR 변경 | 모집, 엔트리, 시리즈, 경매, 픽밴, game, MMR ledger/history, audit/outbox/idempotency 기록 |

가져오기는 부분 성공을 하지 않는다. 대상에 없는 멤버 하나라도 있으면 검토는 가능하지만 적용은 전부
막힌다.

## 아카이브 구조

아래 object가 내려받는 `.json` 파일의 최상위 값이다. API export response 전체를 다시 올리는 것이 아니라
그 response의 `data.archive` object만 사용한다. 모든 object는 아래에 적힌 field만 허용하는 strict object다.

```text
MemberSeasonMmrSeedArchive = {
  format: "MONKEYBOT_DATA_TRANSFER",
  kind: "MEMBER_SEASON_MMR_SEED",
  schemaVersion: 1,
  exportedAt: RFC 3339 timestamp string,
  source: {
    guildId: Discord snowflake string,
    guildName: non-empty text,
    season: {
      id: UUID string,
      name: non-empty text
    }
  },
  members: [
    {
      discordId: Discord snowflake string,
      displayName: non-empty text,
      lanes: [
        { lane: "TOP" | "JUNGLE" | "MID" | "BOTTOM" | "SUPPORT", mmr: finite JSON number }
      ]
    }
  ]
}
```

| Field | 규칙 |
| --- | --- |
| `format` | 정확히 `MONKEYBOT_DATA_TRANSFER` |
| `kind` | 정확히 `MEMBER_SEASON_MMR_SEED` |
| `schemaVersion` | JSON number `1` |
| `exportedAt` | RFC 3339 timestamp string. provenance 표기용이며 대상 MMR 계산에는 쓰지 않는다. |
| `source.guildId`, `members[].discordId` | JSON string의 0이 아닌 10진 Discord snowflake. 앞자리 `0`은 불가하며 unsigned 64-bit 범위를 넘어서는 값은 불가하다. JSON number가 아니라 string으로 보낸다. |
| `source.guildName`, `source.season.name`, `members[].displayName` | `trim()` 후 Unicode 문자 수가 1~100. 빈 문자열/공백만 있는 값은 불가하다. |
| `source.season.id` | 파싱 가능한 UUID string |
| `members` | 0~1,000개. `discordId`는 아카이브 안에서 중복될 수 없다. 빈 배열은 구조상 유효하지만 변경값이 없어 적용할 수 없다. |
| `members[].lanes` | 정확히 5개. `TOP`, `JUNGLE`, `MID`, `BOTTOM`, `SUPPORT`를 각각 한 번씩 포함해야 하며 중복/누락은 불가하다. |
| `mmr` | 유한한 JSON number. `NaN`, `Infinity`, 문자열 MMR, `null`은 불가하다. 별도의 고정 최솟값/최댓값은 없지만 대상의 현재 MMR과 안전한 finite delta를 계산할 수 있어야 한다. |

Export는 lane을 `TOP`, `JUNGLE`, `MID`, `BOTTOM`, `SUPPORT` 순서로 쓴다. Import는 다섯 lane이 각각
한 번 있다는 조건만 검사하므로 배열 순서 자체에는 의미가 없지만, 파일을 수정할 때도 이 canonical 순서를
유지하는 것을 권장한다.

`source`와 `exportedAt`은 file-declared provenance metadata일 뿐이다. 서버는 source guild/season이 실제로
존재하는지, 대상 길드와 같은지, 특정 외부 시스템이 생성했는지를 인증하지 않는다. 이 값은 import 권한이나
대상 멤버 매칭에 사용되지 않으므로 신뢰의 근거로 사용하면 안 된다. 대상 매칭의 기준은 오직
`members[].discordId`다.

## 유효한 파일 예시

```json
{
  "format": "MONKEYBOT_DATA_TRANSFER",
  "kind": "MEMBER_SEASON_MMR_SEED",
  "schemaVersion": 1,
  "exportedAt": "2026-07-16T12:00:00Z",
  "source": {
    "guildId": "1525851181157580861",
    "guildName": "MonkeyBot Test Guild",
    "season": {
      "id": "019f7000-0000-7000-8000-000000000001",
      "name": "2026 Summer"
    }
  },
  "members": [
    {
      "discordId": "390526792496054283",
      "displayName": "Longcat",
      "lanes": [
        { "lane": "TOP", "mmr": 1590 },
        { "lane": "JUNGLE", "mmr": 1560.5 },
        { "lane": "MID", "mmr": 1625 },
        { "lane": "BOTTOM", "mmr": 1540 },
        { "lane": "SUPPORT", "mmr": 1510 }
      ]
    }
  ]
}
```

정상 export API 응답은 다음처럼 envelope에 archive를 담는다. 파일로 보관하거나 외부 시스템에 전달할 값은
`data.archive`이며, `meta.requestId`나 `fileName`은 아카이브 안에 넣지 않는다.

```json
{
  "data": {
    "archive": { "...": "위 규격의 archive object" },
    "fileName": "monkeybot-mmr-1525851181157580861-019f7000-0000-7000-8000-000000000001.json"
  },
  "meta": {
    "requestId": "019f7000-0000-7000-8000-000000000002"
  }
}
```

## 검토와 적용 절차

외부 시스템이 만든 archive는 반드시 preview를 거친 뒤에만 적용할 수 있다. archive를 apply request에 다시
넣는 방식은 지원하지 않는다.

1. 대상 길드 운영자가 `POST /api/guilds/{guildId}/data-imports/preview`에 아래 body를 보낸다.
2. 응답의 `conflicts`가 비어 있고 `summary.changes`가 1 이상인지 확인한다.
3. 같은 운영자가 preview 응답의 `importId`, `digest`, `targetRevision`으로 15분 안에 apply를 호출한다.

Preview request body는 `archive` 하나만 허용한다.

```json
{
  "archive": { "...": "위 규격의 archive object" }
}
```

Preview 성공 응답의 핵심 값은 다음과 같다.

```json
{
  "data": {
    "importId": "019f7000-0000-7000-8000-000000000003",
    "digest": "server-generated 64-character lowercase SHA-256 hex",
    "expiresAt": "2026-07-16T12:15:00Z",
    "source": { "...": "archive.source" },
    "summary": {
      "archiveMembers": 1,
      "matchedMembers": 1,
      "laneEntries": 5,
      "changes": 5
    },
    "conflicts": [],
    "targetRevision": 7
  },
  "meta": { "requestId": "019f7000-0000-7000-8000-000000000004" }
}
```

Apply request는 다음 field만 받으며, `Idempotency-Key` header가 필요하다. 같은 apply 재시도에는 같은 key를
쓴다. key는 비어 있지 않은 printable ASCII 128 bytes 이하를 사용하며 UUID를 권장한다. `expectedDigest`와
`expectedTargetRevision`은 반드시 방금 받은 preview 응답의 값을 그대로 사용한다. 클라이언트가 파일 바이트를
해시해서 만든 값으로 대체하면 안 된다.

```http
POST /api/guilds/{guildId}/data-imports/{importId}/apply
Content-Type: application/json
Idempotency-Key: 019f7000-0000-7000-8000-000000000005
```

```json
{
  "expectedDigest": "preview response의 digest",
  "expectedTargetRevision": 7
}
```

`preview`와 `apply`는 모두 authenticated ACTIVE OPERATOR browser session, CSRF cookie/header, 유효한
Origin을 요구한다. 이 계약에는 bearer token, API key, Bot command 또는 원격 데이터베이스 접속을 통한
적용 경로가 없다.

## 검증과 실패 동작

Preview는 MMR, 길드 멤버, game, ledger를 변경하지 않는다. 서버는 preview 시점의 archive digest, 대상
멤버 revision, 대상 MMR snapshot, actor 및 만료 시간을 저장하고 apply transaction에서 다시 비교한다. Preview
뒤 로컬 파일을 수정해도 이미 저장된 preview에는 영향을 주지 않으며, 수정한 값을 적용하려면 그 파일을 다시
올려 새 preview를 만들어야 한다. 대상 멤버/MMR/활성 시즌이 바뀐 경우도 새 preview가 필요하다.

| 상황 | HTTP / code | 동작 |
| --- | --- | --- |
| archive JSON이 아닌 값, literal/version/field/ID/text/lane/MMR 규격 위반, 1,000명 초과 | `400 invalid_archive` | 검토 row를 만들지 않는다. |
| Content-Type 누락/오류, malformed outer JSON, outer body의 unknown field | `415 unsupported_media_type`, `400 bad_request`, `422 unprocessable_entity` | API body 자체를 거절한다. |
| 각 MMR은 finite이지만 대상 MMR과 finite ledger delta를 안전하게 계산할 수 없음 | `422 invalid_mmr_transfer` | 검토 또는 적용을 거절하며 MMR을 바꾸지 않는다. |
| archive의 Discord ID가 대상 길드의 ACTIVE Discord member가 아님 | preview `conflicts[].reason = member_not_active`, 이후 `409 preview_conflicts` | 일부 멤버만 적용하지 않는다. 대상 멤버를 먼저 등록/활성화한 뒤 새 preview를 만든다. |
| ACTIVE 시즌이 없음 | `409 no_active_season` | export/preview/apply를 진행하지 않는다. |
| 모든 lane MMR이 이미 같음 | `409 no_import_changes` | 변경 및 ledger를 만들지 않는다. |
| preview가 15분을 넘김, actor/digest/revision/MMR snapshot/시즌이 달라짐 | `404 preview_not_found`, `403 preview_actor_mismatch`, 또는 `409 preview_expired`, `preview_digest_mismatch`, `revision_conflict`, `mmr_state_changed`, `target_season_changed` | 다시 preview부터 수행한다. |
| 진행 중인 모집 또는 내전이 존재 | `409 active_workflow_conflict` | workflow를 정리한 뒤 새 preview로 재시도한다. |
| 로그인하지 않았거나 운영자 권한/CSRF가 없음 | `401 unauthorized`, `403 operator_required`, `403 csrf_validation_failed` | 대상 길드의 활성 운영자 session으로 다시 시도한다. |

모든 실패 응답은 아래 envelope를 사용한다. `message`는 사용자에게 표시할 수 있는 안전한 이유이며,
`requestId`는 운영 문의와 로그 상관관계에 사용한다.

```json
{
  "error": {
    "code": "invalid_archive",
    "message": "지원되는 MonkeyBot MMR 백업 파일 형식이 아닙니다.",
    "requestId": "019f7000-0000-7000-8000-000000000006"
  }
}
```

성공 apply는 `status: "APPLIED"`, `appliedAt`, preview와 같은 `summary`, 적용 후 `targetRevision`을
반환한다. 실제 값이 바뀐 lane마다 `DATA_IMPORT_SEED` ledger를 남기지만, 이 import는 database restore나
generic revert checkpoint를 만들지 않는다.
