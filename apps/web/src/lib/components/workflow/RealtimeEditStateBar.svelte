<script lang="ts">
	import { Check, CloudAlert, RefreshCw, TriangleAlert, Wifi, WifiOff } from 'lucide-svelte';
	import type {
		RealtimeConnectionState,
		RealtimeEditor,
		RealtimeSaveState
	} from '$lib/realtime/types';

	interface Props {
		resourceLabel: string;
		connectionState: RealtimeConnectionState;
		saveState: RealtimeSaveState;
		editors?: RealtimeEditor[];
		lastSavedAt?: string;
		conflictMessage?: string;
		onReconnect?: () => void;
		onAcceptServer?: () => void;
		onKeepLocal?: () => void;
	}

	let {
		resourceLabel,
		connectionState,
		saveState,
		editors = [],
		lastSavedAt = '방금',
		conflictMessage = '다른 운영자가 먼저 저장한 변경이 있습니다.',
		onReconnect,
		onAcceptServer,
		onKeepLocal
	}: Props = $props();

	const connectionMeta = $derived.by(() => {
		if (connectionState === 'connected') {
			return { label: '실시간 연결', icon: Wifi, class: 'text-emerald-300' };
		}
		if (connectionState === 'reconnecting') {
			return { label: '재연결 중', icon: RefreshCw, class: 'animate-spin text-amber-200' };
		}
		return { label: '연결 끊김', icon: WifiOff, class: 'text-red-300' };
	});

	const saveMeta = $derived.by(() => {
		if (saveState === 'saved') {
			return { label: '저장됨', icon: Check, class: 'text-emerald-300' };
		}
		if (saveState === 'saving') {
			return { label: '저장 중', icon: RefreshCw, class: 'animate-spin text-cyan-200' };
		}
		if (saveState === 'dirty') {
			return { label: '저장 대기', icon: CloudAlert, class: 'text-amber-200' };
		}
		return { label: '변경 충돌', icon: TriangleAlert, class: 'text-red-300' };
	});

	function initials(name: string): string {
		return name.trim().slice(0, 1).toUpperCase();
	}
</script>

<section
	class={`overflow-hidden border bg-[var(--color-app-surface)] ${
		saveState === 'conflict' || connectionState === 'offline'
			? 'border-red-400/30'
			: connectionState === 'reconnecting' || saveState === 'dirty'
				? 'border-amber-300/30'
				: 'border-[var(--color-app-border)]'
	}`}
	aria-live="polite"
>
	<div class="flex min-h-10 items-center gap-3 px-3 py-1.5">
		<div class="flex min-w-0 flex-1 items-center gap-2">
			<span class="grid size-5 shrink-0 place-items-center" title={connectionMeta.label}>
				<connectionMeta.icon
					size={14}
					class={`shrink-0 ${connectionMeta.class}`}
					aria-hidden="true"
				/>
			</span>
			<strong class="truncate text-xs text-slate-100">{resourceLabel}</strong>
			<span class="hidden shrink-0 text-[10px] text-slate-600 lg:inline">
				· {connectionMeta.label}
			</span>
		</div>

		<div class="flex shrink-0 items-center gap-1.5" title={`${editors.length}명 편집 중`}>
			<div class="flex -space-x-1.5">
				{#each editors.slice(0, 3) as editor (editor.id)}
					<span
						title={`${editor.name}${editor.isSelf ? ' (나)' : ''}`}
						class={`grid size-6 place-items-center rounded-full border text-[9px] font-bold ${
							editor.isSelf
								? 'border-cyan-300/45 bg-cyan-400/15 text-cyan-100'
								: 'border-slate-600 bg-slate-800 text-slate-300'
						}`}
					>
						{initials(editor.name)}
					</span>
				{/each}
			</div>
			<span class="hidden whitespace-nowrap text-[10px] text-slate-500 md:inline">
				{editors.length > 0 ? `${editors.length}명 편집 중` : '편집자 없음'}
			</span>
		</div>

		<div
			class="flex shrink-0 items-center gap-1.5 border-l border-[var(--color-app-border-muted)] pl-3"
		>
			<saveMeta.icon size={13} class={`shrink-0 ${saveMeta.class}`} aria-hidden="true" />
			<span class="whitespace-nowrap text-[11px] font-semibold text-slate-300"
				>{saveMeta.label}</span
			>
			{#if saveState === 'saved'}
				<span class="hidden whitespace-nowrap text-[10px] text-slate-600 lg:inline">
					· {lastSavedAt}
				</span>
			{/if}
		</div>
	</div>

	{#if saveState === 'conflict'}
		<div
			class="flex flex-col gap-2 border-t border-red-400/20 bg-red-500/[0.06] px-3 py-2 sm:flex-row sm:items-center"
		>
			<div class="flex min-w-0 flex-1 items-start gap-2">
				<TriangleAlert size={15} class="mt-0.5 shrink-0 text-red-300" aria-hidden="true" />
				<p class="text-xs leading-5 text-red-100">{conflictMessage}</p>
			</div>
			<div class="flex shrink-0 justify-end gap-1.5">
				<button
					type="button"
					onclick={onKeepLocal}
					class="inline-flex h-7 items-center rounded-sm px-2.5 text-[10px] font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] hover:text-slate-100"
				>
					내 입력 유지
				</button>
				<button
					type="button"
					onclick={onAcceptServer}
					class="inline-flex h-7 items-center rounded-sm border border-red-300/30 bg-red-500/10 px-2.5 text-[10px] font-semibold text-red-100 hover:bg-red-500/15"
				>
					최신 상태 반영
				</button>
			</div>
		</div>
	{:else if connectionState === 'offline'}
		<div
			class="flex items-center justify-between gap-3 border-t border-red-400/20 bg-red-500/[0.06] px-3 py-2"
		>
			<p class="text-xs text-red-100">변경 알림을 받을 수 없습니다.</p>
			<button
				type="button"
				onclick={onReconnect}
				class="inline-flex h-7 shrink-0 items-center gap-1.5 rounded-sm border border-red-300/30 px-2.5 text-[10px] font-semibold text-red-100 hover:bg-red-500/10"
			>
				<RefreshCw size={13} aria-hidden="true" /> 재연결
			</button>
		</div>
	{/if}
</section>
