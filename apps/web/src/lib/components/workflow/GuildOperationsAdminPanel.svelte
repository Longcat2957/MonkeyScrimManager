<script lang="ts">
	import { Dialog } from 'bits-ui';
	import { Check, History, RotateCcw, ShieldAlert, X } from 'lucide-svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import { type GuildRevertCheckpoint } from '$lib/fixtures/guildAdmin';
	import type { GuildRevertCommand } from '$lib/realtime/admin';

	interface Props {
		checkpoints: GuildRevertCheckpoint[];
		revision: number;
		onRevert: (command: GuildRevertCommand) => void | Promise<void>;
	}

	let { checkpoints, revision, onRevert }: Props = $props();
	let pendingAction = $state<GuildRevertCheckpoint | null>(null);
	let reason = $state('');
	let confirmOpen = $state(false);
	let notice = $state('');
	let submitting = $state(false);

	const canConfirm = $derived(reason.trim().length >= 3);

	function requestRevert(item: GuildRevertCheckpoint) {
		pendingAction = item;
		reason = '';
		confirmOpen = true;
	}

	async function confirmAction() {
		if (!pendingAction || !canConfirm || submitting) return;
		submitting = true;
		try {
			await onRevert({
				kind: 'APPLY_REVERT',
				checkpointId: pendingAction.id,
				reason: reason.trim(),
				expectedRevision: revision
			});
			notice = `${pendingAction.target} 복구 완료`;
			confirmOpen = false;
			pendingAction = null;
		} finally {
			submitting = false;
		}
	}
</script>

<div class="grid gap-4">
	<section
		class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<header
			class="flex min-h-14 flex-wrap items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
		>
			<div class="flex min-w-0 items-center gap-3">
				<History size={18} class="shrink-0 text-cyan-200" aria-hidden="true" />
				<div class="min-w-0">
					<h2 class="truncate text-sm font-semibold text-slate-50">최근 복구 지점</h2>
					<p class="text-xs text-slate-500">지원되는 고정 단계만 복구</p>
				</div>
			</div>
			{#if notice}
				<span class="inline-flex items-center gap-1 text-xs font-semibold text-emerald-300">
					<Check size={14} aria-hidden="true" />
					{notice}
				</span>
			{/if}
		</header>

		<div class="overflow-x-auto">
			<div class="min-w-[760px]">
				<div
					class="grid grid-cols-[170px_minmax(220px,1fr)_100px_100px_110px] border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600"
				>
					<span>복구 작업</span><span>대상</span><span>실행자</span><span>생성</span><span></span>
				</div>
				<div class="divide-y divide-[var(--color-app-border-muted)]">
					{#each checkpoints as checkpoint (checkpoint.id)}
						<div
							class="grid grid-cols-[170px_minmax(220px,1fr)_100px_100px_110px] items-center px-4 py-3"
						>
							<strong class="truncate text-xs text-slate-200">{checkpoint.action}</strong>
							<span class="truncate text-xs text-slate-400">{checkpoint.target}</span>
							<span class="text-xs text-slate-500">{checkpoint.operator}</span>
							<span class="text-xs text-slate-600">{checkpoint.createdAt}</span>
							{#if checkpoint.status === 'ACTIVE'}
								<button
									type="button"
									onclick={() => requestRevert(checkpoint)}
									class="inline-flex h-8 items-center justify-center gap-1.5 border border-amber-300/30 px-2 text-xs font-semibold text-amber-100 hover:bg-amber-300/8"
								>
									<RotateCcw size={13} aria-hidden="true" /> 복구
								</button>
							{:else}
								<StatusPill label="복구됨" tone="neutral" />
							{/if}
						</div>
					{/each}
				</div>
			</div>
		</div>
	</section>
</div>

<Dialog.Root bind:open={confirmOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-lg -translate-x-1/2 -translate-y-1/2 border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="flex items-center gap-2 text-sm font-semibold text-slate-50">
					<ShieldAlert size={16} class="text-amber-200" />
					복구 작업 확인
				</Dialog.Title>
				<Dialog.Close
					class="grid size-8 place-items-center text-slate-400 hover:text-slate-100"
					aria-label="닫기"
				>
					<X size={16} />
				</Dialog.Close>
			</header>
			{#if pendingAction}
				<div class="grid gap-4 p-4">
					<div class="border border-amber-300/20 bg-amber-300/6 px-3 py-2">
						<strong class="block text-sm text-amber-100">
							{pendingAction.action}
						</strong>
						<p class="mt-1 text-xs text-slate-500">
							{pendingAction.target}
						</p>
					</div>
					<label class="grid gap-1.5 text-xs font-semibold text-slate-500">
						실행 사유
						<input
							bind:value={reason}
							placeholder="감사 로그에 남길 사유"
							class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none placeholder:text-slate-600"
						/>
					</label>
				</div>
				<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] p-3">
					<Dialog.Close
						class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400"
						>취소</Dialog.Close
					>
					<button
						type="button"
						disabled={!canConfirm || submitting}
						onclick={confirmAction}
						class="h-9 border border-amber-300/35 bg-amber-300/10 px-3 text-xs font-semibold text-amber-100 disabled:opacity-35"
					>
						{submitting ? '복구 중' : '복구 실행'}
					</button>
				</footer>
			{/if}
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
