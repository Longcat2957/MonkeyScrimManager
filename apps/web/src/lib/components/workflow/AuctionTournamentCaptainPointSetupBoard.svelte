<script lang="ts">
	import { Check, LoaderCircle, Play, Save, Search } from 'lucide-svelte';
	import { untrack } from 'svelte';
	import type {
		AuctionSetupDraft,
		AuctionSetupConfiguredTeam,
		AuctionSetupParticipant,
		AuctionSetupTeam
	} from '$lib/contracts/auctionTournamentSetup';
	import type { ChampionPreferenceLane } from '$lib/contracts/workflow';
	import PositionIcon from './PositionIcon.svelte';

	export type AuctionSetupBoardCommand = {
		expectedRevision: number;
		teams: AuctionSetupConfiguredTeam[];
	};

	interface Props {
		draft: AuctionSetupDraft;
		participants: AuctionSetupParticipant[];
		canEdit?: boolean;
		canCreateTournament?: boolean;
		busy?: boolean;
		onSave?: (command: AuctionSetupBoardCommand) => void | Promise<void>;
		onCreate?: (command: AuctionSetupBoardCommand) => void | Promise<void>;
	}

	let {
		draft,
		participants,
		canEdit = false,
		canCreateTournament = false,
		busy = false,
		onSave,
		onCreate
	}: Props = $props();

	const laneLabels: Record<ChampionPreferenceLane, string> = {
		TOP: '탑',
		JUNGLE: '정글',
		MID: '미드',
		BOTTOM: '바텀',
		SUPPORT: '서포트'
	};

	function sortedDraftTeams(source: AuctionSetupDraft): AuctionSetupTeam[] {
		return source.teams.slice().sort((left, right) => left.teamIndex - right.teamIndex);
	}

	function captainIdsFromDraft(source: AuctionSetupDraft): string[] {
		return sortedDraftTeams(source).flatMap((team) =>
			team.captainUserId === null ? [] : [team.captainUserId]
		);
	}

	function pointInputsFromDraft(source: AuctionSetupDraft): string[] {
		return sortedDraftTeams(source).map((team) => String(team.initialPoints));
	}

	let appliedRevision = $state(untrack(() => draft.revision));
	let captainIds = $state<string[]>(untrack(() => captainIdsFromDraft(draft)));
	let pointInputs = $state<string[]>(untrack(() => pointInputsFromDraft(draft)));
	let query = $state('');

	$effect(() => {
		if (draft.revision === appliedRevision) return;
		appliedRevision = draft.revision;
		captainIds = captainIdsFromDraft(draft);
		pointInputs = pointInputsFromDraft(draft);
		query = '';
	});

	const teams = $derived(sortedDraftTeams(draft));
	const teamCount = $derived(draft.teamCount);
	const normalizedQuery = $derived(query.trim().toLocaleLowerCase('ko-KR'));
	const participantsById = $derived(
		new Map(participants.map((participant) => [participant.id, participant]))
	);
	const selectedCaptains = $derived(
		captainIds
			.map((id) => participantsById.get(id))
			.filter((participant): participant is AuctionSetupParticipant => Boolean(participant))
	);
	const teamPoints = $derived(teams.map((_, index) => pointInputNumber(pointInputs[index] ?? '')));
	const currentTeams = $derived(
		teams.map((team, index) => ({
			...team,
			captainUserId: captainIds[index] ?? null,
			initialPoints: pointInputNumber(pointInputs[index] ?? '') ?? team.initialPoints
		}))
	);
	const isDirty = $derived(
		currentTeams.some((team, index) => {
			const persisted = teams[index];
			return (
				persisted !== undefined &&
				(team.captainUserId !== persisted.captainUserId ||
					team.initialPoints !== persisted.initialPoints)
			);
		})
	);
	const ready = $derived(
		captainIds.length === teamCount &&
			selectedCaptains.length === teamCount &&
			teamPoints.length === teamCount &&
			teamPoints.every((point) => point !== null)
	);
	const canSave = $derived(Boolean(onSave) && canEdit && !busy && ready && isDirty);
	const canStart = $derived(Boolean(onCreate) && canCreateTournament && !busy && ready && !isDirty);
	const saveLabel = $derived(isDirty ? '설정 저장' : ready ? '저장됨' : '설정 대기');
	const filteredParticipants = $derived(
		participants.filter((participant) => {
			if (!normalizedQuery) return true;
			return (
				participant.displayName.toLocaleLowerCase('ko-KR').includes(normalizedQuery) ||
				declaredLaneLabel(participant.declaredLane)
					.toLocaleLowerCase('ko-KR')
					.includes(normalizedQuery)
			);
		})
	);

	function pointInputNumber(value: string): number | null {
		const parsed = Number(value.trim());
		if (!Number.isSafeInteger(parsed) || parsed <= 0) return null;
		return parsed;
	}

	function participantInitial(displayName: string): string {
		return displayName.slice(0, 1).toLocaleUpperCase('ko-KR');
	}

	function declaredLaneLabel(lane: ChampionPreferenceLane | null): string {
		return lane ? laneLabels[lane] : '미설정';
	}

	function toggleCaptain(participantId: string) {
		if (!canEdit || busy) return;
		if (captainIds.includes(participantId)) {
			captainIds = captainIds.filter((id) => id !== participantId);
			return;
		}
		if (captainIds.length >= teamCount) return;
		captainIds = [...captainIds, participantId];
	}

	function updateTeamPoint(index: number, value: string) {
		if (!canEdit || busy) return;
		pointInputs = pointInputs.map((point, pointIndex) => (pointIndex === index ? value : point));
	}

	function captainOrder(participantId: string): number | null {
		const index = captainIds.indexOf(participantId);
		return index === -1 ? null : index + 1;
	}

	function command(): AuctionSetupBoardCommand {
		const configuredTeams = currentTeams.map((team) => {
			if (team.captainUserId === null) {
				throw new Error('auction_setup_incomplete');
			}
			return { ...team, captainUserId: team.captainUserId };
		});
		return { expectedRevision: draft.revision, teams: configuredTeams };
	}

	async function save() {
		if (!canSave || !onSave) return;
		await onSave(command());
	}

	async function startTournament() {
		if (!canStart || !onCreate) return;
		await onCreate(command());
	}
</script>

<div class="grid gap-2 pb-2" aria-busy={busy}>
	<div class="grid gap-2 xl:grid-cols-[minmax(22rem,30rem)_minmax(0,1fr)]">
		<section
			class="grid grid-rows-[auto_minmax(0,1fr)] border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] xl:h-[min(42rem,calc(100dvh-16rem))] xl:min-h-[30rem]"
		>
			<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
				<label
					class="grid h-10 grid-cols-[auto_minmax(0,1fr)] items-center gap-2 border border-[var(--color-app-border-muted)] bg-slate-950/35 px-3"
				>
					<Search size={15} class="text-slate-500" aria-hidden="true" />
					<input
						class="min-w-0 bg-transparent text-sm font-semibold text-slate-100 outline-none placeholder:text-slate-600"
						placeholder="참가자 검색"
						bind:value={query}
					/>
				</label>
			</div>

			<div
				role="list"
				class="grid min-h-0 content-start gap-1.5 p-2 pr-[9px] xl:overflow-x-hidden xl:overflow-y-auto xl:overscroll-contain"
			>
				{#each filteredParticipants as participant (participant.id)}
					{@const order = captainOrder(participant.id)}
					<div role="listitem" class="min-w-0">
						<button
							type="button"
							class={`grid min-h-14 w-full grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 border px-2 py-2 text-left transition ${
								order
									? 'border-amber-300/35 bg-amber-300/10'
									: 'border-transparent bg-slate-950/20 hover:border-cyan-300/25 hover:bg-cyan-400/5'
							}`}
							onclick={() => toggleCaptain(participant.id)}
							disabled={!canEdit || busy || (!order && captainIds.length >= teamCount)}
							aria-pressed={Boolean(order)}
						>
							<span
								class="grid size-8 shrink-0 place-items-center border border-[var(--color-app-border)] bg-[var(--color-app-control)] text-xs font-semibold text-slate-300"
							>
								{participantInitial(participant.displayName)}
							</span>
							<span class="min-w-0">
								<span class="block truncate text-base font-semibold text-slate-100">
									{participant.displayName}
								</span>
								<span class="block truncate text-xs font-medium text-slate-500">
									신청 라인 · {declaredLaneLabel(participant.declaredLane)}
								</span>
							</span>
							<span class="grid min-w-16 justify-items-end gap-1">
								{#if participant.declaredLane}
									<PositionIcon role={participant.declaredLane} size="sm" />
								{/if}
								{#if order}
									<span class="text-xs font-bold text-amber-100">{order}팀장</span>
								{:else}
									<span class="text-xs font-semibold text-slate-500">후보</span>
								{/if}
							</span>
						</button>
					</div>
				{:else}
					<div class="grid min-h-40 place-items-center text-center text-sm text-slate-500">
						검색 결과 없음
					</div>
				{/each}
			</div>
		</section>

		<section
			class="grid grid-rows-[auto_minmax(0,1fr)_auto] border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] xl:h-[min(42rem,calc(100dvh-16rem))] xl:min-h-[30rem]"
		>
			<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
				<p class="text-base font-semibold text-slate-100">토너먼트 시작 설정</p>
				<p class="mt-1 text-xs text-slate-500">
					팀장과 팀별 시작 포인트를 저장한 뒤 입찰을 시작합니다.
				</p>
			</div>

			<div
				class="grid min-h-0 gap-2 p-3 pr-[13px] xl:overflow-x-hidden xl:overflow-y-auto xl:overscroll-contain"
			>
				<div class="grid min-h-0 content-start gap-2">
					<div class="grid grid-cols-1 gap-2 md:grid-cols-2">
						{#each teams as team, index (team.teamIndex)}
							{@const captain = selectedCaptains[index] ?? null}
							{@const point = teamPoints[index]}
							<div
								class="grid h-56 grid-rows-[auto_minmax(0,1fr)] gap-4 border border-[var(--color-app-border-muted)] bg-slate-950/20 p-4"
							>
								<div class="grid gap-2">
									<p class="text-base font-semibold text-cyan-100">{team.teamName}</p>
									<label class="grid gap-1">
										<span class="text-[11px] font-semibold text-slate-500">시작 포인트</span>
										<input
											type="text"
											inputmode="numeric"
											pattern="[0-9]*"
											class={`h-9 w-full border bg-slate-950/35 px-3 text-right text-base font-semibold outline-none tabular-nums ${
												point === null
													? 'border-rose-300/45 text-rose-100 focus:border-rose-300/70'
													: 'border-amber-300/25 text-amber-100 focus:border-amber-300/50'
											}`}
											value={pointInputs[index] ?? ''}
											disabled={!canEdit || busy}
											oninput={(event) => updateTeamPoint(index, event.currentTarget.value)}
											aria-label={`${team.teamName} 시작 포인트`}
										/>
									</label>
								</div>
								{#if captain}
									<div class="flex min-h-0 items-center gap-3 overflow-hidden">
										<span
											class="grid size-12 shrink-0 place-items-center border border-[var(--color-app-border)] bg-[var(--color-app-control)] text-base font-semibold text-slate-200"
										>
											{participantInitial(captain.displayName)}
										</span>
										<div class="min-w-0">
											<p class="truncate text-xl font-semibold text-slate-100">
												{captain.displayName}
											</p>
											<p class="mt-1 truncate text-sm font-medium text-slate-500">
												신청 라인 · {declaredLaneLabel(captain.declaredLane)}
											</p>
										</div>
									</div>
								{:else}
									<div
										class="grid min-h-0 place-items-center overflow-hidden border border-dashed border-slate-700 text-sm font-semibold text-slate-600"
									>
										팀장 미지정
									</div>
								{/if}
							</div>
						{/each}
					</div>
				</div>
			</div>

			<div class="grid gap-2 border-t border-[var(--color-app-border-muted)] p-3 sm:grid-cols-2">
				{#if onSave}
					<button
						type="button"
						class="inline-flex h-11 items-center justify-center gap-2 border border-cyan-300/35 bg-cyan-400/10 px-4 text-sm font-semibold text-cyan-100 enabled:hover:bg-cyan-400/20 disabled:cursor-not-allowed disabled:opacity-40"
						disabled={!canSave}
						onclick={save}
					>
						{#if busy}
							<LoaderCircle size={16} class="animate-spin" aria-hidden="true" />
						{:else if !isDirty}
							<Check size={16} aria-hidden="true" />
						{:else}
							<Save size={16} aria-hidden="true" />
						{/if}
						{saveLabel}
					</button>
				{/if}
				{#if onCreate}
					<button
						type="button"
						class="inline-flex h-11 items-center justify-center gap-2 border border-emerald-300/35 bg-emerald-400/14 px-4 text-sm font-semibold text-emerald-100 enabled:hover:bg-emerald-400/20 disabled:cursor-not-allowed disabled:opacity-40"
						disabled={!canStart}
						onclick={startTournament}
					>
						{#if busy}
							<LoaderCircle size={16} class="animate-spin" aria-hidden="true" />
						{:else}
							<Play size={16} aria-hidden="true" />
						{/if}
						입찰 시작
					</button>
				{/if}
			</div>
		</section>
	</div>
</div>
