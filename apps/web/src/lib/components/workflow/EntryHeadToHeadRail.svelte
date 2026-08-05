<script lang="ts">
	import {
		type EntrySlot,
		laneLabel,
		type Lane,
		type MockEntryParticipant
	} from '$lib/fixtures/workflow';
	import type { EntryDraftHeadToHeadRecord } from '$lib/contracts/entryDraft';
	import { entrySlot, participantForSlot } from './entryDraft';

	interface Props {
		activeLanes: Lane[];
		assignments: Record<string, EntrySlot>;
		participants: MockEntryParticipant[];
		headToHead: EntryDraftHeadToHeadRecord[];
	}

	let { activeLanes, assignments, participants, headToHead }: Props = $props();

	function assignedUser(team: 'TEAM_1' | 'TEAM_2', lane: Lane): MockEntryParticipant | null {
		return participantForSlot(participants, assignments, entrySlot(team, lane));
	}

	function recordFor(
		lane: Lane,
		team1User: MockEntryParticipant | null,
		team2User: MockEntryParticipant | null
	): { plays: number; wins: number; losses: number } | null {
		if (!team1User || !team2User) return null;
		const record = headToHead.find(
			(item) =>
				item.lane === lane &&
				((item.userAId === team1User.id && item.userBId === team2User.id) ||
					(item.userAId === team2User.id && item.userBId === team1User.id))
		);
		if (!record) return null;
		if (record.userAId === team1User.id) {
			return { plays: record.plays, wins: record.userAWins, losses: record.userBWins };
		}
		return { plays: record.plays, wins: record.userBWins, losses: record.userAWins };
	}

	function tone(record: { wins: number; losses: number } | null): 'success' | 'danger' | 'neutral' {
		if (!record || record.wins === record.losses) return 'neutral';
		return record.wins > record.losses ? 'success' : 'danger';
	}

	function symbol(record: { wins: number; losses: number }): '>' | '<' | '=' {
		if (record.wins > record.losses) return '>';
		if (record.wins < record.losses) return '<';
		return '=';
	}

	function lead(record: { wins: number; losses: number }): string {
		const diff = Math.abs(record.wins - record.losses);
		return diff === 0 ? '동률' : `${diff}승 우위`;
	}
</script>

<aside class="min-w-0 bg-[var(--color-app-surface)]">
	<div class="flex h-11 items-center justify-center overflow-hidden px-2 py-2 sm:h-[3.125rem]">
		<p class="truncate text-xs font-semibold text-slate-400">상대전적</p>
	</div>
	<div
		class="grid grid-rows-[repeat(5,4.25rem)] gap-1.5 overflow-hidden px-1.5 py-1 sm:grid-rows-[repeat(5,4.75rem)] sm:px-2"
	>
		{#each activeLanes as lane (lane)}
			{@const team1User = assignedUser('TEAM_1', lane)}
			{@const team2User = assignedUser('TEAM_2', lane)}
			{@const record = recordFor(lane, team1User, team2User)}
			{@const cardTone = tone(record)}
			<div
				class={`grid h-full min-h-0 place-items-center overflow-hidden border px-1.5 py-1.5 text-center text-xs transition ${
					cardTone === 'success'
						? 'rounded-md border-blue-300/35 bg-blue-400/8 text-blue-100'
						: cardTone === 'danger'
							? 'rounded-md border-red-300/35 bg-red-400/8 text-red-100'
							: team1User && team2User
								? 'rounded-sm border-slate-700 bg-slate-900/55 text-slate-300'
								: 'border-slate-800 bg-slate-950/20 text-slate-600'
				}`}
				title={record
					? `${laneLabel[lane]} 상대전적: ${record.plays}게임 ${record.wins}승 ${record.losses}패, ${lead(record)}`
					: `${laneLabel[lane]} 상대전적 없음`}
			>
				<div class="min-w-0">
					{#if record}
						<p class="text-xl font-semibold leading-none tabular-nums">{symbol(record)}</p>
						<p class="mt-1 truncate text-[11px] font-semibold tabular-nums">{lead(record)}</p>
					{:else if team1User && team2User}
						<p class="truncate text-[11px] text-slate-500">기록 없음</p>
					{:else}
						<p class="truncate text-[11px] text-slate-600">대기</p>
					{/if}
				</div>
			</div>
		{/each}
	</div>
</aside>
