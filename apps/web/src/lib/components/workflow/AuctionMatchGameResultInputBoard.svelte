<script lang="ts">
	import { Dialog } from 'bits-ui';
	import { LoaderCircle, Undo2 } from 'lucide-svelte';
	import type { ChampionCatalog } from '$lib/datadragon';
	import {
		auctionMatchResultTeams,
		type AuctionMatchConfirmedGame,
		type AuctionMatchGameResultRevertCommand,
		type AuctionMatchGameResultSnapshot,
		type AuctionMatchGameResultSubmitCommand,
		type AuctionMatchResultTeam
	} from '$lib/contracts/auctionMatchGameResult';
	import type { ChampionPreferenceLane } from '$lib/contracts/workflow';
	import type {
		ScrimPickBanResultInputGame,
		ScrimPickBanResultInputSide,
		ScrimPickBanResultInputTeam
	} from '$lib/fixtures/scrimPickBanResultInputFixtures';
	import ScrimPickBanResultInputBoard from './ScrimPickBanResultInputBoard.svelte';
	import ScrimPickBanResultSetSummaryCard from './ScrimPickBanResultSetSummaryCard.svelte';

	type Side = ScrimPickBanResultInputSide;

	interface Props {
		snapshot: AuctionMatchGameResultSnapshot;
		catalog: ChampionCatalog;
		busy?: boolean;
		onSubmit?: (command: AuctionMatchGameResultSubmitCommand) => void | Promise<void>;
		onDeleteLastGame?: (command: AuctionMatchGameResultRevertCommand) => void | Promise<void>;
	}

	let { snapshot, catalog, busy = false, onSubmit, onDeleteLastGame }: Props = $props();

	const lanes: ChampionPreferenceLane[] = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'];

	let reverting = $state(false);
	let revertDialogOpen = $state(false);
	let operationError = $state('');
	let selectedCompletedGameId = $state<string | null>(null);

	const resultTeams = $derived(
		auctionMatchResultTeams(snapshot).map((team) => ({
			...team,
			members: [...team.members].sort(
				(left, right) => lanes.indexOf(left.lane) - lanes.indexOf(right.lane)
			)
		}))
	);
	const nextSetNumber = $derived(Math.max(0, ...snapshot.games.map((game) => game.setNumber)) + 1);
	const requiredWins = $derived(Math.floor(snapshot.match.bestOf / 2) + 1);
	const matchDecided = $derived(
		snapshot.match.leftScore >= requiredWins || snapshot.match.rightScore >= requiredWins
	);
	const editingLocked = $derived(busy || reverting);
	const canEnterResults = $derived(
		Boolean(onSubmit) &&
			snapshot.permissions.canEnterResults &&
			snapshot.match.lineupsLocked &&
			snapshot.match.status !== 'COMPLETED' &&
			snapshot.match.status !== 'CANCELLED' &&
			nextSetNumber <= snapshot.match.bestOf &&
			!matchDecided &&
			!editingLocked
	);
	const canDeleteLastGame = $derived(
		Boolean(onDeleteLastGame) &&
			snapshot.games.length > 0 &&
			(snapshot.permissions.canRevertLastGame ?? snapshot.permissions.canEnterResults) &&
			!editingLocked
	);
	const boardTeams = $derived.by(() => ({
		BLUE: toBoardTeam('BLUE', resultTeams[0]),
		RED: toBoardTeam('RED', resultTeams[1])
	}));
	const currentGame = $derived.by(
		(): ScrimPickBanResultInputGame => ({
			id: `${snapshot.match.id}:set:${nextSetNumber}`,
			label: `${nextSetNumber}세트`,
			sidePreset: 'DEFAULT',
			winner: null,
			bans: emptyBans(),
			picks: Object.fromEntries(snapshot.lineups.map((lineup) => [lineup.userId, null]))
		})
	);
	const previousPickChampionIds = $derived(
		snapshot.games.flatMap((game) =>
			Object.values(game.picks).filter((championId): championId is string => championId !== null)
		)
	);
	const confirmedGames = $derived(
		[...snapshot.games].sort((left, right) => left.setNumber - right.setNumber)
	);
	const completedSetNumbers = $derived(
		Array.from({ length: snapshot.match.bestOf }, (_, index) => index + 1)
	);
	const confirmedGamesBySetNumber = $derived(
		new Map(confirmedGames.map((game) => [game.setNumber, game]))
	);
	const selectedCompletedGame = $derived(
		confirmedGames.find((game) => game.id === selectedCompletedGameId) ??
			confirmedGames.at(-1) ??
			null
	);

	function emptyBans(): Record<Side, Array<string | null>> {
		return { BLUE: [null, null, null, null, null], RED: [null, null, null, null, null] };
	}

	function toBoardTeam(side: Side, team: AuctionMatchResultTeam | undefined): ScrimPickBanResultInputTeam {
		return {
			side,
			name: team?.teamName ?? `${side} 팀`,
			players: (team?.members ?? []).map((member) => ({
				id: member.userId,
				name: member.displayName,
				lane: member.lane,
				mmr: member.mmrAtAcquisition ?? 0,
				riotTier: null,
				preferredChampions: [],
				frequentChampions: []
			}))
		};
	}

	function teamsForConfirmedGame(game: AuctionMatchConfirmedGame): Record<Side, ScrimPickBanResultInputTeam> {
		const leftSide: Side = game.sidePreset === 'DEFAULT' ? 'BLUE' : 'RED';
		return {
			BLUE: toBoardTeam('BLUE', leftSide === 'BLUE' ? resultTeams[0] : resultTeams[1]),
			RED: toBoardTeam('RED', leftSide === 'RED' ? resultTeams[0] : resultTeams[1])
		};
	}

	async function submitCurrentGame(game: ScrimPickBanResultInputGame): Promise<void> {
		if (!canEnterResults || !onSubmit || game.winner === null) return;
		await onSubmit({
			expectedRevision: snapshot.match.revision,
			setNumber: nextSetNumber,
			sidePreset: game.sidePreset ?? 'DEFAULT',
			winner: game.winner,
			picks: { ...game.picks },
			bans: { BLUE: [...game.bans.BLUE], RED: [...game.bans.RED] },
			patchVersion: catalog.version
		});
	}

	async function deleteLastGame(): Promise<void> {
		if (!canDeleteLastGame || !onDeleteLastGame) return;
		reverting = true;
		operationError = '';
		try {
			await onDeleteLastGame({ expectedRevision: snapshot.match.revision });
			revertDialogOpen = false;
		} catch (cause) {
			operationError = callbackErrorText(cause, '마지막 경매 게임 결과를 되돌리지 못했습니다.');
		} finally {
			reverting = false;
		}
	}

	function callbackErrorText(cause: unknown, fallback: string): string {
		if (!(cause instanceof Error)) return fallback;
		const details = cause as Error & { requestId?: unknown };
		const requestId = typeof details.requestId === 'string' ? details.requestId : null;
		return requestId && !cause.message.includes(requestId)
			? `${cause.message} (요청 ID: ${requestId})`
			: cause.message;
	}

	function winnerLabel(game: AuctionMatchGameResultSnapshot['games'][number]): string {
		const leftWon =
			(game.sidePreset === 'DEFAULT' && game.winner === 'BLUE') ||
			(game.sidePreset === 'SWAPPED' && game.winner === 'RED');
		const team = leftWon ? resultTeams[0] : resultTeams[1];
		return `${team?.teamName ?? game.winner} 승리`;
	}
</script>

<div class="grid gap-2">
	{#if snapshot.games.length > 0}
		<div
			class="flex min-h-10 flex-wrap items-center justify-between gap-2 border border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] px-3 py-1.5"
		>
			<p class="text-xs font-semibold text-slate-300">
				{snapshot.match.status === 'COMPLETED' ? '최종 점수' : '현재 점수'}
				<span class="text-slate-100">{snapshot.match.leftScore}:{snapshot.match.rightScore}</span>
			</p>
			<button
				type="button"
				disabled={!canDeleteLastGame}
				onclick={() => (revertDialogOpen = true)}
				class="inline-flex h-8 shrink-0 items-center justify-center gap-1.5 border border-amber-300/25 px-2.5 text-[11px] font-semibold text-amber-100 enabled:hover:bg-amber-400/10 disabled:cursor-not-allowed disabled:opacity-35"
			>
				{#if reverting}
					<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 되돌리는 중
				{:else}
					<Undo2 size={14} aria-hidden="true" /> 마지막 결과 되돌리기
				{/if}
			</button>
		</div>
	{/if}

	{#if !snapshot.match.lineupsLocked}
		<p
			class="border border-amber-300/20 bg-amber-400/[0.06] px-3 py-2 text-xs leading-5 font-medium text-amber-100"
		>
			양 팀의 라인업을 확정한 뒤에만 게임 결과를 기록할 수 있습니다.
		</p>
	{:else if resultTeams.length !== 2}
		<p
			class="border border-rose-300/20 bg-rose-400/[0.06] px-3 py-2 text-xs leading-5 font-medium text-rose-100"
		>
			경기 라인업을 확인할 수 없습니다. 최신 경기 정보를 다시 불러와 주세요.
		</p>
	{:else if nextSetNumber > snapshot.match.bestOf || matchDecided}
		<p
			class="border border-emerald-300/20 bg-emerald-400/[0.06] px-3 py-2 text-xs leading-5 font-medium text-emerald-100"
		>
			이 경기의 승패가 이미 확정되었습니다.
		</p>
	{:else}
		{#key `${snapshot.match.id}:${snapshot.match.revision}:${nextSetNumber}`}
			<ScrimPickBanResultInputBoard
				teams={boardTeams}
				hardFearless={snapshot.match.hardFearless}
				{previousPickChampionIds}
				validateDraft
				allowLocalBulkInput
				readOnly={!canEnterResults}
				busy={editingLocked}
				initialGame={currentGame}
				champions={catalog.champions}
				patchVersion={catalog.version}
				onSubmit={canEnterResults ? submitCurrentGame : undefined}
			/>
		{/key}
	{/if}

	{#if operationError}
		<p
			class="border border-rose-300/25 bg-rose-400/[0.06] px-3 py-2 text-xs leading-5 text-rose-100"
			role="alert"
		>
			{operationError}
		</p>
	{/if}

	{#if snapshot.match.status === 'COMPLETED' && selectedCompletedGame && resultTeams.length === 2}
		{@const gameTeams = teamsForConfirmedGame(selectedCompletedGame)}
		<section class="overflow-hidden border border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]">
			<header
				class="flex min-h-11 items-center justify-between gap-3 border-b border-[var(--color-app-border-muted)] px-3"
			>
				<h2 class="text-xs font-semibold text-slate-200">확정 세트 결과</h2>
				<span class="text-[11px] font-medium text-slate-500">{confirmedGames.length}세트 확정</span>
			</header>
			<div
				class="grid gap-1 border-b border-[var(--color-app-border-muted)] bg-[var(--color-app-bg-soft)] p-2"
				style={`grid-template-columns: repeat(${snapshot.match.bestOf}, minmax(0, 1fr));`}
			>
				{#each completedSetNumbers as setNumber (setNumber)}
					{@const game = confirmedGamesBySetNumber.get(setNumber)}
					<button
						type="button"
						disabled={!game}
						aria-label={game ? `${setNumber}세트 결과 보기` : `${setNumber}세트 미진행`}
						title={game ? undefined : '진행되지 않은 세트'}
						class={`grid min-h-14 place-items-center border px-2 text-center ${
							!game
								? 'cursor-not-allowed border-[var(--color-app-border-muted)] bg-slate-950/20 text-slate-700'
								: selectedCompletedGame.id === game.id
									? 'border-cyan-400/50 bg-cyan-400/8 text-slate-100'
									: 'border-[var(--color-app-border)] bg-[var(--color-app-control)] text-slate-300 hover:bg-[var(--color-app-hover)]'
						}`}
						onclick={() => {
							if (game) selectedCompletedGameId = game.id;
						}}
					>
						<strong class="text-base font-semibold">{setNumber}세트</strong>
					</button>
				{/each}
			</div>
			<ScrimPickBanResultSetSummaryCard
				label={`${selectedCompletedGame.setNumber}세트`}
				blueTeam={gameTeams.BLUE}
				redTeam={gameTeams.RED}
				winner={selectedCompletedGame.winner}
				bans={selectedCompletedGame.bans}
				picks={selectedCompletedGame.picks}
				champions={catalog.champions}
				version={selectedCompletedGame.patchVersion ?? catalog.version}
			/>
		</section>
	{:else if confirmedGames.length > 0 && resultTeams.length === 2}
		<details class="border border-[var(--color-app-border-muted)] bg-slate-950/15">
			<summary class="cursor-pointer px-3 py-2 text-xs font-semibold text-slate-300">
				입력된 세트 결과 {confirmedGames.length}개
			</summary>
			<div
				class="grid gap-px border-t border-[var(--color-app-border-muted)] bg-[var(--color-app-border-muted)]"
			>
				{#each confirmedGames as game (game.id)}
					<div
						class="flex items-center justify-between gap-3 bg-[var(--color-app-surface)] px-3 py-2"
					>
						<span class="text-xs font-semibold text-slate-300">{game.setNumber}세트</span>
						<span class="truncate text-xs font-medium text-slate-500">
							{game.sidePreset === 'DEFAULT' ? '왼쪽 팀 블루' : '오른쪽 팀 블루'} · {winnerLabel(
								game
							)}
						</span>
					</div>
				{/each}
			</div>
		</details>
	{/if}
</div>

<Dialog.Root bind:open={revertDialogOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-2rem)] max-w-sm -translate-x-1/2 -translate-y-1/2 border border-[var(--color-app-border)] bg-[var(--color-app-surface)] p-4 shadow-2xl shadow-black/50 outline-none"
		>
			<Dialog.Title class="text-sm font-semibold text-slate-50"
				>마지막 게임 결과 되돌리기</Dialog.Title
			>
			<Dialog.Description class="mt-2 text-xs leading-5 text-slate-400">
				가장 최근에 확정한 세트와 경기 점수, 이미 반영된 길드 MMR을 함께 되돌립니다. 완료된 결승의
				Discord 결과 메시지는 서버가 전송 상태를 확인해 정리하며, 확인할 수 없으면 이 작업은
				진행되지 않습니다.
			</Dialog.Description>
			<div class="mt-4 flex justify-end gap-2">
				<Dialog.Close
					disabled={editingLocked}
					class="inline-flex h-8 items-center px-3 text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:cursor-not-allowed disabled:opacity-35"
				>
					취소
				</Dialog.Close>
				<button
					type="button"
					disabled={!canDeleteLastGame}
					onclick={() => void deleteLastGame()}
					class="inline-flex h-8 items-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 enabled:hover:bg-amber-400/16 disabled:cursor-not-allowed disabled:opacity-35"
				>
					{#if reverting}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 되돌리는 중
					{:else}
						<Undo2 size={14} aria-hidden="true" /> 결과 되돌리기
					{/if}
				</button>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
