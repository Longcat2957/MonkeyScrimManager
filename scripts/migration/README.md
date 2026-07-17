# MOOKBOT_v3 → MonkeyBot 전체 스냅샷

이 디렉터리의 변환기는 MOOKBOT_v3 D1/Valkey read-only snapshot을 MonkeyBot Web/API가 받는
`GUILD_FULL_SNAPSHOT` V1 JSON으로 만든다. 변환기는 PostgreSQL에 접속하거나 import/apply를 호출하지 않는다.

원본 export/analyze 도구와 PII 산출물은 Git에서 제외된 `.local/migration-tools` 및
`.local/migration-artifacts`에 둔다. 최종 archive도 Discord ID, Riot ID/PUUID와 운영 이력을 포함하므로
`.local` 아래에 mode `0600`으로만 저장하고 commit하지 않는다.

## 변환

Source mutation과 authoritative Valkey write를 중지한 뒤 D1/Valkey snapshot과 analysis report를 다시 만든다.
`stableDoubleRead=true`, foreign-key violation 0, analysis error 0을 확인한 다음 실행한다.

```bash
node scripts/migration/convert-mookbot-v3.mjs \
  --snapshot .local/migration-artifacts/<snapshot> \
  --champions apps/web/static/fixtures/datadragon/champion-ko_KR.fixture.json \
  --guild-id '<historical source Discord guild snowflake>' \
  --guild-name '<historical source guild name>' \
  --write-freeze-at '<RFC3339 freeze timestamp>' \
  --output .local/migration-artifacts/<snapshot>/monkeybot-full-v1.json \
  --report .local/migration-artifacts/<snapshot>/conversion-report.json
```

`--guild-id`는 archive의 historical source guild ID다. 실제 preview/apply의 API path guild ID는 달라도 되며,
서버가 모든 tenant 참조를 target guild로 재매핑한다. 교차 guild 적용에서는 과거 모집글/결과 카드의 Discord
channel/message 연결을 제거하고 전적·MMR·대회 이력은 보존한다.
변환은 deterministic UUIDv7을 만들고 완료된 이력만 archive의 단일 active season에 포함한다. 일반 내전은
source `series.status = COMPLETED`인 시리즈와 그 확정 게임만 보존한다. 완료된 시리즈가 soft-delete된 과거
이력이면 전적은 유지하되, `IN_PROGRESS`·`CANCELLED` 시리즈와 연결된 참가자·게임·픽밴·게임 MMR 원장은 모두
제외한다. 완료된 시리즈의 부모로 필요한 일반 모집만 `CONVERTED` 상태로 남기며, 원본 부모 모집/참가자가 이미
사라진 경우 frozen roster로 최소 부모를 합성하고 report에 근거를 남긴다. 미제출 pick/ban draft는 가져오지
않는다. MMR 원장이 남아 있지 않은 완료 게임은 `LEGACY_NOT_APPLIED`로 보존해 현재 MMR을 재계산하지 않는다.
완료 시리즈 참가자의 시작 MMR 근거가 원본 MMR 변경 원장과 시즌 요약 양쪽에 모두 없으면 시스템 기본값
`1500`을 `INFERRED` snapshot으로 저장한다. 해당 참가자 수와 기준값은 conversion report의
`reconciliation.inferredBaselineSeriesMmr`에 남긴다.

시리즈 생성 대기, 엔트리 입력 대기, 공개 모집, 취소 모집 등 완료된 시리즈로 이어지지 않은 일반 모집은 source
상태와 관계없이 전부 제외한다. 경매도 완료된 대회와 완료된 경기로 이루어진 이력만 허용하므로, 완료 대회가
없는 snapshot에서는 경매 모집·경매 진행 상태를 모두 제외한다. 원본 snapshot은 수정하지 않으며 제외한 번호,
상태별 수량과 종속 row 수는 conversion report의 `reconciliation.omittedUnfinishedRecruitments`,
`omittedIncompleteSeries`, `omittedIncompleteAuctionWorkflows`에 기록한다.

## Preview-only PostgreSQL 검증

다음 ignored integration test는 격리된 임시 PostgreSQL에 모든 migration을 적용하고 repository preview를
호출한다. Archive row는 SAVEPOINT 안에서 실제 FK/CHECK/UNIQUE 제약을 검사한 뒤 rollback되며 apply는 호출하지
않는다.

```bash
MONKEYBOT_V3_ARCHIVE_PATH="$PWD/.local/migration-artifacts/<snapshot>/monkeybot-full-v1.json" \
MONKEYBOT_V3_TARGET_GUILD_ID='<target Discord guild snowflake>' \
  cargo test -p monkeybot-api --test mookbot_v3_archive_preview -- --ignored --nocapture
```

격리 target에는 닫힌 시즌 이력과 실제 `/초기설정`과 같은 빈 active season 하나를 둔다. Preview는 archive
active season을 그 target season ID로 매핑하고, source active season row는 새로 만들지 않는다. 기존 target의
closed season은 보존하며 전역 display number는 기존 번호 뒤로 이어 붙인다.
성공 조건은 preview conflict 0, preview 후 target active season 1개 유지,
`remapsSourceGuild=true`(source/target이 다를 때), `usesTargetActiveSeason=true`,
`rebasesDisplayNumbers=true`(기존 번호가 있을 때),
`actualDatabaseImportPerformed=false`다.

## 실제 적용 gate

실제 DB import는 자동화하지 않는다. 운영자가 다음을 확인하고 명시적으로 승인한 뒤 이미 실행 중인 개발 Web의
`/g/{guildId}/admin/data`에서 파일을 선택한다.

- 최종 write freeze 뒤 다시 만든 D1/Valkey checksum과 안정적인 double read
- D1 foreign-key check 및 가능한 경우 official SQLite `integrity_check`
- conversion report의 row count, 완료 이력 제외 내역, `includedSoftDeleted`, `omittedSoftDeleted`
- source/target guild ID와 `remapsSourceGuild` 표시가 의도와 같고, target의 현재 active season만 비어 있으며
  기존 closed season 이력과 번호가 보존된다는 preview 결과
- backup/restore rehearsal 및 맞는 API binary/migration 0021

Web preview 성공은 import가 아니다. 길드 이름 확인 modal에서 최종 적용을 눌러야만 apply transaction이 실행된다.
