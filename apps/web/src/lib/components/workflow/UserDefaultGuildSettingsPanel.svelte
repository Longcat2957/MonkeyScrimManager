<script lang="ts">
	import { Check, ChevronDown, RotateCcw, Save, Server } from 'lucide-svelte';
	import { untrack } from 'svelte';
	import type { GuildAccessSummary } from '$lib/contracts/workflow';

	interface Props {
		initialGuildId: string | null;
		guilds: GuildAccessSummary[];
		onSave: (guildId: string) => void | Promise<void>;
	}

	let { initialGuildId, guilds, onSave }: Props = $props();
	const startingSavedGuildId = untrack(() => initialGuildId ?? '');
	const startingDraftGuildId = untrack(() => initialGuildId ?? guilds[0]?.id ?? '');
	let savedGuildId = $state(startingSavedGuildId);
	let draftGuildId = $state(startingDraftGuildId);
	let savedNotice = $state(false);
	let submitting = $state(false);
	let saveError = $state(false);

	const dirty = $derived(Boolean(draftGuildId) && draftGuildId !== savedGuildId);

	function reset() {
		draftGuildId = savedGuildId;
		savedNotice = false;
		saveError = false;
	}

	async function save() {
		if (!dirty || submitting) return;
		submitting = true;
		saveError = false;
		try {
			await onSave(draftGuildId);
			savedGuildId = draftGuildId;
			savedNotice = true;
		} catch {
			saveError = true;
		} finally {
			submitting = false;
		}
	}
</script>

<section
	class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
>
	<header
		class="flex min-h-14 flex-wrap items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
	>
		<div class="flex min-w-0 items-center gap-3">
			<Server size={18} class="shrink-0 text-cyan-200" aria-hidden="true" />
			<div class="min-w-0">
				<h2 class="truncate text-sm font-semibold text-slate-50">대표 길드</h2>
				<p class="text-xs text-slate-500">기본 진입 서버</p>
			</div>
		</div>
		<div class="flex items-center gap-2">
			{#if savedNotice}
				<span class="inline-flex items-center gap-1 text-xs font-semibold text-emerald-300">
					<Check size={14} aria-hidden="true" /> 저장됨
				</span>
			{/if}
			{#if saveError}
				<span class="text-xs font-semibold text-red-300">저장 실패</span>
			{/if}
			<button
				type="button"
				class="grid size-8 place-items-center border border-[var(--color-app-border)] bg-[var(--color-app-control)] text-slate-400 enabled:hover:text-slate-100 disabled:opacity-35"
				disabled={!dirty || submitting}
				aria-label="대표 길드 변경 취소"
				title="변경 취소"
				onclick={reset}
			>
				<RotateCcw size={15} aria-hidden="true" />
			</button>
			<button
				type="button"
				class="inline-flex h-8 items-center gap-1.5 border border-cyan-400/45 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 enabled:hover:bg-cyan-400/15 disabled:opacity-35"
				disabled={!dirty || submitting}
				aria-label="대표 길드 저장"
				onclick={save}
			>
				<Save size={14} aria-hidden="true" />
				{submitting ? '저장 중' : '저장'}
			</button>
		</div>
	</header>

	<div
		class="grid gap-4 px-4 py-5 sm:grid-cols-[minmax(0,1fr)_minmax(16rem,22rem)] sm:items-center"
	>
		<div class="min-w-0">
			<strong class="block text-sm text-slate-100">기본으로 열 길드</strong>
			<p class="mt-1 text-xs leading-5 text-slate-500">
				MonkeyBot 홈에 접속할 때 선택한 길드의 작업 화면으로 이동합니다.
			</p>
		</div>
		<label class="relative block min-w-0">
			<span class="sr-only">대표 길드 선택</span>
			<select
				bind:value={draftGuildId}
				onchange={() => (savedNotice = false)}
				disabled={guilds.length === 0 || submitting}
				class="h-11 w-full appearance-none border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 pr-10 text-sm font-semibold text-slate-100 outline-none hover:border-cyan-400/35 focus:border-cyan-400/50"
			>
				{#if guilds.length === 0}
					<option value="">선택 가능한 길드 없음</option>
				{:else if !savedGuildId}
					<option value="" disabled>대표 길드 미설정</option>
				{/if}
				{#each guilds as guild (guild.id)}
					<option value={guild.id}
						>{guild.displayName} · {guild.currentUserRole === 'OPERATOR'
							? '운영자'
							: '참가자'}</option
					>
				{/each}
			</select>
			<ChevronDown
				size={15}
				class="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 text-slate-500"
				aria-hidden="true"
			/>
		</label>
	</div>
</section>
