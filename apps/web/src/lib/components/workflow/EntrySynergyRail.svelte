<script lang="ts">
	import {
		laneLabel,
		teamLabel,
		type EntrySlot,
		type Lane,
		type MockEntryParticipant,
		type MockEntrySynergyRecord,
		type SynergyKind,
		type Team
	} from '$lib/fixtures/workflow';
	import { entrySlot, participantForSlot } from './entryDraft';
	import PositionIcon from './PositionIcon.svelte';

	interface Props {
		team: Team;
		activeLanes: Lane[];
		assignments: Record<string, EntrySlot>;
		participants: MockEntryParticipant[];
		synergies: MockEntrySynergyRecord[];
	}

	let { team, activeLanes, assignments, participants, synergies }: Props = $props();

	const rows: Array<{
		kind: SynergyKind;
		roleA: Lane;
		roleB: Lane;
		rowClass: string;
	}> = [
		{ kind: 'TOP_JUNGLE', roleA: 'TOP', roleB: 'JUNGLE', rowClass: 'row-start-1' },
		{ kind: 'JUNGLE_MID', roleA: 'JUNGLE', roleB: 'MID', rowClass: 'row-start-2' },
		{ kind: 'BOTTOM_SUPPORT', roleA: 'BOTTOM', roleB: 'SUPPORT', rowClass: 'row-start-4' }
	];

	function assignedUser(lane: Lane): MockEntryParticipant | null {
		return participantForSlot(participants, assignments, entrySlot(team, lane));
	}

	function recordFor(
		kind: SynergyKind,
		userA: MockEntryParticipant | null,
		userB: MockEntryParticipant | null
	): MockEntrySynergyRecord | null {
		if (!userA || !userB) return null;
		return (
			synergies.find(
				(record) =>
					record.team === team &&
					record.kind === kind &&
					record.userAId === userA.id &&
					record.userBId === userB.id
			) ?? null
		);
	}

	function winRate(record: MockEntrySynergyRecord): number {
		if (record.plays === 0) return 0;
		return Math.round((record.wins / record.plays) * 100);
	}

	function tone(record: MockEntrySynergyRecord | null): 'success' | 'danger' | 'neutral' {
		if (!record || record.plays === 0) return 'neutral';
		return record.wins >= record.losses ? 'success' : 'danger';
	}

	function label(roleA: Lane, roleB: Lane): string {
		return `${laneLabel[roleA]}-${laneLabel[roleB]}`;
	}
</script>

<aside class="min-w-0 bg-[var(--color-app-surface)]">
	<div
		class="flex h-11 items-center justify-between gap-2 overflow-hidden px-2 py-2 sm:h-[3.125rem]"
	>
		<p class="truncate text-xs font-semibold text-slate-400">{teamLabel[team]} 시너지</p>
	</div>
	<div
		class="grid grid-rows-[repeat(5,4.25rem)] gap-1.5 overflow-hidden px-1.5 py-1 sm:grid-rows-[repeat(5,4.75rem)] sm:px-2"
	>
		{#each rows as row (row.kind)}
			{#if activeLanes.includes(row.roleA) && activeLanes.includes(row.roleB)}
				{@const userA = assignedUser(row.roleA)}
				{@const userB = assignedUser(row.roleB)}
				{@const record = recordFor(row.kind, userA, userB)}
				{@const cardTone = tone(record)}
				{@const assigned = Boolean(userA && userB)}
				<div class={`synergy-cell h-full min-h-0 ${row.rowClass}`}>
					<div
						class={`synergy-card grid h-full min-h-0 items-center overflow-hidden border px-1.5 py-1.5 text-xs transition sm:px-2 ${
							cardTone === 'success'
								? 'rounded-md border-emerald-300/35 bg-emerald-400/8 text-emerald-100'
								: cardTone === 'danger'
									? 'rounded-md border-red-300/35 bg-red-400/8 text-red-100'
									: userA && userB
										? 'rounded-sm border-slate-700 bg-slate-900/55 text-slate-300'
										: 'border-slate-800 bg-slate-950/20 text-slate-600'
						}`}
						title={record
							? `${label(row.roleA, row.roleB)}: ${record.wins}-${record.losses} (${record.plays}게임)`
							: `${label(row.roleA, row.roleB)}: 기록 없음`}
					>
						<div class="synergy-icons relative h-9 w-9">
							<span
								class={`absolute top-0 left-0 grid size-6 place-items-center border border-slate-800 bg-slate-950/45 transition-opacity ${
									assigned ? 'opacity-60' : 'opacity-35'
								}`}
							>
								<PositionIcon role={row.roleA} size="sm" />
							</span>
							<span
								class={`absolute right-0 bottom-0 grid size-6 place-items-center border border-slate-800 bg-slate-950/45 transition-opacity ${
									assigned ? 'opacity-60' : 'opacity-35'
								}`}
							>
								<PositionIcon role={row.roleB} size="sm" />
							</span>
						</div>
						<div class="min-w-0">
							<div class="flex min-w-0 items-center justify-between gap-1">
								<span class="min-w-0 truncate text-[11px] font-semibold opacity-70">
									{record && record.plays > 0 ? `${record.plays}게임` : label(row.roleA, row.roleB)}
								</span>
								{#if record && record.plays > 0}
									<span class="shrink-0 text-[11px] font-semibold tabular-nums"
										>{winRate(record)}%</span
									>
								{/if}
							</div>
							{#if record && record.plays > 0}
								<div class="mt-0.5 min-w-0 overflow-hidden tabular-nums">
									<span class="block truncate font-semibold">{record.wins}승 {record.losses}패</span
									>
								</div>
							{:else if userA && userB}
								<p class="mt-0.5 truncate text-[11px] text-slate-500">기록 없음</p>
							{:else}
								<p class="mt-0.5 truncate text-[11px]">배정 대기</p>
							{/if}
						</div>
					</div>
				</div>
			{/if}
		{/each}
	</div>
</aside>

<style>
	.synergy-cell {
		container-type: inline-size;
	}

	.synergy-card {
		grid-template-columns: minmax(0, 1fr);
		gap: 0;
	}

	.synergy-icons {
		display: none;
	}

	@container (min-width: 7.5rem) {
		.synergy-card {
			grid-template-columns: 2.25rem minmax(0, 1fr);
			gap: 0.5rem;
		}

		.synergy-icons {
			display: block;
		}
	}
</style>
