<script lang="ts">
	import { Check, Search } from 'lucide-svelte';
	import { untrack } from 'svelte';
	import { laneLabel, type MockAuctionMember } from '$lib/fixtures/workflow';
	import PlayerAvatar from './PlayerAvatar.svelte';
	import PositionIcon from './PositionIcon.svelte';

	interface Props {
		initialMembers: MockAuctionMember[];
		initialCaptainIds?: string[];
		teamCount?: number;
		initialPoints?: number;
		profileIconVersion?: string | null;
	}

	let {
		initialMembers,
		initialCaptainIds = [],
		teamCount = 4,
		initialPoints = 1000,
		profileIconVersion = null
	}: Props = $props();

	let captainIds = $state<string[]>(untrack(() => initialCaptainIds.slice(0, teamCount)));
	let pointInputs = $state<string[]>(
		untrack(() => Array.from({ length: teamCount }, () => String(initialPoints)))
	);
	let query = $state('');

	const normalizedQuery = $derived(query.trim().toLowerCase());
	const teamIndexes = $derived(Array.from({ length: teamCount }, (_, index) => index));
	const teamPoints = $derived(
		teamIndexes.map((index) => pointInputNumber(pointInputs[index] ?? ''))
	);
	const validPointCount = $derived(teamPoints.filter((point) => point !== null).length);
	const selectedCaptains = $derived(
		captainIds
			.map((id) => initialMembers.find((member) => member.id === id))
			.filter((member): member is MockAuctionMember => Boolean(member))
	);
	const filteredMembers = $derived(
		initialMembers.filter((member) => {
			if (!normalizedQuery) return true;
			return (
				member.name.toLowerCase().includes(normalizedQuery) ||
				laneLabel[member.role].toLowerCase().includes(normalizedQuery) ||
				(member.riotTier ?? '').toLowerCase().includes(normalizedQuery)
			);
		})
	);
	const ready = $derived(selectedCaptains.length === teamCount && validPointCount === teamCount);

	function pointInputNumber(value: string): number | null {
		const parsed = Number(value.trim());
		if (!Number.isFinite(parsed) || parsed <= 0) return null;
		return Math.floor(parsed);
	}

	function toggleCaptain(memberId: string) {
		if (captainIds.includes(memberId)) {
			captainIds = captainIds.filter((id) => id !== memberId);
			return;
		}
		if (captainIds.length >= teamCount) return;
		captainIds = [...captainIds, memberId];
	}

	function updateTeamPoint(index: number, value: string) {
		pointInputs = teamIndexes.map((teamIndex) =>
			teamIndex === index ? value : (pointInputs[teamIndex] ?? '')
		);
	}

	function captainOrder(memberId: string): number | null {
		const index = captainIds.indexOf(memberId);
		return index === -1 ? null : index + 1;
	}

	function memberMeta(member: MockAuctionMember): string {
		return `${member.riotTier ?? '-'} · MMR ${member.mmr} · ${laneLabel[member.role]}`;
	}
</script>

<div class="grid gap-2 pb-2">
	<div class="grid gap-2 xl:grid-cols-[minmax(24rem,34rem)_minmax(0,1fr)]">
		<section
			class="grid h-[min(42rem,calc(100vh-14rem))] min-h-[32rem] grid-rows-[auto_minmax(0,1fr)] border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]"
		>
			<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
				<label
					class="grid h-10 grid-cols-[auto_minmax(0,1fr)] items-center gap-2 border border-[var(--color-app-border-muted)] bg-slate-950/35 px-3"
				>
					<Search size={15} class="text-slate-500" />
					<input
						class="min-w-0 bg-transparent text-sm font-semibold text-slate-100 outline-none placeholder:text-slate-600"
						placeholder="참가자 검색"
						bind:value={query}
					/>
				</label>
			</div>

			<div
				role="list"
				class="grid min-h-0 content-start gap-1.5 overflow-x-hidden overflow-y-auto overscroll-contain p-2 pr-[9px]"
			>
				{#each filteredMembers as member (member.id)}
					{@const order = captainOrder(member.id)}
					<div role="listitem" class="min-w-0">
						<button
							type="button"
							class={`grid min-h-14 w-full grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-2 border px-2 py-2 text-left transition ${
								order
									? 'border-amber-300/35 bg-amber-300/10'
									: 'border-transparent bg-slate-950/20 hover:border-cyan-300/25 hover:bg-cyan-400/5'
							}`}
							onclick={() => toggleCaptain(member.id)}
							disabled={!order && captainIds.length >= teamCount}
							aria-pressed={Boolean(order)}
						>
							<PlayerAvatar
								name={member.name}
								role={member.role}
								avatarUrl={member.avatarUrl}
								profileIconId={member.profileIconId}
								{profileIconVersion}
								size="sm"
							/>
							<span class="min-w-0">
								<span class="block truncate text-base font-semibold text-slate-100">
									{member.name}
								</span>
								<span class="block truncate text-xs font-medium text-slate-500">
									{memberMeta(member)}
								</span>
							</span>
							<span class="grid min-w-16 justify-items-end gap-1">
								<PositionIcon role={member.role} size="sm" />
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
			class="grid h-[min(42rem,calc(100vh-14rem))] min-h-[32rem] grid-rows-[auto_minmax(0,1fr)_auto] border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]"
		>
			<div class="border-b border-[var(--color-app-border-muted)] px-3 py-3">
				<div>
					<p class="text-base font-semibold text-slate-100">토너먼트 시작 설정</p>
					<p class="mt-1 text-xs text-slate-500">
						팀장과 팀별 시작 포인트가 확정되면 입찰 후보 풀이 생성됩니다.
					</p>
				</div>
			</div>

			<div
				class="grid min-h-0 gap-2 overflow-x-hidden overflow-y-auto overscroll-contain p-3 pr-[13px]"
			>
				<div class="grid min-h-0 content-start gap-2">
					<div class="grid grid-cols-1 gap-2 md:grid-cols-2">
						{#each teamIndexes as index (index)}
							{@const captain = selectedCaptains[index] ?? null}
							{@const point = teamPoints[index]}
							<div
								class="grid h-56 grid-rows-[auto_minmax(0,1fr)] gap-4 border border-[var(--color-app-border-muted)] bg-slate-950/20 p-4"
							>
								<div class="grid gap-2">
									<p class="text-base font-semibold text-cyan-100">{index + 1}팀</p>
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
											oninput={(event) => updateTeamPoint(index, event.currentTarget.value)}
											aria-label={`${index + 1}팀 시작 포인트`}
										/>
									</label>
								</div>
								{#if captain}
									<div class="flex min-h-0 items-center gap-3 overflow-hidden">
										<PlayerAvatar
											name={captain.name}
											role={captain.role}
											avatarUrl={captain.avatarUrl}
											profileIconId={captain.profileIconId}
											{profileIconVersion}
											size="md"
										/>
										<div class="min-w-0">
											<p class="truncate text-xl font-semibold text-slate-100">{captain.name}</p>
											<p class="mt-1 truncate text-sm font-medium text-slate-500">
												{memberMeta(captain)}
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

			<div class="border-t border-[var(--color-app-border-muted)] p-3">
				<button
					type="button"
					class="inline-flex h-12 w-full items-center justify-center gap-2 border border-emerald-300/35 bg-emerald-400/14 px-4 text-base font-semibold text-emerald-100 hover:bg-emerald-400/20 disabled:cursor-not-allowed disabled:opacity-40"
					disabled={!ready}
				>
					<Check size={16} />
					입찰 준비 완료
				</button>
			</div>
		</section>
	</div>
</div>
