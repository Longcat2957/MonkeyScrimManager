# Development Velocity Plan

## 목적

MonkeyBot 개발은 빠른 첫 동작 확인과 필요한 수준의 안정성을 동시에 목표로 한다. 속도를 떨어뜨리는 주된
원인은 넓은 사전 탐색, 한 번에 너무 많은 분기 구현, 중복 검증, 거대한 통합 fixture, 실행 중인 backend와
소스 상태의 불일치다. 아래 절차를 기본 개발 루틴으로 사용한다.

## 기본 원칙

1. 사용자 요청을 3~6개의 검증 가능한 acceptance 항목으로 먼저 줄인다.
2. 한 번에 하나의 수직 흐름을 완성한다. UI, API client, endpoint, repository를 필요한 범위에서만 연결한다.
3. 현재 frontend 동작을 제품 계약의 우선 기준으로 삼고, 문서는 불명확한 부분만 보완한다.
4. 변경 위험도에 맞춰 테스트한다. 모든 변경에 전체 workspace 검증을 적용하지 않는다.
5. 소스 반영과 실행 중인 프로세스 반영을 같은 완료 조건으로 취급한다.

## 실행 순서

### 1. Scope lock

- 대상 route와 사용자 동작을 한 문장으로 고정한다.
- 이번 변경에서 하지 않을 일을 명시한다.
- 기존 계약과 충돌하는 한 가지 쟁점만 질문하고, 발견 가능한 내용은 코드와 DB에서 직접 확인한다.

### 2. 제한된 탐색

- 대상 page/handler/repository, 직접 사용하는 contract, 관련 문서 한 곳을 우선 읽는다.
- named reference가 있을 때만 해당 reference의 같은 workflow를 확인한다.
- 관련 파일을 찾은 뒤 광범위한 repo 검색을 중단한다.
- 독립적인 `rg`, `sed`, `git diff` 읽기는 한 번에 병렬 실행한다. 같은 범위를 반복 조회하지 않는다.

### 3. 작은 수직 구현

- 먼저 happy path를 실제 데이터로 연결한다.
- 다음으로 권한, 빈 상태, 오류 상태를 붙인다.
- 실제로 존재하는 format 분기만 추가한다. 미래 기능을 위한 DTO나 UI는 만들지 않는다.
- 서로 독립적인 frontend/backend만 병렬화하며 한 파일에 여러 작업자를 배정하지 않는다.

### 4. 빠른 피드백

- 첫 편집 직후 가장 작은 compile/check 또는 단일 테스트를 실행한다.
- 실패하면 더 넓은 suite를 실행하지 않고 해당 실패부터 수정한다.
- 구현 중 문서 포맷, 전체 lint, 전체 build를 반복하지 않는다.

### 5. 통합과 실행 반영

- frontend는 이미 실행 중인 `5173` 환경에 HMR로 반영하고 dev server를 새로 시작하지 않는다.
- backend route는 targeted test 통과 후 아래 명령으로 빌드, 재시작, readiness, direct/proxy smoke를 묶어서 확인한다.

```bash
scripts/local-api-reload.sh '/api/<changed-route>'
```

- 인증 route의 unauthenticated `401`은 router 등록 smoke로 유효하다. `404`, 연결 실패, `5xx`는 완료가 아니다.

## 검증 매트릭스

| 변경 종류                            | 구현 중                                       | 완료 시                                    | 전체 workspace 검증 |
| ------------------------------------ | --------------------------------------------- | ------------------------------------------ | ------------------- |
| 문구/CSS/간단 UI 상태                | 관련 component/page check                     | package check 또는 관련 test               | 하지 않음           |
| frontend API client/SSR              | client 또는 page targeted test                | web check + 관련 tests                     | commit gate에서만   |
| 기존 schema의 read endpoint          | repository/handler unit 또는 단일 integration | 관련 endpoint integration + clippy package | 필요할 때만         |
| auth/tenant/privacy/mutation         | happy path + 핵심 denial integration          | 관련 package tests + clippy                | commit gate 권장    |
| migration/shared contract/foundation | migration 또는 contract targeted test         | package 전체 + build                       | 필수                |
| release/사용자 요청 commit 전        | 변경별 targeted test                          | lint/check/build                           | 필수                |

같은 보장을 확인하는 명령은 중복 실행하지 않는다. 예를 들어 마지막 편집 뒤 package 전체 test를 통과했다면
그 안에 포함된 동일 targeted test를 다시 실행하지 않는다.

## 테스트 작성 규칙

- 테스트는 사용자 workflow 또는 실제 장애를 보호해야 한다.
- 한 endpoint fixture는 필요한 branch를 최소 row로 구성한다.
- 같은 users/guild/season/lineup 생성 SQL은 공용 builder나 support module로 재사용한다.
- SQL을 그대로 재구현해 결과를 계산하는 테스트는 피하고, API에서 관찰 가능한 계약을 assertion한다.
- privacy, tenant, idempotency처럼 실패 시 영향이 큰 경계는 반드시 남긴다.
- 스키마상 불가능하거나 문서에 없는 가상 edge case는 기본 범위에 넣지 않는다.
- 테스트 파일이 fixture 중심으로 비대해지면 다음 branch 추가 전에 seed helper를 분리한다.

## 도구와 협업 규칙

- 독립적인 read-only 명령은 묶어서 호출한다.
- 장시간 command는 실제로 필요한 하나만 실행하고 완료를 기다린다. 같은 Cargo/pnpm suite를 동시에 실행하지 않는다.
- sub-agent는 frontend/backend처럼 독립적인 작업에만 사용한다.
- 별도 reviewer는 DB, auth, tenant, pagination처럼 고위험 변경에 한 번만 사용한다.
- reviewer 지적은 severity와 실제 재현 가능성으로 정리하고, 계약에 없는 개선은 후속 작업으로 분리한다.

## 완료 조건

1. acceptance 항목이 모두 실제 코드로 연결됐다.
2. 목업이 production route에 남지 않았다.
3. 변경 위험도에 맞는 최소 검증이 통과했다.
4. backend 변경이면 실행 중인 route의 direct/proxy smoke가 통과했다.
5. 사용자에게 변경 내용, 실제 데이터의 현재 상태, 실행하지 못한 검증만 짧게 보고한다.

## 이번 작업에서의 개선점

- `/records/search`처럼 format 분기가 많은 read API는 계약을 먼저 동결하되, regular, 10인, 20인을 작은
  fixture builder로 나누고 마지막에 한 endpoint test에서 조합한다.
- UI 명도 문제처럼 runtime state 오판으로 바로 확인 가능한 버그는 전체 suite보다 해당 state와 HMR 확인을
  먼저 수행한다.
- 새 route의 반복 404는 stale Axum binary 문제이므로 매번 수동 진단하지 않고 reload script를 완료 루틴에
  포함한다.
