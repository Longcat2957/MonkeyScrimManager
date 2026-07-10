<script lang="ts">
	import { Lock, X } from 'lucide-svelte';
	import { type MockAuctionTeam } from '$lib/fixtures/workflow';
	import PlayerAvatar from './PlayerAvatar.svelte';

	type MatchId = 'A' | 'B';
	type SlotKey = `${MatchId}_${0 | 1}`;

	interface BracketMatch {
		id: MatchId;
		name: string;
	}

	interface Props {
		teams: MockAuctionTeam[];
		profileIconVersion?: string | null;
	}

	let { teams, profileIconVersion = null }: Props = $props();

	const matches: BracketMatch[] = [
		{ id: 'A', name: '준결승 A' },
		{ id: 'B', name: '준결승 B' }
	];
	const matchSlotIndexes = [0, 1] as const;
	const slotKeys: SlotKey[] = ['A_0', 'A_1', 'B_0', 'B_1'];

	let bracket = $state<Record<SlotKey, string | null>>({
		A_0: null,
		A_1: null,
		B_0: null,
		B_1: null
	});
	let draggedTeamId = $state<string | null>(null);
	let overSlotKey = $state<SlotKey | null>(null);

	const assignedTeamIds = $derived(
		Object.values(bracket).filter((teamId): teamId is string => Boolean(teamId))
	);
	const availableTeams = $derived(teams.filter((team) => !assignedTeamIds.includes(team.id)));
	const ready = $derived(slotKeys.every((slotKey) => Boolean(bracket[slotKey])));

	function teamById(teamId: string | null): MockAuctionTeam | null {
		if (!teamId) return null;
		return teams.find((team) => team.id === teamId) ?? null;
	}

	function slotKey(matchId: MatchId, slot: 0 | 1): SlotKey {
		return `${matchId}_${slot}`;
	}

	function assignTeam(targetSlotKey: SlotKey, teamId: string) {
		bracket = Object.fromEntries(
			slotKeys.map((currentSlotKey) => [
				currentSlotKey,
				currentSlotKey === targetSlotKey
					? teamId
					: bracket[currentSlotKey] === teamId
						? null
						: bracket[currentSlotKey]
			])
		) as Record<SlotKey, string | null>;
	}

	function clearSlot(targetSlotKey: SlotKey) {
		bracket = {
			...bracket,
			[targetSlotKey]: null
		};
	}

	function startTeamDrag(event: globalThis.DragEvent, team: MockAuctionTeam) {
		if (!event.dataTransfer) return;
		draggedTeamId = team.id;
		event.dataTransfer.setData('text/plain', team.id);
		event.dataTransfer.effectAllowed = 'move';
	}

	function clearDragState() {
		draggedTeamId = null;
		overSlotKey = null;
	}

	function handleSlotDragOver(event: globalThis.DragEvent, targetSlotKey: SlotKey) {
		if (!event.dataTransfer || !draggedTeamId) return;
		event.preventDefault();
		event.dataTransfer.dropEffect = 'move';
		overSlotKey = targetSlotKey;
	}

	function handleSlotDrop(event: globalThis.DragEvent, targetSlotKey: SlotKey) {
		if (!event.dataTransfer) return;
		event.preventDefault();
		const teamId = event.dataTransfer.getData('text/plain');
		if (teamId) assignTeam(targetSlotKey, teamId);
		clearDragState();
	}

	function slotClass(targetSlotKey: SlotKey): string {
		if (overSlotKey === targetSlotKey) {
			return 'border-cyan-300/60 bg-cyan-400/12 ring-1 ring-cyan-300/35';
		}
		return 'border-transparent bg-slate-950/35';
	}
</script>

<div class="grid gap-2 pb-2">
	<div class="grid gap-2 xl:grid-cols-[minmax(22rem,28rem)_minmax(0,1fr)]">
		<section
			class="grid h-[min(40rem,calc(100vh-14rem))] min-h-[30rem] grid-rows-[auto_minmax(0,1fr)] border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]"
		>
			<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
				<p class="text-base font-semibold text-slate-100">팀 풀</p>
				<p class="mt-1 text-xs text-slate-500">대진 슬롯으로 드래그</p>
			</div>

			<div
				role="list"
				class="grid min-h-0 content-start gap-2 overflow-x-hidden overflow-y-auto overscroll-contain p-3 pr-[13px]"
			>
				{#each availableTeams as team (team.id)}
					<div
						role="listitem"
						draggable="true"
						ondragstart={(event) => startTeamDrag(event, team)}
						ondragend={clearDragState}
						class={`grid min-h-20 cursor-grab bg-slate-950/20 px-3 py-3 active:cursor-grabbing ${
							draggedTeamId === team.id ? 'opacity-60' : ''
						}`}
					>
						<div class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3">
							<div class="min-w-0">
								<p class="truncate text-xl font-semibold text-slate-100">{team.name}</p>
								<p class="mt-1 truncate text-sm font-medium text-slate-500">{team.captain} 팀장</p>
							</div>
							<div class="flex items-center -space-x-2">
								{#each team.members as member (member.id)}
									<PlayerAvatar
										name={member.name}
										role={member.role}
										avatarUrl={member.avatarUrl}
										profileIconId={member.profileIconId}
										{profileIconVersion}
										size="lg"
										shape="circle"
									/>
								{/each}
							</div>
						</div>
					</div>
				{:else}
					<div
						class="grid min-h-40 place-items-center text-center text-sm font-semibold text-slate-600"
					>
						모든 팀 배치됨
					</div>
				{/each}
			</div>
		</section>

		<section
			class="grid h-[min(40rem,calc(100vh-14rem))] min-h-[30rem] grid-rows-[auto_minmax(0,1fr)_auto] border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]"
		>
			<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
				<p class="text-base font-semibold text-slate-100">대진 설정</p>
				<p class="mt-1 text-xs text-slate-500">4팀을 두 경기 슬롯에 한 번씩 배치</p>
			</div>

			<div
				class="grid min-h-0 content-start gap-3 overflow-x-hidden overflow-y-auto overscroll-contain p-3 pr-[13px]"
			>
				{#each matches as match (match.id)}
					<section class="grid min-h-0 content-start gap-3 bg-slate-950/15 p-3">
						<div class="flex items-center justify-between gap-2">
							<p class="text-sm font-semibold text-cyan-100">{match.name}</p>
							<p class="text-xs font-semibold text-slate-500">BO3</p>
						</div>

						<div class="grid items-stretch gap-2 lg:grid-cols-[minmax(0,1fr)_4rem_minmax(0,1fr)]">
							{#each matchSlotIndexes as slot (slot)}
								{@const currentSlotKey = slotKey(match.id, slot)}
								{@const team = teamById(bracket[currentSlotKey])}
								<div
									class={`grid h-44 grid-rows-[auto_minmax(0,1fr)] gap-3 border px-3 py-3 transition ${slotClass(currentSlotKey)}`}
									ondragover={(event) => handleSlotDragOver(event, currentSlotKey)}
									ondragleave={() => {
										if (overSlotKey === currentSlotKey) overSlotKey = null;
									}}
									ondrop={(event) => handleSlotDrop(event, currentSlotKey)}
									role="group"
									aria-label={`${match.name} ${slot === 0 ? '왼쪽' : '오른쪽'} 팀 슬롯`}
								>
									<div class="flex items-center justify-between gap-2">
										<p class="text-xs font-semibold text-slate-500">
											{slot === 0 ? 'LEFT' : 'RIGHT'}
										</p>
										{#if team}
											<button
												type="button"
												class="grid size-9 place-items-center border border-rose-300/25 bg-rose-500/10 text-rose-200 hover:border-rose-300/45 hover:bg-rose-500/16"
												aria-label={`${team.name} 슬롯 비우기`}
												onclick={() => clearSlot(currentSlotKey)}
											>
												<X size={17} />
											</button>
										{/if}
									</div>

									{#if team}
										<div
											class="grid min-h-0 grid-cols-[minmax(0,1fr)_auto] items-center gap-3 overflow-hidden"
										>
											<div class="min-w-0">
												<p class="truncate text-3xl font-semibold text-slate-100">{team.name}</p>
												<p class="mt-1 truncate text-sm font-medium text-slate-500">
													{team.captain} 팀장
												</p>
											</div>
											<div class="flex -space-x-2">
												{#each team.members as member (member.id)}
													<PlayerAvatar
														name={member.name}
														role={member.role}
														avatarUrl={member.avatarUrl}
														profileIconId={member.profileIconId}
														{profileIconVersion}
														size="lg"
														shape="circle"
													/>
												{/each}
											</div>
										</div>
									{:else}
										<div
											class="grid min-h-0 place-items-center bg-slate-950/25 text-base font-semibold text-slate-600"
										>
											팀 배치
										</div>
									{/if}
								</div>

								{#if slot === 0}
									<div
										class="grid min-h-16 place-items-center text-xl font-black text-slate-500 lg:min-h-44"
									>
										VS
									</div>
								{/if}
							{/each}
						</div>
					</section>
				{/each}
			</div>

			<div class="border-t border-[var(--color-app-border-muted)] p-3">
				<button
					type="button"
					class="inline-flex h-12 w-full items-center justify-center gap-2 border border-emerald-300/35 bg-emerald-400/14 px-4 text-base font-semibold text-emerald-100 hover:bg-emerald-400/20 disabled:cursor-not-allowed disabled:opacity-40"
					disabled={!ready}
				>
					<Lock size={16} />
					대진 확정
				</button>
			</div>
		</section>
	</div>
</div>
