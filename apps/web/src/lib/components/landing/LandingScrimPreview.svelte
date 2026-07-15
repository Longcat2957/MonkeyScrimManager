<script lang="ts">
	import { Gavel, GripVertical, Swords } from 'lucide-svelte';
	import EntryDraftBoard from '$lib/components/workflow/EntryDraftBoard.svelte';
	import ScrimPickBanResultInputBoard from '$lib/components/workflow/ScrimPickBanResultInputBoard.svelte';
	import {
		mockCompletedEntryAssignments,
		mockEntryHeadToHead,
		mockEntryParticipants,
		mockEntrySynergies,
		type EntrySlot
	} from '$lib/fixtures/workflow';
	import { moveEntryParticipant } from '$lib/components/workflow/entryDraft';

	type DemoView = 'entry' | 'pickban';

	let activeView = $state<DemoView>('entry');
	let assignments = $state<Record<string, EntrySlot>>({ ...mockCompletedEntryAssignments });

	function moveParticipant(userId: string, slot: EntrySlot | null) {
		assignments = moveEntryParticipant(assignments, mockEntryParticipants, userId, slot);
	}

	function resetAssignments() {
		assignments = {};
	}

	async function recordDemoResult() {
		// Landing demo only: the input board keeps its own local success state.
	}
</script>

<section
	class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-[0_22px_50px_rgb(0_0_0_/_0.28)]"
	aria-label="MonkeyBot 내전 운영 컴포넌트 데모"
>
	<header
		class="flex flex-col gap-3 border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-3 sm:flex-row sm:items-center sm:justify-between sm:px-5"
	>
		<div class="flex min-w-0 items-center gap-3">
			<span
				class="grid size-9 shrink-0 place-items-center border border-cyan-300/25 bg-cyan-300/10 text-cyan-200"
			>
				<GripVertical size={18} aria-hidden="true" />
			</span>
			<div class="min-w-0">
				<p class="text-[11px] font-semibold text-cyan-200">INTERACTIVE DEMO</p>
				<h3 class="truncate text-sm font-semibold text-slate-50">직접 조작해 보는 내전 작업대</h3>
			</div>
		</div>

		<div
			class="flex border border-[var(--color-app-border-muted)] bg-[#0b1018] p-1"
			role="tablist"
			aria-label="작업대 데모"
		>
			<button
				type="button"
				role="tab"
				aria-selected={activeView === 'entry'}
				class={`inline-flex h-8 items-center gap-1.5 px-3 text-xs font-semibold outline-none ${
					activeView === 'entry'
						? 'bg-cyan-300/15 text-cyan-100'
						: 'text-slate-500 hover:text-slate-200'
				}`}
				onclick={() => {
					activeView = 'entry';
				}}
			>
				<Swords size={14} aria-hidden="true" />
				엔트리 편성
			</button>
			<button
				type="button"
				role="tab"
				aria-selected={activeView === 'pickban'}
				class={`inline-flex h-8 items-center gap-1.5 px-3 text-xs font-semibold outline-none ${
					activeView === 'pickban'
						? 'bg-cyan-300/15 text-cyan-100'
						: 'text-slate-500 hover:text-slate-200'
				}`}
				onclick={() => {
					activeView = 'pickban';
				}}
			>
				<Gavel size={14} aria-hidden="true" />
				픽밴 입력
			</button>
		</div>
	</header>

	<div class="bg-[var(--color-app-bg)] p-2 sm:p-3">
		{#if activeView === 'entry'}
			<EntryDraftBoard
				participants={mockEntryParticipants}
				{assignments}
				synergies={mockEntrySynergies}
				headToHead={mockEntryHeadToHead}
				onMoveParticipant={moveParticipant}
				onReset={resetAssignments}
			/>
		{:else}
			<ScrimPickBanResultInputBoard allowLocalBulkInput onSubmit={recordDemoResult} />
		{/if}
	</div>
</section>
