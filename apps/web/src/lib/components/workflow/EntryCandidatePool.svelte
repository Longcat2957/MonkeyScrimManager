<script lang="ts">
	import type { MockEntryParticipant } from '$lib/fixtures/workflow';
	import { entryParticipantDetail } from './entryDraft';
	import PlayerAvatar from './PlayerAvatar.svelte';

	interface Props {
		participants: MockEntryParticipant[];
		showDetails: boolean;
		dragUserId: string | null;
		overPool: boolean;
		profileIconVersion?: string | null;
		handleDragStart: (event: globalThis.DragEvent, userId: string) => void;
		clearDragState: () => void;
		handlePoolDragOver: (event: globalThis.DragEvent) => void;
		handlePoolDragLeave: () => void;
		handlePoolDrop: (event: globalThis.DragEvent) => void;
	}

	let {
		participants,
		showDetails,
		dragUserId,
		overPool,
		profileIconVersion = null,
		handleDragStart,
		clearDragState,
		handlePoolDragOver,
		handlePoolDragLeave,
		handlePoolDrop
	}: Props = $props();

	function poolParticipantClass(participant: MockEntryParticipant): string {
		const dragging = dragUserId === participant.id;
		return [
			'rounded-sm border-amber-300/10 bg-amber-300/[0.045] text-slate-100 hover:border-amber-200/20 hover:bg-amber-300/[0.075]',
			dragging ? 'opacity-60' : ''
		].join(' ');
	}
</script>

<section
	class={`border-b border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] ${
		overPool ? 'ring-1 ring-cyan-300/45' : ''
	}`}
	role="group"
	aria-label="후보 풀"
	ondragover={handlePoolDragOver}
	ondragleave={handlePoolDragLeave}
	ondrop={handlePoolDrop}
>
	<div class="flex items-center justify-between px-3 py-2">
		<p class="text-sm font-semibold text-slate-100">후보 풀</p>
		<p class="text-xs text-slate-500">탭 선택 / 드래그</p>
	</div>
	<div
		role="list"
		class="grid max-h-40 gap-1.5 overflow-auto p-2 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-5"
	>
		{#each participants as participant (participant.id)}
			<div
				role="listitem"
				draggable="true"
				ondragstart={(event) => handleDragStart(event, participant.id)}
				ondragend={clearDragState}
				class={`flex min-h-11 min-w-0 cursor-grab items-center gap-2 border px-2 py-1.5 text-left active:cursor-grabbing ${poolParticipantClass(participant)}`}
			>
				<PlayerAvatar
					name={participant.name}
					role={participant.mainRole}
					avatarUrl={participant.avatarUrl}
					profileIconId={participant.profileIconId}
					{profileIconVersion}
				/>
				<span class="min-w-0 flex-1">
					<span class="block truncate text-sm font-semibold">{participant.name}</span>
					{#if showDetails}
						<span class="block truncate text-[11px] text-slate-500">
							{entryParticipantDetail(participant)}
						</span>
					{/if}
				</span>
			</div>
		{:else}
			<p class="px-3 py-6 text-center text-sm text-slate-500">모든 참가자가 배치되었습니다.</p>
		{/each}
	</div>
</section>
