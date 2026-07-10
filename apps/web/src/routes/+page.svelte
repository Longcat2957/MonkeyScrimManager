<script lang="ts">
	import { resolve } from '$app/paths';
	import { ArrowRight, BadgeCheck, Crown, Shield, Swords, Trophy, UsersRound } from 'lucide-svelte';
	import ChampionFace from '$lib/components/champion/ChampionFace.svelte';
	import Panel from '$lib/components/layout/Panel.svelte';
	import RowList from '$lib/components/layout/RowList.svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import type { DashboardSummary, QueueItem } from '$lib/contracts/dashboard';
	import type { ChampionCatalogEntry } from '$lib/datadragon';
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
			status: 'OPEN 10/10',
			priority: 'critical',
			nextAction: '엔트리 편성',
			href: '/recruitments/r-8412/entry',
			updatedAt: '2m ago'
		},
		{
			id: 's-402',
			kind: 'series',
			title: 'Bo3 Game 2 pick-ban 진행',
			guildName: 'Jungle Diff Lab',
			status: 'PICK_BAN',
			priority: 'high',
			nextAction: '밴픽 보드',
			href: '/series/s-402/pick-ban',
			updatedAt: '8m ago'
		},
		{
			id: 'a-118',
			kind: 'auction',
			title: '주말 경매 내전 3라운드 낙찰 확인',
			guildName: 'Rift Night',
			status: 'AUCTION LIVE',
			priority: 'normal',
			nextAction: '경매 룸',
			href: '/auction/a-118',
			updatedAt: '14m ago'
		},
		{
			id: 's-390',
			kind: 'result',
			title: 'Game 3 결과 및 MMR 반영 대기',
			guildName: 'MSM Seoul',
			status: 'RESULT_PENDING',
			priority: 'high',
			nextAction: '결과 입력',
			href: '/series/s-390/result',
			updatedAt: '22m ago'
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
		'Jungle Diff Lab / s-402 RED side 밴 2개 저장',
		'Rift Night / a-118 captain bid updated',
		'MSM Seoul / operator role synced'
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

	function champion(id: string): ChampionCatalogEntry {
		return data.championCatalog.byId[id];
	}
</script>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="Operations queue"
		title="multi-guild 작업 흐름을 한 shell에서 처리하는 운영 콘솔"
		description="Route별 화면을 깊게 만들기 전에 guild context, operator state, command/search, workflow navigation, status rail이 같은 구조로 작동하는지 확인합니다."
		metrics={[
			{ label: 'Open', value: summary.openRecruitments, tone: 'warning' },
			{ label: 'Series', value: summary.activeSeries, tone: 'info' },
			{ label: 'Auction', value: summary.auctionRooms, tone: 'neutral' },
			{ label: 'Results', value: summary.pendingResults, tone: 'danger' }
		]}
	/>

	<div class="grid gap-5 2xl:grid-cols-[minmax(0,1.35fr)_minmax(360px,0.65fr)]">
		<div class="space-y-5">
			<Panel kicker="Primary workflow" title="다음 처리 작업" compact>
				<RowList
					columns="88px minmax(0,1fr) 112px 116px"
					headings={['Priority', 'Workspace', 'Status', 'Action']}
				>
					{#each queue as item (item.id)}
						<a
							href={queueHref(item)}
							class="grid gap-3 px-4 py-4 hover:bg-slate-900/80 md:grid-cols-[88px_minmax(0,1fr)_112px_116px] md:items-center"
						>
							<div>
								<StatusPill tone={priorityTone(item.priority)} label={item.priority} />
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
					kicker="Workspace preview"
					title="s-402 / Game 2 draft snapshot"
					description="세부 픽밴 구현이 아니라 table/row 작업대 밀도와 Data Dragon icon 사용 기준을 검증합니다."
					compact
				>
					<div class="overflow-x-auto">
						<table class="w-full min-w-[620px] border-collapse text-sm">
							<thead class="text-xs uppercase text-slate-500">
								<tr class="border-b border-slate-800">
									<th class="w-20 px-4 py-2 text-left">Lane</th>
									<th class="px-4 py-2 text-left text-blue-200">Blue</th>
									<th class="px-4 py-2 text-left text-red-200">Red</th>
									<th class="w-28 px-4 py-2 text-left">State</th>
								</tr>
							</thead>
							<tbody class="divide-y divide-slate-800">
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
												label={lane.state}
												tone={lane.state === 'locked' ? 'success' : 'neutral'}
											/>
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				</Panel>

				<Panel kicker="Guild focus" title="MSM Seoul" compact>
					<div class="grid grid-cols-2 gap-px bg-slate-800 text-sm">
						<div class="bg-[#0b0f16] p-4">
							<UsersRound size={17} class="text-cyan-200" />
							<p class="mt-2 text-xl font-semibold">38</p>
							<p class="text-xs text-slate-500">members</p>
						</div>
						<div class="bg-[#0b0f16] p-4">
							<Crown size={17} class="text-amber-200" />
							<p class="mt-2 text-xl font-semibold">4</p>
							<p class="text-xs text-slate-500">operators</p>
						</div>
						<div class="bg-[#0b0f16] p-4">
							<Trophy size={17} class="text-emerald-200" />
							<p class="mt-2 text-xl font-semibold">12</p>
							<p class="text-xs text-slate-500">season games</p>
						</div>
						<div class="bg-[#0b0f16] p-4">
							<BadgeCheck size={17} class="text-violet-200" />
							<p class="mt-2 text-xl font-semibold">98%</p>
							<p class="text-xs text-slate-500">sync</p>
						</div>
					</div>
				</Panel>
			</div>
		</div>

		<aside class="space-y-5">
			<Panel kicker="Activity" title="최근 운영 이벤트" compact>
				<RowList>
					{#each activity as event, index (event)}
						<div class="flex gap-3 px-4 py-3">
							<span class="mt-1 size-2 shrink-0 bg-cyan-300"></span>
							<div>
								<p class="text-sm text-slate-200">{event}</p>
								<p class="mt-1 text-[11px] text-slate-500">{index + 3}m ago</p>
							</div>
						</div>
					{/each}
				</RowList>
			</Panel>

			<Panel
				kicker="Data Dragon"
				title={`${data.championCatalog.version} / ${data.championCatalog.locale}`}
				description="챔피언 카탈로그와 이미지 proxy는 route-specific feature가 아니라 모든 작업대가 공유하는 기반 기능입니다."
				compact
			>
				{#snippet actions()}
					<a
						href={resolve('/dd/catalog')}
						class="inline-flex h-8 items-center gap-2 border border-slate-700 px-3 text-xs font-semibold text-slate-200 hover:border-cyan-400/40 hover:text-cyan-100"
					>
						Catalog
						<ArrowRight size={14} />
					</a>
				{/snippet}

				<div
					class="grid grid-cols-2 gap-px bg-slate-800 sm:grid-cols-4 2xl:grid-cols-2 min-[1800px]:grid-cols-4"
				>
					{#each data.dashboardChampions as champion (champion.id)}
						<div class="min-w-0 bg-[#0b0f16] px-3 py-3">
							<ChampionFace {champion} version={data.championCatalog.version} size="md" showName />
							<p class="mt-1 truncate text-[11px] text-slate-500">{champion.id} / {champion.key}</p>
						</div>
					{/each}
				</div>
			</Panel>

			<Panel kicker="Route frame" title="기능 페이지는 아직 얕게 유지" compact>
				<div class="grid gap-2 p-4 text-sm">
					<a
						class="flex items-center justify-between border border-slate-800 px-3 py-2 text-slate-300 hover:border-slate-700 hover:text-slate-100"
						href={resolve('/recruitments')}
					>
						<span>모집 목록</span>
						<UsersRound size={14} />
					</a>
					<a
						class="flex items-center justify-between border border-slate-800 px-3 py-2 text-slate-300 hover:border-slate-700 hover:text-slate-100"
						href={resolve('/series')}
					>
						<span>시리즈 목록</span>
						<Swords size={14} />
					</a>
					<a
						class="flex items-center justify-between border border-slate-800 px-3 py-2 text-slate-300 hover:border-slate-700 hover:text-slate-100"
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
