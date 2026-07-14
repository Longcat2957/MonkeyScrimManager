<script lang="ts">
	import { X } from 'lucide-svelte';
	import {
		laneLabel,
		teamLabel,
		type EntrySlot,
		type Lane,
		type MockEntryParticipant,
		type Team
	} from '$lib/fixtures/workflow';
	import PlayerAvatar from './PlayerAvatar.svelte';
	import PositionIcon from './PositionIcon.svelte';
	import { entryParticipantDetail, entrySlot, participantForSlot } from './entryDraft';

	interface Props {
		team: Team;
		activeLanes: Lane[];
		participants: MockEntryParticipant[];
		assignments: Record<string, EntrySlot>;
		showDetails: boolean;
		dragUserId: string | null;
		overSlot: EntrySlot | null;
		profileIconVersion?: string | null;
		interactive?: boolean;
		moveUserToSlot: (userId: string, slot: EntrySlot | null) => void;
		handleDragStart: (event: globalThis.DragEvent, userId: string) => void;
		clearDragState: () => void;
		handleSlotDragOver: (event: globalThis.DragEvent, slot: EntrySlot) => void;
		handleSlotDrop: (event: globalThis.DragEvent, slot: EntrySlot) => void;
		teamAverage: (team: Team) => number;
	}

	let {
		team,
		activeLanes,
		participants,
		assignments,
		showDetails,
		dragUserId,
		overSlot,
		profileIconVersion = null,
		interactive = true,
		moveUserToSlot,
		handleDragStart,
		clearDragState,
		handleSlotDragOver,
		handleSlotDrop,
		teamAverage
	}: Props = $props();

	function occupant(slot: EntrySlot): MockEntryParticipant | null {
		return participantForSlot(participants, assignments, slot);
	}

	function participantClass(participant: MockEntryParticipant): string {
		const dragging = dragUserId === participant.id;
		return [
			'rounded-sm border-transparent bg-slate-800/65 text-slate-100',
			interactive ? 'cursor-grab active:cursor-grabbing hover:bg-slate-700/60' : 'cursor-default',
			dragging ? 'opacity-60' : ''
		].join(' ');
	}

	function slotClass(slot: EntrySlot, placed: MockEntryParticipant | null): string {
		if (overSlot === slot)
			return 'rounded-md border-cyan-300/70 bg-cyan-400/12 ring-1 ring-cyan-300/35';
		if (placed) return 'rounded-md border-slate-700 bg-slate-800/60';
		return 'rounded-sm border-dashed border-slate-700 bg-slate-950/25';
	}
</script>

<section class="min-w-0 bg-[var(--color-app-surface)]">
	<div
		class="flex h-11 items-center justify-between overflow-hidden px-2 py-2 sm:h-[3.125rem] sm:px-3"
	>
		<div>
			<p class="text-sm font-semibold text-slate-100">{teamLabel[team]}</p>
			{#if showDetails}
				<p class="text-[11px] text-slate-500">평균 {teamAverage(team)}</p>
			{/if}
		</div>
		<span
			class={`border px-2 py-1 text-[11px] font-semibold ${
				team === 'TEAM_1' ? 'border-blue-300/30 text-blue-100' : 'border-red-300/30 text-red-100'
			}`}
		>
			{team === 'TEAM_1' ? 'BLUE' : 'RED'}
		</span>
	</div>
	<div
		class="grid grid-rows-[repeat(5,4.25rem)] gap-1.5 px-1.5 py-1 sm:grid-rows-[repeat(5,4.75rem)] sm:px-2"
	>
		{#each activeLanes as lane (lane)}
			{@const slot = entrySlot(team, lane)}
			{@const placed = occupant(slot)}
			<div
				role="group"
				aria-label={`${teamLabel[team]} ${laneLabel[lane]} 배치 영역`}
				class={`grid h-[4.25rem] min-h-0 grid-cols-[2rem_minmax(0,1fr)_2.25rem] items-stretch gap-1 border p-1 text-left transition sm:h-[4.75rem] sm:grid-cols-[48px_minmax(0,1fr)_44px] sm:gap-2 sm:p-1.5 ${slotClass(slot, placed)}`}
				ondragover={(event) => handleSlotDragOver(event, slot)}
				ondragleave={() => {
					if (overSlot === slot) clearDragState();
				}}
				ondrop={(event) => handleSlotDrop(event, slot)}
			>
				<span class="grid place-items-center bg-slate-950/35 text-slate-300">
					<PositionIcon role={lane} size="md" />
				</span>
				{#if placed}
					<span
						class={`flex min-w-0 items-center gap-1 rounded-sm px-1.5 py-1 sm:gap-2 sm:px-2 ${participantClass(placed)}`}
						draggable={interactive}
						ondragstart={(event) => handleDragStart(event, placed.id)}
						ondragend={clearDragState}
						role="presentation"
					>
						<PlayerAvatar
							name={placed.name}
							role={placed.mainRole}
							avatarUrl={placed.avatarUrl}
							profileIconId={placed.profileIconId}
							{profileIconVersion}
						/>
						<span class="min-w-0 flex-1">
							<span class="block truncate text-sm font-semibold">{placed.name}</span>
							{#if showDetails}
								<span class="block truncate text-[11px] text-slate-500">
									{entryParticipantDetail(placed)}
								</span>
							{/if}
						</span>
					</span>
				{:else}
					<span class="flex items-center justify-center text-sm italic text-slate-600">
						비어 있음
					</span>
				{/if}
				<span class="grid place-items-center">
					{#if placed && interactive}
						<span
							role="button"
							tabindex="0"
							class="grid h-full min-h-14 w-full place-items-center rounded-sm text-slate-500 transition hover:bg-red-500/10 hover:text-red-100"
							onclick={(event) => {
								event.stopPropagation();
								moveUserToSlot(placed.id, null);
							}}
							onkeydown={(event) => {
								if (event.key !== 'Enter' && event.key !== ' ') return;
								event.preventDefault();
								event.stopPropagation();
								moveUserToSlot(placed.id, null);
							}}
							aria-label="배치 해제"
						>
							<X size={22} strokeWidth={2.4} />
						</span>
					{/if}
				</span>
			</div>
		{/each}
	</div>
</section>
