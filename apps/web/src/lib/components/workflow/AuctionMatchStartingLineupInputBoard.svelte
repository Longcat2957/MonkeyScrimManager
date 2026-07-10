<script lang="ts">
	import { Lock, X } from 'lucide-svelte';
	import {
		LANES,
		laneLabel,
		type Lane,
		type MockAuctionMember,
		type MockAuctionTeam
	} from '$lib/fixtures/workflow';
	import PlayerAvatar from './PlayerAvatar.svelte';
	import PositionIcon from './PositionIcon.svelte';

	type MatchSide = 'LEFT' | 'RIGHT';
	type Lineup = Record<MatchSide, Partial<Record<Lane, string>>>;

	interface Props {
		leftTeam: MockAuctionTeam;
		rightTeam: MockAuctionTeam;
		profileIconVersion?: string | null;
	}

	let { leftTeam, rightTeam, profileIconVersion = null }: Props = $props();

	let lineup = $state<Lineup>({
		LEFT: {},
		RIGHT: {}
	});
	let draggedMember = $state<{ side: MatchSide; memberId: string } | null>(null);
	let overSlot = $state<{ side: MatchSide; lane: Lane } | null>(null);

	const ready = $derived(countFilled(lineup) === LANES.length * 2);

	function teamForSide(side: MatchSide): MockAuctionTeam {
		return side === 'LEFT' ? leftTeam : rightTeam;
	}

	function assignedIds(side: MatchSide): string[] {
		return Object.values(lineup[side]).filter((memberId): memberId is string => Boolean(memberId));
	}

	function availableMembers(side: MatchSide): MockAuctionMember[] {
		const assigned = new Set(assignedIds(side));
		return teamForSide(side).members.filter((member) => !assigned.has(member.id));
	}

	function memberById(side: MatchSide, memberId: string | undefined): MockAuctionMember | null {
		if (!memberId) return null;
		return teamForSide(side).members.find((member) => member.id === memberId) ?? null;
	}

	function assignMember(side: MatchSide, lane: Lane, memberId: string) {
		const nextSideLineup = Object.fromEntries(
			Object.entries(lineup[side]).filter(([, assignedId]) => assignedId !== memberId)
		) as Partial<Record<Lane, string>>;
		nextSideLineup[lane] = memberId;

		lineup = {
			...lineup,
			[side]: nextSideLineup
		};
	}

	function clearSlot(side: MatchSide, lane: Lane) {
		const nextSideLineup = { ...lineup[side] };
		delete nextSideLineup[lane];
		lineup = {
			...lineup,
			[side]: nextSideLineup
		};
	}

	function startMemberDrag(
		event: globalThis.DragEvent,
		side: MatchSide,
		member: MockAuctionMember
	) {
		if (!event.dataTransfer) return;
		draggedMember = { side, memberId: member.id };
		event.dataTransfer.setData('text/plain', member.id);
		event.dataTransfer.effectAllowed = 'move';
	}

	function clearDragState() {
		draggedMember = null;
		overSlot = null;
	}

	function handleSlotDragOver(event: globalThis.DragEvent, side: MatchSide, lane: Lane) {
		if (!event.dataTransfer || !draggedMember || draggedMember.side !== side) return;
		event.preventDefault();
		event.dataTransfer.dropEffect = 'move';
		overSlot = { side, lane };
	}

	function handleSlotDrop(event: globalThis.DragEvent, side: MatchSide, lane: Lane) {
		if (!event.dataTransfer || !draggedMember || draggedMember.side !== side) return;
		event.preventDefault();
		const memberId = event.dataTransfer.getData('text/plain');
		if (memberId) assignMember(side, lane, memberId);
		clearDragState();
	}

	function slotClass(side: MatchSide, lane: Lane): string {
		if (overSlot?.side === side && overSlot.lane === lane) {
			return 'border-cyan-300/60 bg-cyan-400/12 ring-1 ring-cyan-300/35';
		}
		return 'border-transparent bg-slate-950/35';
	}

	function countFilled(currentLineup: Lineup): number {
		return LANES.reduce(
			(sum, lane) => sum + (currentLineup.LEFT[lane] ? 1 : 0) + (currentLineup.RIGHT[lane] ? 1 : 0),
			0
		);
	}

	function sideLabel(side: MatchSide): string {
		return teamForSide(side).name;
	}
</script>

<div class="grid gap-2 pb-2">
	<div class="grid gap-2 xl:grid-cols-2">
		{#each ['LEFT', 'RIGHT'] as side (side)}
			{@const typedSide = side as MatchSide}
			{@const team = teamForSide(typedSide)}
			<section
				class="grid h-[min(42rem,calc(100vh-14rem))] min-h-[32rem] grid-rows-[auto_minmax(0,1fr)] border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]"
			>
				<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
					<p class="text-base font-semibold text-slate-100">{team.name}</p>
					<p class="mt-1 text-xs text-slate-500">{team.captain} 팀장</p>
				</div>

				<div class="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] gap-3 p-3">
					<div class="grid gap-1.5">
						{#each LANES as lane (lane)}
							{@const assigned = memberById(typedSide, lineup[typedSide][lane])}
							<div
								class={`grid h-16 grid-cols-[auto_minmax(0,1fr)_2.25rem] items-center gap-2 overflow-hidden border px-3 py-2 transition ${slotClass(typedSide, lane)}`}
								ondragover={(event) => handleSlotDragOver(event, typedSide, lane)}
								ondragleave={() => {
									if (overSlot?.side === typedSide && overSlot.lane === lane) overSlot = null;
								}}
								ondrop={(event) => handleSlotDrop(event, typedSide, lane)}
								role="group"
								aria-label={`${team.name} ${laneLabel[lane]} 라인업 슬롯`}
							>
								<PositionIcon role={lane} size="md" />
								{#if assigned}
									<div class="flex min-w-0 items-center gap-2 overflow-hidden">
										<PlayerAvatar
											name={assigned.name}
											role={assigned.role}
											avatarUrl={assigned.avatarUrl}
											profileIconId={assigned.profileIconId}
											{profileIconVersion}
											size="sm"
										/>
										<div class="min-w-0">
											<p class="truncate text-base font-semibold text-slate-100">
												{assigned.name}
											</p>
											<p class="truncate text-xs text-slate-500">
												{assigned.riotTier ?? '-'} · MMR {assigned.mmr}
											</p>
										</div>
									</div>
									<button
										type="button"
										class="grid size-9 place-items-center border border-rose-300/25 bg-rose-500/10 text-rose-200 hover:border-rose-300/45 hover:bg-rose-500/16"
										aria-label={`${laneLabel[lane]} 슬롯 비우기`}
										onclick={() => clearSlot(typedSide, lane)}
									>
										<X size={17} />
									</button>
								{:else}
									<p class="min-w-0 truncate text-sm font-semibold text-slate-600">
										{laneLabel[lane]} 배치
									</p>
									<span class="size-9"></span>
								{/if}
							</div>
						{/each}
					</div>

					<div class="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] gap-1.5">
						<p class="text-xs font-semibold text-slate-500">{sideLabel(typedSide)} 팀원 풀</p>
						<div
							role="list"
							class="grid min-h-0 content-start gap-1.5 overflow-x-hidden overflow-y-auto overscroll-contain pr-[13px]"
						>
							{#each availableMembers(typedSide) as member (member.id)}
								<div
									role="listitem"
									draggable="true"
									ondragstart={(event) => startMemberDrag(event, typedSide, member)}
									ondragend={clearDragState}
									class={`grid h-12 cursor-grab grid-cols-[auto_minmax(0,1fr)_1.75rem] items-center gap-2 overflow-hidden bg-slate-950/20 px-2 py-1.5 active:cursor-grabbing ${
										draggedMember?.memberId === member.id ? 'opacity-60' : ''
									}`}
								>
									<PlayerAvatar
										name={member.name}
										role={member.role}
										avatarUrl={member.avatarUrl}
										profileIconId={member.profileIconId}
										{profileIconVersion}
										size="sm"
									/>
									<div class="min-w-0">
										<p class="truncate text-sm font-semibold text-slate-100">{member.name}</p>
										<p class="truncate text-xs text-slate-500">
											{member.riotTier ?? '-'} · MMR {member.mmr}
										</p>
									</div>
									<PositionIcon role={member.role} size="sm" />
								</div>
							{:else}
								<div
									class="grid h-12 place-items-center overflow-hidden text-sm font-semibold text-slate-600"
								>
									모두 배치됨
								</div>
							{/each}
						</div>
					</div>
				</div>
			</section>
		{/each}
	</div>

	<button
		type="button"
		class="inline-flex h-12 items-center justify-center gap-2 border border-emerald-300/35 bg-emerald-400/14 px-4 text-base font-semibold text-emerald-100 hover:bg-emerald-400/20 disabled:cursor-not-allowed disabled:opacity-40"
		disabled={!ready}
	>
		<Lock size={16} />
		라인업 확정
	</button>
</div>
