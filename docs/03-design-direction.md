# MonkeyScrimManager Design Direction

작성일: 2026-07-10

## 현재 결정된 것

현재 확정된 프론트엔드/UI 기반은 다음과 같다.

- SvelteKit.
- Tailwind CSS.
- Bits UI.
- lucide-svelte.

구현 순서는 다음으로 고정한다.

```txt
common layout / overall product structure first
  -> route frame
  -> route-specific working screens
```

page별 placeholder를 먼저 늘리는 방식은 사용하지 않는다. 개별 route는 공통 shell, navigation hierarchy, workspace primitive가 잡힌 뒤 해당 workflow에 필요한 만큼만 확장한다.

## 현재 디자인 레퍼런스

전체적인 시각 감각은 `https://lol.ps/` 같은 League of Legends 전적검색/데이터 포털 사이트에서 많은 영감을 받아 진행한다.

현재 참고하려는 방향:

- dark-first 기반.
- 롤 데이터 사이트다운 정보 밀도.
- 검색, 필터, 랭킹, 티어, 챔피언/유저 식별이 빠른 화면.
- 게임 데이터가 중심에 있고 장식은 보조인 구조.
- e-sports/LoL 맥락은 느껴지지만 과장된 게임 UI는 피하는 방향.

주의:

- `lol.ps`를 그대로 복제하지 않는다.
- 광고/커뮤니티/콘텐츠 포털 성격은 이 프로젝트의 핵심이 아니다.
- MonkeyScrimManager는 전적검색 사이트가 아니라 내전 운영 콘솔이므로, 운영 흐름과 작업대 구조가 우선이다.

## Product Shell

공통 shell이 먼저 책임지는 정보:

- multi-guild context.
- operator/session state.
- command/search area.
- primary workflow navigation.
- operations status rail.
- shared capability 상태, 예: Data Dragon catalog/proxy.

route는 이 shell 안에서 작업대 내용을 채운다. route마다 별도의 header/nav/status/card 구조를 새로 만들지 않는다.

## Layout Primitive

초기 baseline primitive:

- app shell.
- top bar.
- workflow rail/sidebar.
- workspace header.
- panel.
- dense row/list/table frame.
- status pill.

규칙:

- panel은 작업 영역, 반복 row, table, modal 같은 명확한 프레임에만 쓴다.
- card 안의 card는 만들지 않는다.
- route-local 색상, spacing, badge 변형을 늘리지 않는다.
- dashboard는 metric hero가 아니라 operations queue다.
- Data Dragon catalog와 champion face/icon은 공유 capability로 유지한다.
- route placeholder가 필요하면 공통 route frame을 사용하고, page별 화면처럼 꾸미지 않는다.

## 현재 제외하는 것

- shadcn-svelte는 사용하지 않는다.
- Skeleton UI는 사용하지 않는다.
- daisyUI는 사용하지 않는다.
- Discord Activity UI 스타일은 계승하지 않는다.

## 보류 중인 것

다음 항목은 추후 협의 후 문서화한다.

- 전체 시각 방향.
- 색상과 typography.
- route별 화면 구조.
- dashboard, entry editor, pick-ban, auction, admin 화면의 세부 UX.
