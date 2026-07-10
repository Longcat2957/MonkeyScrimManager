<script lang="ts">
	import { RotateCcw } from 'lucide-svelte';
	import { Dialog } from 'bits-ui';
	import { untrack } from 'svelte';
	import {
		LANES,
		TEAMS,
		type EntrySlot,
		type MockEntryHeadToHeadRecord,
		type MockEntryParticipant,
		type MockEntrySynergyRecord,
		type Team
	} from '$lib/fixtures/workflow';
	import EntryCandidatePool from './EntryCandidatePool.svelte';
	import EntryHeadToHeadRail from './EntryHeadToHeadRail.svelte';
	import EntrySynergyRail from './EntrySynergyRail.svelte';
	import EntryTeamColumn from './EntryTeamColumn.svelte';
	import { entrySlots, entryTeamAverage, moveEntryParticipant } from './entryDraft';

	interface Props {
		participants: MockEntryParticipant[];
		initialAssignments: Record<string, EntrySlot>;
		synergies?: MockEntrySynergyRecord[];
		headToHead?: MockEntryHeadToHeadRecord[];
		targetCount?: number;
		profileIconVersion?: string | null;
	}

	let {
		participants,
		initialAssignments,
		synergies = [],
		headToHead = [],
		targetCount = 10,
		profileIconVersion = null
	}: Props = $props();

	let assignments = $state<Record<string, EntrySlot>>(untrack(() => ({ ...initialAssignments })));
	let dragUserId = $state<string | null>(null);
	let overSlot = $state<EntrySlot | null>(null);
	let overPool = $state(false);
	let showDetails = $state(false);
	let confirmResetOpen = $state(false);

	const activeLanes = $derived(LANES.slice(0, Math.floor(targetCount / 2)));
	const activeSlots = $derived(entrySlots(activeLanes));
	const activeSlotSet = $derived(new Set(activeSlots));
	const filledCount = $derived(
		Object.values(assignments).filter((slot) => activeSlotSet.has(slot)).length
	);
	const unassigned = $derived(
		participants.filter((participant) => !activeSlotSet.has(assignments[participant.id]))
	);

	function moveUserToSlot(userId: string, slot: EntrySlot | null) {
		assignments = moveEntryParticipant(assignments, participants, userId, slot);
	}

	function resetDraft() {
		assignments = {};
		clearDragState();
		confirmResetOpen = false;
	}

	function handleDragStart(event: globalThis.DragEvent, userId: string) {
		if (!event.dataTransfer) return;
		dragUserId = userId;
		event.dataTransfer.setData('text/plain', userId);
		event.dataTransfer.effectAllowed = 'move';
	}

	function clearDragState() {
		dragUserId = null;
		overSlot = null;
		overPool = false;
	}

	function handleSlotDragOver(event: globalThis.DragEvent, slot: EntrySlot) {
		if (!event.dataTransfer) return;
		event.preventDefault();
		event.dataTransfer.dropEffect = 'move';
		overSlot = slot;
		overPool = false;
	}

	function handleSlotDrop(event: globalThis.DragEvent, slot: EntrySlot) {
		if (!event.dataTransfer) return;
		event.preventDefault();
		const userId = event.dataTransfer.getData('text/plain');
		if (userId) moveUserToSlot(userId, slot);
		clearDragState();
	}

	function handlePoolDragOver(event: globalThis.DragEvent) {
		if (!event.dataTransfer) return;
		event.preventDefault();
		event.dataTransfer.dropEffect = 'move';
		overPool = true;
		overSlot = null;
	}

	function handlePoolDrop(event: globalThis.DragEvent) {
		if (!event.dataTransfer) return;
		event.preventDefault();
		const userId = event.dataTransfer.getData('text/plain');
		if (userId) moveUserToSlot(userId, null);
		clearDragState();
	}

	function handlePoolDragLeave() {
		overPool = false;
	}

	function teamAverage(team: Team): number {
		return entryTeamAverage(team, activeLanes, participants, assignments);
	}
</script>

<div class="grid gap-2">
	<div
		class="flex flex-col gap-2 border-t border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] px-3 py-2 md:flex-row md:items-center md:justify-between"
	>
		<div class="min-w-0">
			<p class="text-xs font-semibold text-slate-500">엔트리 편성</p>
			<p class="mt-0.5 truncate text-sm font-semibold text-slate-100">
				{filledCount}/{activeSlots.length} 배치 · 후보 {unassigned.length}명
			</p>
		</div>
		<div class="flex flex-wrap items-center gap-1.5">
			<button
				type="button"
				class={`inline-flex h-8 items-center border px-2.5 text-xs font-semibold ${
					showDetails
						? 'rounded-md border-cyan-400/35 bg-cyan-400/10 text-cyan-100'
						: 'rounded-sm border-transparent text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-cyan-100'
				}`}
				onclick={() => {
					showDetails = !showDetails;
				}}
			>
				상세
			</button>
			<button
				type="button"
				class="inline-flex h-8 items-center gap-1.5 rounded-sm px-2.5 text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-cyan-100"
				onclick={() => {
					confirmResetOpen = true;
				}}
			>
				<RotateCcw size={14} />
				초기화
			</button>
		</div>
	</div>

	<div class="grid gap-2">
		{#if showDetails}
			<section
				class="overflow-x-auto border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-border-muted)]"
			>
				<div
					class="grid min-w-[56rem] grid-cols-[7rem_minmax(11.5rem,1fr)_5rem_minmax(11.5rem,1fr)_7rem] gap-px xl:min-w-0 xl:grid-cols-[minmax(8rem,0.28fr)_minmax(0,1fr)_minmax(5.5rem,0.18fr)_minmax(0,1fr)_minmax(8rem,0.28fr)]"
				>
					<EntrySynergyRail team="TEAM_1" {activeLanes} {assignments} {participants} {synergies} />
					<EntryTeamColumn
						team="TEAM_1"
						{activeLanes}
						{participants}
						{assignments}
						{showDetails}
						{dragUserId}
						{overSlot}
						{profileIconVersion}
						{moveUserToSlot}
						{handleDragStart}
						{clearDragState}
						{handleSlotDragOver}
						{handleSlotDrop}
						{teamAverage}
					/>
					<EntryHeadToHeadRail {activeLanes} {assignments} {participants} {headToHead} />
					<EntryTeamColumn
						team="TEAM_2"
						{activeLanes}
						{participants}
						{assignments}
						{showDetails}
						{dragUserId}
						{overSlot}
						{profileIconVersion}
						{moveUserToSlot}
						{handleDragStart}
						{clearDragState}
						{handleSlotDragOver}
						{handleSlotDrop}
						{teamAverage}
					/>
					<EntrySynergyRail team="TEAM_2" {activeLanes} {assignments} {participants} {synergies} />
				</div>
			</section>
		{:else}
			<section
				class="overflow-x-auto border-y border-[var(--color-app-border-muted)] bg-[var(--color-app-border-muted)]"
			>
				<div class="grid min-w-[30rem] grid-cols-2 gap-px xl:min-w-0">
					{#each TEAMS as team (team)}
						<EntryTeamColumn
							{team}
							{activeLanes}
							{participants}
							{assignments}
							{showDetails}
							{dragUserId}
							{overSlot}
							{profileIconVersion}
							{moveUserToSlot}
							{handleDragStart}
							{clearDragState}
							{handleSlotDragOver}
							{handleSlotDrop}
							{teamAverage}
						/>
					{/each}
				</div>
			</section>
		{/if}

		<EntryCandidatePool
			participants={unassigned}
			{showDetails}
			{dragUserId}
			{overPool}
			{profileIconVersion}
			{handleDragStart}
			{clearDragState}
			{handlePoolDragOver}
			{handlePoolDragLeave}
			{handlePoolDrop}
		/>
	</div>
</div>

<Dialog.Root bind:open={confirmResetOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-2rem)] max-w-sm -translate-x-1/2 -translate-y-1/2 border border-[var(--color-app-border)] bg-[var(--color-app-surface)] p-4 shadow-2xl shadow-black/50 outline-none"
		>
			<Dialog.Title class="text-sm font-semibold text-slate-50">배정 초기화</Dialog.Title>
			<Dialog.Description class="mt-2 text-xs leading-5 text-slate-400">
				현재 팀에 배치된 모든 참가자를 후보 풀로 되돌립니다.
			</Dialog.Description>
			<div class="mt-4 flex justify-end gap-2">
				<Dialog.Close
					class="inline-flex h-8 items-center rounded-sm px-3 text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-slate-100"
				>
					취소
				</Dialog.Close>
				<button
					type="button"
					class="inline-flex h-8 items-center rounded-sm border border-red-300/30 bg-red-500/10 px-3 text-xs font-semibold text-red-100 hover:bg-red-500/15"
					onclick={resetDraft}
				>
					되돌리기
				</button>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
