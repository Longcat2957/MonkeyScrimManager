# MonkeyBot Design Direction

작성일: 2026-07-11

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
- MonkeyBot은 전적검색 사이트가 아니라 내전 운영 콘솔이므로, 운영 흐름과 작업대 구조가 우선이다.

## Product Shell

공통 shell이 먼저 책임지는 정보:

- multi-guild context.
- operator/session state.
- command/search area.
- primary workflow navigation.
- shared capability 상태, 예: Data Dragon catalog/proxy.

route는 이 shell 안에서 작업대 내용을 채운다. route마다 별도의 header/nav/status/card 구조를 새로 만들지 않는다.

상단 레이아웃 원칙:

- 전체 app navigation은 최상단 navbar에 둔다.
- 홈 이동은 navbar 메뉴 항목이 아니라 좌측 로고 클릭으로 처리한다.
- 모바일 navbar는 아이콘만 보여준다.
- PC navbar는 명칭만 보여준다.
- 검색은 avatar 옆 검색 버튼으로 열고 닫는다.
- 검색 영역은 펼쳐졌을 때 navbar 아래에 표시하며, navbar와 시각적으로 분리한다.
- 검색 왼쪽 카테고리는 Tabs로 구성하고, `Riot`과 `Custom`으로 이원화한다.
- `Riot` 검색은 Riot ID/소환사명 검색을 우선한다.
- `Custom` 검색은 내전 전적 검색 진입만 담당한다. 검색 패널 안에서 guild별 정보 노출이나 guild dropdown을 제공하지 않는다.
- Custom 검색 이후 어떤 guild의 어떤 전적을 볼지는 이동한 화면에서 사용자가 조정한다.
- 루트 화면이나 전역 shell에 Data Dragon 진입 버튼을 노출하지 않는다.
- lol.ps의 정보 밀도와 상단 구조 감각은 참고하되 그대로 복제하지 않는다.

## UI 문구와 톤

이 서비스는 한국인 사용자를 주 대상으로 한다. 화면에 직접 노출되는 용어는 한국어를 기본으로 한다.

원칙:

- 단순 직역이나 개발자 용어를 피한다.
- `운영 권한`, `감사 기준`, `operation`, `route`, `shell`처럼 딱딱하거나 내부 구현에 가까운 표현은 사용자 화면에 그대로 노출하지 않는다.
- `내 역할`, `최근 변경`, `화면`, `상단 메뉴`, `할 일`처럼 실제 사용자가 자연스럽게 읽는 표현을 우선한다.
- Data Dragon, Discord, champion id, API status code처럼 고유명사나 시스템 식별자는 필요한 경우 그대로 둔다.
- 버튼, 탭, 상태 badge, 빈 상태, 오류 문구는 짧고 명확한 한국어로 작성한다.
- 관리자용 화면이라도 기계적인 보고서 말투보다 운영자가 바로 이해하는 표현을 쓴다.

## Layout Primitive

초기 baseline primitive:

- app shell.
- top navbar.
- separated search bar.
- workspace header.
- panel.
- dense row/list/table frame.
- status pill.

규칙:

- panel은 작업 영역, 반복 row, table, modal 같은 명확한 프레임에만 쓴다.
- 참가자, roster, 감사 로그처럼 길어지는 반복 목록 panel은 고정 px 높이 대신 viewport의 남은 높이를 쓰는 내부 ScrollArea를 사용한다. panel header와 actions는 scroll 영역 밖에 두어 항상 보이게 하고, 목록은 Footer 직전의 남은 영역에서만 스크롤한다.
- card 안의 card는 만들지 않는다.
- route-local 색상, spacing, badge 변형을 늘리지 않는다.
- 사용자가 선택할 수 없는 항목은 `border-radius: 0`으로 처리한다.
- 사용자가 선택 가능한 항목은 약한 `border-radius`를 둔다.
- 현재 사용자가 선택한 항목은 중간 정도의 `border-radius`를 적용해 활성 상태가 분명히 보이게 한다.
- 선택 가능한 버튼, 탭, 아이콘 버튼은 너무 둥글게 만들지 않고 약한 radius를 기본으로 한다. Tailwind 기준 `rounded-sm`을 우선한다.
- Tabs/segmented control처럼 한 컨테이너 안에서 선택면이 움직이는 UI는 현재 선택 항목에 중간 정도의 radius를 둔다. Tailwind 기준 `rounded-md`를 우선한다.
- 모든 페이지 전환은 빈 화면으로 튀지 않게 Skeleton placeholder 또는 Spinner를 제공한다.
- 페이지 전환 로딩 상태와 화면 진입/이탈은 짧고 자연스러운 transition을 적용한다.
- 여기서 Skeleton은 로딩 placeholder 패턴을 뜻한다. Skeleton UI 라이브러리를 뜻하지 않는다.
- 기본 화면에는 의사결정에 필요한 정보만 노출한다.
- 부가 정보, 설명, 상세 지표는 기본 노출하지 않고 사용자가 expand해서 확인하게 한다.
- expand UI는 사용자가 추가 정보를 요청하는 동작이어야 하며, 화면 밀도를 무너뜨리지 않아야 한다.
- border는 정보 층위가 실제로 바뀌는 곳에만 사용한다.
- 같은 영역 안에서 외곽, 헤더, row, card border를 중첩하지 않는다.
- 인접한 컨테이너와 카드의 border가 겹쳐 두껍거나 지저분해 보이면 한쪽 border를 제거하고 gap, 배경 톤, separator 중 하나로 정리한다.
- 반복 카드 내부에서 모든 요소에 border를 두르지 않는다. 카드 외곽, 활성 상태, drop target처럼 의미가 있는 경계만 남긴다.
- 단순 구분은 여백, 배경 톤, gap, typography로 먼저 처리하고 border를 남발하지 않는다.
- dashboard는 metric hero가 아니라 operations queue다.
- Data Dragon catalog와 champion face/icon은 공유 capability로 유지한다.
- route placeholder가 필요하면 공통 route frame을 사용하고, page별 화면처럼 꾸미지 않는다.
- 사용자가 특정 기존 화면 또는 컴포넌트를 기준으로 지시하면 그 구조를 우선한다.
- 기존 사례를 현대화할 때는 workflow, 정보 배치, 상호작용 순서를 유지하고 색상/간격/컴포넌트 문법만 현재 스택에 맞춘다.
- 사용자 지시와 다르게 창의적으로 재구성하지 않는다. 특히 검증용 컴포넌트는 실제 route에 연결하지 않는다.
- 선호 챔피언, 주로 사용한 챔피언처럼 성격이 다른 정보는 한 행 안에 섞지 않고 영역을 분리해 보여준다.

## Workflow Workbench / DnD Rules

엔트리 편성 작업대와 픽밴 작업대는 이후 경매, 라인업, 결과 입력 화면의 root interaction reference로 둔다.

작업대 구조:

- 작업 화면은 source, target, action/inspector 영역을 분리한다.
- source는 후보 풀, 챔피언 패널, 현재 매물처럼 사용자가 움직이거나 선택할 대상을 담는다.
- target은 팀/라인 슬롯, 픽밴 슬롯, 팀 원장처럼 커밋 결과가 남는 고정 영역이다.
- action/inspector는 현재 선택 대상, 필터, 상세 기록, 되돌리기, 저장 상태처럼 작업을 보조하는 정보만 담는다.
- 핵심 작업 영역은 화면의 주인공이어야 하며, 요약 카드나 설명 문구가 작업대보다 앞서면 안 된다.
- 작업대는 full-width에 가깝게 쓰고, 반복 슬롯은 같은 높이와 같은 grid rhythm을 유지한다.
- 작업대 내부 ScrollArea는 스크롤바가 표시될 수 있는 오른쪽에 기본 padding보다 1px 여유를 더 둔다. 콘텐츠가 스크롤바에 붙거나 잘려 보이지 않아야 한다.

상호작용 원칙:

- DnD는 직관적인 기본 조작으로 제공하되 유일한 조작 경로로 두지 않는다.
- coarse pointer 또는 모바일 환경에서는 HTML5 DnD에 기대지 않고 tap-to-select / tap-to-place 흐름을 제공한다.
- 픽밴처럼 drag보다 빠른 작업은 `슬롯 선택 -> 후보 검색/필터 -> 커밋 -> 다음 슬롯` 흐름을 DnD와 같은 작업대 패턴으로 취급한다.
- 선택된 source와 활성 target은 항상 시각적으로 추적 가능해야 한다.
- drag 중인 source는 opacity나 cursor 변화로 이동 중임을 보여준다.
- drop 가능한 target은 hover/over 상태에서 ring, border, background tone으로 즉시 반응한다.
- drop 불가능한 target은 가능한 target과 다른 tone으로 표시하고, 실패 이유는 action 위치 근처에서 짧게 보여준다.
- 이미 점유된 slot에 drop하면 duplicate를 만들지 않고 swap 또는 move 규칙 중 하나를 명확히 적용한다.
- 후보 풀로 되돌리기, 슬롯 해제, 현재 선택 취소처럼 반대 방향 작업도 동일한 수준의 조작으로 제공한다.
- 클릭, Enter/Space, Escape, Tab 같은 keyboard path를 주요 작업 흐름에 맞게 제공한다.
- 대량 입력, 검색, 자동 다음 슬롯처럼 숙련자용 shortcut은 기본 DnD 흐름을 방해하지 않는 보조 수단으로 둔다.

정보 표현 원칙:

- slot/card 안의 기본 정보는 이름, 역할/라인, 현재 상태처럼 의사결정에 필요한 최소 정보로 제한한다.
- 승률, 상대전적, 선호 챔피언, 솔랭 티어 같은 보조 정보는 compact meta 또는 상세/보조 rail에 둔다.
- 서로 다른 성격의 정보는 같은 행에 섞지 않는다. 예: 선호 챔피언과 주 사용 챔피언은 별도 영역으로 분리한다.
- champion face/icon, profile icon, lane icon처럼 식별에 직접 필요한 이미지는 작업대에서 적극적으로 사용한다.
- image 자체는 드래그 대상으로 만들지 않는다. draggable target은 카드/슬롯 단위로 제한한다.
- 빈 슬롯은 빈 상태임을 명확히 보여주되, slot 크기를 바꾸지 않는다.
- 동작 직후 바뀐 항목은 짧은 highlight로 피드백을 주고, layout shift를 만들지 않는다.

도메인 순서:

- 라인 데이터 순서는 TOP / JUNGLE / MID / BOTTOM / SUPPORT를 유지한다. 화면 label은 BOTTOM을 원딜로 표시할 수 있다.
- 양 팀이 대칭으로 비교되는 화면은 좌우 또는 상하 리듬을 유지하고, 팀별 색상은 보조 신호로만 사용한다.
- 픽밴 순서, 라인업 순서, 경매 후보 queue처럼 도메인 순서가 있는 작업은 UI 배열도 그 순서를 따른다.
- 자동 advance가 있는 작업은 현재 위치와 다음 위치가 모두 예측 가능해야 한다.

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
