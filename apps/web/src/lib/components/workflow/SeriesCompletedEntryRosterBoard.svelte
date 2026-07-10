<script lang="ts">
	import { championIconUrl } from '$lib/datadragon';
	import {
		LANES,
		laneLabel,
		type EntrySlot,
		type Lane,
		type MockEntryParticipant,
		type Team
	} from '$lib/fixtures/workflow';
	import PlayerAvatar from './PlayerAvatar.svelte';
	import PositionIcon from './PositionIcon.svelte';
	import { entrySlot, entryTeamAverage, participantForSlot } from './entryDraft';

	interface TeamCopy {
		name: string;
	}

	interface EntryRosterChampion {
		id: string;
		name: string;
		imageFull: string;
	}

	interface Props {
		participants: MockEntryParticipant[];
		assignments: Record<string, EntrySlot>;
		champions: EntryRosterChampion[];
		title?: string;
		subtitle?: string;
		matchLabel?: string;
		matchFormat?: string;
		blueTeam?: TeamCopy;
		redTeam?: TeamCopy;
		profileIconVersion?: string | null;
	}

	let {
		participants,
		assignments,
		champions,
		title = '내전 엔트리 확정',
		subtitle = '라인업이 확정되었습니다',
		matchLabel = '5v5 SCRIM',
		matchFormat = 'Bo3',
		blueTeam = { name: '1팀' },
		redTeam = { name: '2팀' },
		profileIconVersion = null
	}: Props = $props();

	function assignedPlayer(team: Team, lane: Lane): MockEntryParticipant | null {
		return participantForSlot(participants, assignments, entrySlot(team, lane));
	}

	function championById(championId: string): EntryRosterChampion | null {
		return champions.find((champion) => champion.id === championId) ?? null;
	}

	function teamMembers(team: Team): MockEntryParticipant[] {
		return LANES.map((lane) => assignedPlayer(team, lane)).filter(
			(participant): participant is MockEntryParticipant => Boolean(participant)
		);
	}

	function teamAverage(team: Team): number {
		return entryTeamAverage(team, [...LANES], participants, assignments);
	}

	function teamSummary(team: Team): string {
		return `${teamMembers(team).length}/5 배치 · 평균 ${teamAverage(team)}`;
	}

	function teamSurfaceClass(team: Team): string {
		return team === 'TEAM_1' ? 'bg-emerald-400/[0.026]' : 'bg-amber-400/[0.026]';
	}

	function entryRowClass(team: Team): string {
		return team === 'TEAM_1' ? 'bg-emerald-400/[0.016]' : 'bg-amber-400/[0.016]';
	}
</script>

<div class="overflow-x-auto">
	<section
		class="min-w-[960px] overflow-hidden bg-[var(--color-app-surface)] text-slate-100"
		aria-label="완성 엔트리"
	>
		<header class="border-b border-[var(--color-app-border-muted)] px-5 py-4">
			<div class="min-w-0">
				<p class="text-xs font-semibold uppercase text-slate-500">{matchLabel}</p>
				<h2 class="mt-1 truncate text-3xl font-bold text-slate-50">{title}</h2>
				<p class="mt-1 text-sm font-medium text-slate-500">{subtitle}</p>
			</div>
		</header>

		<div
			class="grid grid-cols-[minmax(0,1fr)_7.5rem_minmax(0,1fr)] gap-px bg-[var(--color-app-border-muted)]"
		>
			<section class="bg-[var(--color-app-surface)] p-3">
				<div class={`px-4 py-3 ${teamSurfaceClass('TEAM_1')}`}>
					<div class="flex items-end justify-between gap-4">
						<div class="min-w-0">
							<h3 class="mt-0.5 truncate text-2xl font-bold text-slate-50">{blueTeam.name}</h3>
						</div>
						<div class="text-right">
							<p class="text-xs font-semibold text-slate-500">{teamSummary('TEAM_1')}</p>
						</div>
					</div>
				</div>

				<div class="mt-2 grid gap-2">
					{#each LANES as lane (lane)}
						{@const player = assignedPlayer('TEAM_1', lane)}
						<div
							class={`grid grid-cols-[3.25rem_3.75rem_minmax(0,1fr)_10rem_5.5rem] items-center gap-3 p-2.5 ${entryRowClass('TEAM_1')}`}
						>
							<div class="grid place-items-center p-2">
								<PositionIcon role={lane} size="md" />
								<span class="mt-1 text-[10px] font-bold text-slate-500">{laneLabel[lane]}</span>
							</div>
							{#if player}
								<PlayerAvatar
									name={player.name}
									role={player.mainRole}
									avatarUrl={player.avatarUrl}
									profileIconId={player.profileIconId}
									{profileIconVersion}
									size="md"
								/>
								<div class="min-w-0">
									<p class="truncate text-xl font-bold text-slate-50">{player.name}</p>
									<p class="mt-0.5 truncate text-xs font-semibold text-slate-500">
										{player.recentRecord}
									</p>
								</div>
								<div class="flex min-w-0 justify-end gap-1">
									{#each player.mainChampions?.slice(0, 5) ?? [] as championId (championId)}
										{@const champion = championById(championId)}
										{#if champion}
											<img
												src={championIconUrl(champion.imageFull, profileIconVersion ?? '')}
												alt=""
												title={champion.name}
												class="size-7 shrink-0 object-cover"
												loading="lazy"
											/>
										{:else}
											<span class="size-7 shrink-0 bg-slate-900/80"></span>
										{/if}
									{/each}
								</div>
								<div class="text-right">
									<p class="font-mono text-xl font-bold tabular-nums text-slate-50">{player.mmr}</p>
									<p class="text-xs font-bold text-slate-500">{player.riotTier ?? '-'}</p>
								</div>
							{:else}
								<div class="col-span-3 text-sm font-semibold text-slate-600">미배치</div>
							{/if}
						</div>
					{/each}
				</div>
			</section>

			<div class="grid place-items-center bg-[var(--color-app-surface)] px-3">
				<div class="grid gap-3 text-center">
					<p class="text-xs font-bold text-slate-500">{matchFormat} Match</p>
					<p class="text-5xl font-black text-slate-50">VS</p>
				</div>
			</div>

			<section class="bg-[var(--color-app-surface)] p-3">
				<div class={`px-4 py-3 ${teamSurfaceClass('TEAM_2')}`}>
					<div class="flex items-end justify-between gap-4">
						<div class="min-w-0">
							<h3 class="mt-0.5 truncate text-2xl font-bold text-slate-50">{redTeam.name}</h3>
						</div>
						<div class="text-right">
							<p class="text-xs font-semibold text-slate-500">{teamSummary('TEAM_2')}</p>
						</div>
					</div>
				</div>

				<div class="mt-2 grid gap-2">
					{#each LANES as lane (lane)}
						{@const player = assignedPlayer('TEAM_2', lane)}
						<div
							class={`grid grid-cols-[3.25rem_3.75rem_minmax(0,1fr)_10rem_5.5rem] items-center gap-3 p-2.5 ${entryRowClass('TEAM_2')}`}
						>
							<div class="grid place-items-center p-2">
								<PositionIcon role={lane} size="md" />
								<span class="mt-1 text-[10px] font-bold text-slate-500">{laneLabel[lane]}</span>
							</div>
							{#if player}
								<PlayerAvatar
									name={player.name}
									role={player.mainRole}
									avatarUrl={player.avatarUrl}
									profileIconId={player.profileIconId}
									{profileIconVersion}
									size="md"
								/>
								<div class="min-w-0">
									<p class="truncate text-xl font-bold text-slate-50">{player.name}</p>
									<p class="mt-0.5 truncate text-xs font-semibold text-slate-500">
										{player.recentRecord}
									</p>
								</div>
								<div class="flex min-w-0 justify-end gap-1">
									{#each player.mainChampions?.slice(0, 5) ?? [] as championId (championId)}
										{@const champion = championById(championId)}
										{#if champion}
											<img
												src={championIconUrl(champion.imageFull, profileIconVersion ?? '')}
												alt=""
												title={champion.name}
												class="size-7 shrink-0 object-cover"
												loading="lazy"
											/>
										{:else}
											<span class="size-7 shrink-0 bg-slate-900/80"></span>
										{/if}
									{/each}
								</div>
								<div class="text-right">
									<p class="font-mono text-xl font-bold tabular-nums text-slate-50">{player.mmr}</p>
									<p class="text-xs font-bold text-slate-500">{player.riotTier ?? '-'}</p>
								</div>
							{:else}
								<div class="col-span-3 text-sm font-semibold text-slate-600">미배치</div>
							{/if}
						</div>
					{/each}
				</div>
			</section>
		</div>
	</section>
</div>
