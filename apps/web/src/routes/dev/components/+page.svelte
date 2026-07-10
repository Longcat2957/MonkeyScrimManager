<script lang="ts">
	import { resolve } from '$app/paths';
	import { ArrowRight, BadgeCheck, Crown, Shield, Swords, Trophy, UsersRound } from 'lucide-svelte';
	import ChampionFace from '$lib/components/champion/ChampionFace.svelte';
	import Panel from '$lib/components/layout/Panel.svelte';
	import RowList from '$lib/components/layout/RowList.svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import AuctionBidAssignmentBoard from '$lib/components/workflow/AuctionBidAssignmentBoard.svelte';
	import EntryDraftBoard from '$lib/components/workflow/EntryDraftBoard.svelte';
	import ScrimResultBoard from '$lib/components/workflow/ScrimResultBoard.svelte';
	import type { DashboardSummary, QueueItem } from '$lib/contracts/dashboard';
	import type { ChampionCatalogEntry } from '$lib/datadragon';
	import {
		mockAuctionQueue,
		mockAuctionTeams,
		mockEntryAssignments,
		mockEntryHeadToHead,
		mockEntryParticipants,
		mockEntrySynergies
	} from '$lib/fixtures/workflow';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const summary: DashboardSummary = {
		openRecruitments: 6,
		activeSeries: 3,
		auctionRooms: 1,
		pendingResults: 2
	};

	const queue: QueueItem[] = [
		{
			id: 'r-8412',
			kind: 'recruitment',
			title: '금요 5v5 2팀 편성 대기',
			guildName: 'MSM Seoul',
			status: '모집 10/10',
			priority: 'critical',
			nextAction: '엔트리 편성',
			href: '/recruitments/r-8412/entry',
			updatedAt: '2분 전'
		},
		{
			id: 's-402',
			kind: 'series',
			title: 'Bo3 2세트 밴픽 진행',
			guildName: 'Jungle Diff Lab',
			status: '밴픽',
			priority: 'high',
			nextAction: '밴픽 보드',
			href: '/series/s-402/pick-ban',
			updatedAt: '8분 전'
		},
		{
			id: 'a-118',
			kind: 'auction',
			title: '주말 경매 내전 3라운드 낙찰 확인',
			guildName: 'Rift Night',
			status: '경매 진행',
			priority: 'normal',
			nextAction: '경매 룸',
			href: '/auction/a-118',
			updatedAt: '14분 전'
		},
		{
			id: 's-390',
			kind: 'result',
			title: '3세트 결과 및 MMR 반영 대기',
			guildName: 'MSM Seoul',
			status: '결과 대기',
			priority: 'high',
			nextAction: '결과 입력',
			href: '/series/s-390/result',
			updatedAt: '22분 전'
		}
	];

	const lanes: Array<{
		role: string;
		blueId: string;
		redId: string;
		state: string;
	}> = [
		{ role: 'TOP', blueId: 'Ornn', redId: 'Gwen', state: 'locked' },
		{ role: 'JGL', blueId: 'Vi', redId: 'LeeSin', state: 'open' },
		{ role: 'MID', blueId: 'Azir', redId: 'Taliyah', state: 'locked' },
		{ role: 'BOT', blueId: 'Kaisa', redId: 'Zeri', state: 'open' },
		{ role: 'SUP', blueId: 'Rakan', redId: 'Nautilus', state: 'locked' }
	];

	const activity = [
		'MSM Seoul / r-8412 참가자 10명 도달',
		'Jungle Diff Lab / s-402 레드 진영 밴 2개 저장',
		'Rift Night / a-118 주장 입찰가 갱신',
		'MSM Seoul / 운영자 권한 동기화'
	];

	const coreInteractions = [
		'엔트리 편성: 참가자 풀과 팀/라인 슬롯 사이 DnD, 점유 슬롯 swap, 풀로 되돌리기',
		'엔트리 편성: 선호 라인 기반 자동 배치, 팀 좌우 교체, draft 초기화',
		'경매 드래프트: 현재 대상 타일을 팀 원장에 drop, 정원/포인트 조건 검사',
		'경매 드래프트: DnD 실패를 대비한 버튼 기반 수동 배치',
		'다음 묶음: 픽밴 순서, Hard Fearless 중복 제한, 챔피언 슬롯 입력'
	];

	function queueHref(item: QueueItem): string {
		if (item.kind === 'recruitment') {
			return resolve('/recruitments/[id]/entry', { id: item.id });
		}

		if (item.kind === 'series') {
			return resolve('/series/[id]/pick-ban', { id: item.id });
		}

		if (item.kind === 'auction') {
			return resolve('/auction/[id]', { id: item.id });
		}

		return resolve('/series/[id]/result', { id: item.id });
	}

	function priorityTone(priority: QueueItem['priority']): 'danger' | 'warning' | 'neutral' {
		if (priority === 'critical') return 'danger';
		if (priority === 'high') return 'warning';
		return 'neutral';
	}

	function priorityLabel(priority: QueueItem['priority']): string {
		if (priority === 'critical') return '긴급';
		if (priority === 'high') return '높음';
		return '보통';
	}

	function laneStateLabel(state: string): string {
		if (state === 'locked') return '확정';
		return '대기';
	}

	function champion(id: string): ChampionCatalogEntry {
		return data.championCatalog.byId[id];
	}
</script>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="컴포넌트 점검"
		title="운영 화면 컴포넌트 샘플"
		description="기능별 화면을 깊게 만들기 전에 서버 선택, 운영자 상태, 명령 검색, 작업 흐름 내비게이션이 같은 구조로 작동하는지 확인합니다."
		metrics={[
			{ label: '모집', value: summary.openRecruitments, tone: 'warning' },
			{ label: '내전', value: summary.activeSeries, tone: 'info' },
			{ label: '경매', value: summary.auctionRooms, tone: 'neutral' },
			{ label: '결과', value: summary.pendingResults, tone: 'danger' }
		]}
	/>

	<Panel kicker="v3 핵심 상호작용" title="먼저 컴포넌트로 고정할 로직" compact>
		<details class="group">
			<summary
				class="flex cursor-pointer list-none items-center justify-between gap-3 rounded-sm px-4 py-3 text-sm font-semibold text-slate-200 hover:bg-[var(--color-app-hover)]"
			>
				<span>확인 항목 {coreInteractions.length}개</span>
				<span class="text-xs text-slate-500 group-open:text-cyan-100">펼치기</span>
			</summary>
			<RowList columns="48px minmax(0,1fr)" headings={['순서', '항목']}>
				{#each coreInteractions as interaction, index (interaction)}
					<div class="grid grid-cols-[48px_minmax(0,1fr)] gap-3 px-4 py-3">
						<p class="text-xs font-semibold text-cyan-100">0{index + 1}</p>
						<p class="text-sm text-slate-300">{interaction}</p>
					</div>
				{/each}
			</RowList>
		</details>
	</Panel>

	<Panel compact>
		<EntryDraftBoard
			participants={mockEntryParticipants}
			initialAssignments={mockEntryAssignments}
			synergies={mockEntrySynergies}
			headToHead={mockEntryHeadToHead}
			targetCount={10}
			profileIconVersion={data.championCatalog.version}
		/>
	</Panel>

	<Panel
		kicker="Mock DnD"
		title="경매 현재 대상 배치"
		description="mookbot_v3의 경매 드래프트에서 쓰던 현재 대상 드래그, 팀 원장 drop target, 정원/포인트 검사를 mock 데이터로 확인합니다."
		compact
	>
		<AuctionBidAssignmentBoard
			initialTeams={mockAuctionTeams}
			initialQueue={mockAuctionQueue}
			profileIconVersion={data.championCatalog.version}
		/>
	</Panel>

	<Panel compact>
		<ScrimResultBoard />
	</Panel>

	<div class="grid gap-5 2xl:grid-cols-[minmax(0,1.35fr)_minmax(360px,0.65fr)]">
		<div class="space-y-5">
			<Panel kicker="주요 작업 흐름" title="다음 처리 작업" compact>
				<RowList
					columns="88px minmax(0,1fr) 112px 116px"
					headings={['중요도', '작업', '상태', '이동']}
				>
					{#each queue as item (item.id)}
						<a
							href={queueHref(item)}
							class="grid gap-3 rounded-sm px-4 py-4 hover:bg-[var(--color-app-hover)] md:grid-cols-[88px_minmax(0,1fr)_112px_116px] md:items-center"
						>
							<div>
								<StatusPill
									tone={priorityTone(item.priority)}
									label={priorityLabel(item.priority)}
								/>
							</div>
							<div class="min-w-0">
								<p class="truncate text-sm font-semibold text-slate-50">{item.title}</p>
								<p class="mt-1 flex min-w-0 items-center gap-2 text-xs text-slate-500">
									<Shield size={13} />
									<span class="truncate">{item.guildName}</span>
									<span>/</span>
									<span>{item.updatedAt}</span>
								</p>
							</div>
							<p class="text-xs font-semibold text-cyan-100">{item.status}</p>
							<span
								class="inline-flex items-center justify-between gap-2 text-xs font-semibold text-slate-100"
							>
								{item.nextAction}
								<ArrowRight size={14} />
							</span>
						</a>
					{/each}
				</RowList>
			</Panel>

			<div class="grid gap-5 xl:grid-cols-[minmax(0,1fr)_280px]">
				<Panel
					kicker="작업대 미리보기"
					title="s-402 / 2세트 밴픽 스냅샷"
					description="세부 밴픽 구현이 아니라 표와 행 기반 작업대 밀도, Data Dragon 아이콘 사용 기준을 검증합니다."
					compact
				>
					<div class="overflow-x-auto">
						<table class="w-full min-w-[620px] border-collapse text-sm">
							<thead class="text-xs uppercase text-slate-500">
								<tr class="border-b border-[var(--color-app-border-muted)]">
									<th class="w-20 px-4 py-2 text-left">라인</th>
									<th class="px-4 py-2 text-left text-blue-200">블루</th>
									<th class="px-4 py-2 text-left text-red-200">레드</th>
									<th class="w-28 px-4 py-2 text-left">상태</th>
								</tr>
							</thead>
							<tbody class="divide-y divide-[var(--color-app-border-muted)]">
								{#each lanes as lane (lane.role)}
									<tr>
										<td class="px-4 py-3 text-xs font-semibold text-slate-500">{lane.role}</td>
										<td class="px-4 py-3 text-slate-100">
											<ChampionFace
												champion={champion(lane.blueId)}
												version={data.championCatalog.version}
												showName
											/>
										</td>
										<td class="px-4 py-3 text-slate-100">
											<ChampionFace
												champion={champion(lane.redId)}
												version={data.championCatalog.version}
												showName
											/>
										</td>
										<td class="px-4 py-3">
											<StatusPill
												label={laneStateLabel(lane.state)}
												tone={lane.state === 'locked' ? 'success' : 'neutral'}
											/>
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				</Panel>

				<Panel kicker="서버 현황" title="MSM Seoul" compact>
					<div class="grid grid-cols-2 gap-px bg-[var(--color-app-border-muted)] text-sm">
						<div class="bg-[var(--color-app-surface)] p-4">
							<UsersRound size={17} class="text-cyan-200" />
							<p class="mt-2 text-xl font-semibold">38</p>
							<p class="text-xs text-slate-500">멤버</p>
						</div>
						<div class="bg-[var(--color-app-surface)] p-4">
							<Crown size={17} class="text-amber-200" />
							<p class="mt-2 text-xl font-semibold">4</p>
							<p class="text-xs text-slate-500">운영자</p>
						</div>
						<div class="bg-[var(--color-app-surface)] p-4">
							<Trophy size={17} class="text-emerald-200" />
							<p class="mt-2 text-xl font-semibold">12</p>
							<p class="text-xs text-slate-500">시즌 경기</p>
						</div>
						<div class="bg-[var(--color-app-surface)] p-4">
							<BadgeCheck size={17} class="text-violet-200" />
							<p class="mt-2 text-xl font-semibold">98%</p>
							<p class="text-xs text-slate-500">동기화</p>
						</div>
					</div>
				</Panel>
			</div>
		</div>

		<aside class="space-y-5">
			<Panel kicker="활동" title="최근 운영 이벤트" compact>
				<RowList>
					{#each activity as event, index (event)}
						<div class="flex gap-3 px-4 py-3">
							<span class="mt-1 size-2 shrink-0 bg-cyan-300"></span>
							<div>
								<p class="text-sm text-slate-200">{event}</p>
								<p class="mt-1 text-[11px] text-slate-500">{index + 3}분 전</p>
							</div>
						</div>
					{/each}
				</RowList>
			</Panel>

			<Panel
				kicker="Data Dragon"
				title={`${data.championCatalog.version} / ${data.championCatalog.locale}`}
				description="챔피언 카탈로그와 이미지 프록시는 특정 화면이 아니라 모든 작업대가 공유하는 기반 기능입니다."
				compact
			>
				{#snippet actions()}
					<a
						href={resolve('/dd/catalog')}
						class="inline-flex h-8 items-center gap-2 rounded-sm border border-slate-700 px-3 text-xs font-semibold text-slate-200 hover:border-cyan-400/40 hover:text-cyan-100"
					>
						카탈로그
						<ArrowRight size={14} />
					</a>
				{/snippet}

				<div
					class="grid grid-cols-2 gap-px bg-[var(--color-app-border-muted)] sm:grid-cols-4 2xl:grid-cols-2 min-[1800px]:grid-cols-4"
				>
					{#each data.dashboardChampions as champion (champion.id)}
						<div class="min-w-0 bg-[var(--color-app-surface)] px-3 py-3">
							<ChampionFace {champion} version={data.championCatalog.version} size="md" showName />
							<p class="mt-1 truncate text-[11px] text-slate-500">{champion.id} / {champion.key}</p>
						</div>
					{/each}
				</div>
			</Panel>

			<Panel kicker="화면 틀" title="기능 화면은 아직 가볍게 유지" compact>
				<div class="grid gap-2 p-4 text-sm">
					<a
						class="flex items-center justify-between rounded-sm border border-slate-800 px-3 py-2 text-slate-300 hover:border-slate-700 hover:text-slate-100"
						href={resolve('/recruitments')}
					>
						<span>모집 목록</span>
						<UsersRound size={14} />
					</a>
					<a
						class="flex items-center justify-between rounded-sm border border-slate-800 px-3 py-2 text-slate-300 hover:border-slate-700 hover:text-slate-100"
						href={resolve('/series')}
					>
						<span>시리즈 목록</span>
						<Swords size={14} />
					</a>
					<a
						class="flex items-center justify-between rounded-sm border border-slate-800 px-3 py-2 text-slate-300 hover:border-slate-700 hover:text-slate-100"
						href={resolve('/series/completed')}
					>
						<span>완료 내전</span>
						<Trophy size={14} />
					</a>
				</div>
			</Panel>
		</aside>
	</div>
</div>
