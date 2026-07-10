<script lang="ts">
	import { Dialog } from 'bits-ui';
	import { cubicOut } from 'svelte/easing';
	import { fade, fly } from 'svelte/transition';
	import { onDestroy } from 'svelte';
	import { LoaderCircle, Search, Swords, Trophy, X } from 'lucide-svelte';
	import { championIconUrl, profileIconUrl } from '$lib/datadragon';
	import {
		searchMockQuickRecord,
		type QuickRecordSearchResult
	} from '$lib/fixtures/quickRecordSearch';
	import QuickRecordChampionRow from './QuickRecordChampionRow.svelte';

	interface Props {
		open: boolean;
	}

	let { open = $bindable() }: Props = $props();

	let query = $state('');
	let searching = $state(false);
	let hasSearched = $state(false);
	let result = $state<QuickRecordSearchResult | null>(null);
	let searchTimer: ReturnType<typeof globalThis.setTimeout> | null = null;

	function clearSearchTimer() {
		if (!searchTimer) return;
		globalThis.clearTimeout(searchTimer);
		searchTimer = null;
	}

	function submitSearch(value = query) {
		const nextQuery = value.trim();

		if (!nextQuery || searching) {
			return;
		}

		query = nextQuery;
		searching = true;
		hasSearched = true;
		result = null;
		clearSearchTimer();

		searchTimer = globalThis.setTimeout(() => {
			result = searchMockQuickRecord(nextQuery);
			searching = false;
			searchTimer = null;
		}, 360);
	}

	function recentRecordSummary(games: QuickRecordSearchResult['custom']['recentGames']): string {
		const wins = games.filter((game) => game.result === '승').length;
		return `${wins}승 ${games.length - wins}패`;
	}

	onDestroy(clearSearchTimer);
</script>

<Dialog.Root bind:open>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 max-h-[min(46rem,calc(100vh-2rem))] w-[calc(100vw-1.5rem)] max-w-5xl -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none sm:w-[calc(100vw-3rem)]"
		>
			<div class="flex h-12 items-center justify-between px-4">
				<Dialog.Title class="text-sm font-semibold text-slate-50">전적검색</Dialog.Title>
				<Dialog.Close
					class="grid size-8 place-items-center rounded-sm text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100"
					aria-label="전적검색 닫기"
				>
					<X size={16} />
				</Dialog.Close>
			</div>

			<div
				class="grid max-h-[calc(min(46rem,100vh-2rem)-3rem)] grid-rows-[auto_minmax(0,1fr)] gap-3 p-3 sm:p-4"
			>
				<form
					class="grid gap-2"
					onsubmit={(event) => {
						event.preventDefault();
						submitSearch();
					}}
				>
					<div class="grid min-w-0 grid-cols-[1fr_auto] bg-[#08131d]">
						<input
							bind:value={query}
							aria-label="전적 검색어"
							placeholder="Riot ID, 디스코드 이름, 서버 표시명"
							class="min-w-0 bg-transparent px-3 py-3 text-sm font-medium text-slate-50 outline-none placeholder:text-slate-500"
						/>
						<button
							type="submit"
							class="grid size-11 place-items-center rounded-sm text-amber-100 hover:bg-amber-300/10 disabled:cursor-not-allowed disabled:opacity-50"
							aria-label="전적 검색"
							disabled={!query.trim() || searching}
						>
							{#if searching}
								<LoaderCircle size={18} class="animate-spin" />
							{:else}
								<Search size={18} />
							{/if}
						</button>
					</div>
				</form>

				{#if searching}
					<div
						class="grid min-h-0 content-start gap-2 bg-[var(--color-app-bg-soft)] p-3"
						transition:fade={{ duration: 120 }}
					>
						<div class="h-5 w-36 animate-pulse bg-slate-700/60"></div>
						<div class="grid gap-2 md:grid-cols-3">
							<div class="h-24 animate-pulse bg-slate-800/70"></div>
							<div class="h-24 animate-pulse bg-slate-800/70"></div>
							<div class="h-24 animate-pulse bg-slate-800/70"></div>
						</div>
					</div>
				{:else if result}
					<div
						class="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] gap-3"
						transition:fly={{ y: 8, duration: 160, easing: cubicOut }}
					>
						<section
							class="grid gap-3 bg-[var(--color-app-bg-soft)] p-3 md:grid-cols-[auto_1fr_auto]"
						>
							<img
								src={profileIconUrl(result.profileIconId, result.dataDragonVersion)}
								alt=""
								class="size-14 bg-slate-900 object-cover"
								loading="lazy"
							/>
							<div class="min-w-0">
								<p class="truncate text-lg font-semibold text-slate-50">{result.displayName}</p>
								<p class="mt-1 truncate text-sm font-semibold text-slate-400">{result.riotId}</p>
							</div>
							<div class="grid grid-cols-3 gap-px bg-[var(--color-app-border-muted)] md:w-72">
								<div class="bg-[var(--color-app-surface)] p-2">
									<p class="text-[11px] text-slate-500">내전</p>
									<p class="mt-1 text-sm font-semibold text-slate-100">
										{result.custom.games}게임
									</p>
								</div>
								<div class="bg-[var(--color-app-surface)] p-2">
									<p class="text-[11px] text-slate-500">승률</p>
									<p class="mt-1 text-sm font-semibold text-cyan-100">
										{result.custom.winRate}%
									</p>
								</div>
								<div class="bg-[var(--color-app-surface)] p-2">
									<p class="text-[11px] text-slate-500">주력 포지션</p>
									<p class="mt-1 text-sm font-semibold text-slate-100">
										{result.custom.mainLane}
									</p>
								</div>
							</div>
						</section>

						<div class="grid min-h-0 gap-3 lg:grid-cols-[minmax(0,1.35fr)_minmax(320px,0.65fr)]">
							<section
								class="grid min-h-0 grid-rows-[auto_auto_auto_minmax(0,1fr)] gap-3 bg-[var(--color-app-bg-soft)] p-3"
							>
								<div class="flex items-center justify-between gap-3">
									<div class="flex items-center gap-2">
										<Trophy size={16} class="text-cyan-100" />
										<p class="text-sm font-semibold text-slate-100">내전 전적</p>
									</div>
									<p class="text-xs font-semibold text-slate-500">
										{result.custom.wins}승 {result.custom.losses}패
									</p>
								</div>

								<div class="grid gap-px bg-[var(--color-app-border-muted)] sm:grid-cols-5">
									{#each result.custom.laneMmr as lane (lane.lane)}
										<div class="bg-[var(--color-app-surface)] p-2">
											<div class="flex items-center justify-between gap-2">
												<span class="text-xs font-semibold text-slate-400">{lane.lane}</span>
												<span class="text-[11px] font-semibold text-slate-500"
													>{lane.games}게임</span
												>
											</div>
											<p class="mt-1 text-sm font-semibold tabular-nums text-slate-100">
												{lane.mmr}
											</p>
										</div>
									{/each}
								</div>

								<div class="grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
									{#each result.custom.champions.slice(0, 6) as champion (champion.id)}
										<QuickRecordChampionRow
											{champion}
											dataDragonVersion={result.dataDragonVersion}
											iconSize="md"
										/>
									{/each}
								</div>

								<div class="grid min-h-0 grid-rows-[auto_minmax(0,1fr)] gap-2">
									<div class="grid gap-2">
										<div class="flex items-center justify-between gap-3">
											<p class="text-xs font-semibold text-slate-400">최근 게임</p>
											<p class="text-xs font-semibold text-slate-500">
												{recentRecordSummary(result.custom.recentGames)}
											</p>
										</div>
										<div
											class="flex gap-0.5"
											aria-label={`최근 ${result.custom.recentGames.length}게임 ${recentRecordSummary(result.custom.recentGames)}`}
										>
											{#each result.custom.recentGames as game (game.id)}
												<span
													class={`h-1.5 min-w-0 flex-1 ${
														game.result === '승' ? 'bg-cyan-300/80' : 'bg-rose-300/70'
													}`}
												></span>
											{/each}
										</div>
									</div>

									<div class="grid max-h-64 min-h-0 gap-1 overflow-y-auto pr-1">
										{#each result.custom.recentGames as game (game.id)}
											<div
												class="grid grid-cols-[auto_1fr_auto] items-center gap-2 bg-[var(--color-app-surface)] p-2"
											>
												<img
													src={championIconUrl(game.champion.imageFull, result.dataDragonVersion)}
													alt={game.champion.name}
													class="size-8 object-cover"
													loading="lazy"
												/>
												<div class="min-w-0">
													<p class="truncate text-xs font-semibold text-slate-100">{game.label}</p>
													<p class="truncate text-[11px] font-medium text-slate-500">
														{game.champion.name} · {game.lane} · {game.playedAt}
													</p>
												</div>
												<span
													class={`text-xs font-semibold ${
														game.result === '승' ? 'text-cyan-100' : 'text-rose-200'
													}`}
												>
													{game.result}
												</span>
											</div>
										{/each}
									</div>
								</div>
							</section>

							<section
								class="grid min-h-0 grid-rows-[auto_auto_minmax(0,1fr)] gap-3 bg-[var(--color-app-bg-soft)] p-3"
							>
								<div class="flex items-center justify-between gap-3">
									<div class="flex items-center gap-2">
										<Swords size={16} class="text-amber-100" />
										<p class="text-sm font-semibold text-slate-100">Riot 전적</p>
									</div>
									<p class="text-xs font-semibold text-slate-500">{result.riot.refreshedAt}</p>
								</div>

								<div class="grid grid-cols-3 gap-px bg-[var(--color-app-border-muted)]">
									<div class="bg-[var(--color-app-surface)] p-2">
										<p class="text-[11px] text-slate-500">{result.riot.rankType}</p>
										<p class="mt-1 text-sm font-semibold text-amber-100">{result.riot.tier}</p>
									</div>
									<div class="bg-[var(--color-app-surface)] p-2">
										<p class="text-[11px] text-slate-500">최근</p>
										<p class="mt-1 text-sm font-semibold text-slate-100">
											{result.riot.games}게임
										</p>
									</div>
									<div class="bg-[var(--color-app-surface)] p-2">
										<p class="text-[11px] text-slate-500">승률</p>
										<p class="mt-1 text-sm font-semibold text-amber-100">
											{result.riot.winRate}%
										</p>
									</div>
								</div>

								<div class="grid min-h-0 gap-3 overflow-y-auto pr-1">
									<div class="grid gap-2">
										<p class="text-xs font-semibold text-slate-400">최근 30게임 기준</p>
										<div class="grid gap-px bg-[var(--color-app-border-muted)]">
											{#each result.riot.recentChampions.slice(0, 6) as champion (champion.id)}
												<QuickRecordChampionRow
													{champion}
													dataDragonVersion={result.dataDragonVersion}
													tone="riot"
												/>
											{/each}
										</div>
									</div>

									<div class="grid gap-2">
										<p class="text-xs font-semibold text-slate-400">역대 많이 플레이한 챔피언</p>
										<div class="grid gap-px bg-[var(--color-app-border-muted)]">
											{#each result.riot.lifetimeChampions.slice(0, 6) as champion (champion.id)}
												<QuickRecordChampionRow
													{champion}
													dataDragonVersion={result.dataDragonVersion}
													tone="riot"
												/>
											{/each}
										</div>
									</div>
								</div>
							</section>
						</div>
					</div>
				{:else if hasSearched}
					<div
						class="grid min-h-44 place-items-center bg-[var(--color-app-bg-soft)] p-4 text-center"
					>
						<div>
							<p class="text-sm font-semibold text-slate-100">검색 결과가 없습니다</p>
							<p class="mt-2 text-xs font-medium text-slate-500">
								Riot ID 또는 서버 표시명을 다시 확인해 주세요.
							</p>
						</div>
					</div>
				{:else}
					<div
						class="grid min-h-44 place-items-center bg-[var(--color-app-bg-soft)] p-4 text-center"
					>
						<div>
							<p class="text-sm font-semibold text-slate-100">검색어를 입력하세요</p>
							<p class="mt-2 text-xs font-medium text-slate-500">
								한 번 검색하면 Riot 전적과 현재 Guild 내전 기록을 함께 보여줍니다.
							</p>
						</div>
					</div>
				{/if}
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
