<script lang="ts">
	import { Dialog } from 'bits-ui';
	import { ClipboardPenLine, X } from 'lucide-svelte';
	import type { SeriesPickBanBulkItem } from '$lib/contracts/seriesPickBan';
	import type {
		ScrimPickBanResultInputChampion,
		ScrimPickBanResultInputSide,
		ScrimPickBanResultInputTeam
	} from '$lib/fixtures/scrimPickBanResultInputFixtures';
	import {
		buildScrimPickBanBulkInputPlan,
		emptyScrimPickBanBulkInputValues,
		type ScrimPickBanBulkInputFieldKey,
		type ScrimPickBanBulkInputValues
	} from './scrimPickBanBulkInput';

	interface Props {
		open?: boolean;
		teams: Record<ScrimPickBanResultInputSide, ScrimPickBanResultInputTeam>;
		picks: Record<string, string | null>;
		bans: Record<ScrimPickBanResultInputSide, Array<string | null>>;
		champions: ScrimPickBanResultInputChampion[];
		hardFearless?: boolean;
		previousPickChampionIds?: readonly string[];
		readOnly?: boolean;
		busy?: boolean;
		onApply: (items: SeriesPickBanBulkItem[]) => void | Promise<void>;
	}

	let {
		open = $bindable(false),
		teams,
		picks,
		bans,
		champions,
		hardFearless = false,
		previousPickChampionIds = [],
		readOnly = false,
		busy = false,
		onApply
	}: Props = $props();

	let applying = $state(false);
	let values = $state<ScrimPickBanBulkInputValues>(emptyScrimPickBanBulkInputValues());
	const plan = $derived(
		buildScrimPickBanBulkInputPlan({
			values,
			teams,
			picks,
			bans,
			champions,
			hardFearless,
			previousPickChampionIds
		})
	);
	const disabled = $derived(readOnly || busy || applying);

	function updateValue(key: ScrimPickBanBulkInputFieldKey, value: string) {
		values = { ...values, [key]: value };
	}

	async function applyAll() {
		if (disabled || !plan.canApply) return;
		const operations = plan.operations;
		applying = true;
		try {
			await onApply(operations);
		} finally {
			applying = false;
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 flex max-h-[calc(100vh-1.5rem)] w-[calc(100vw-1.5rem)] max-w-2xl -translate-x-1/2 -translate-y-1/2 flex-col overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex min-h-12 items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
			>
				<div class="flex min-w-0 items-center gap-2">
					<ClipboardPenLine size={16} class="shrink-0 text-cyan-200" aria-hidden="true" />
					<div class="min-w-0">
						<Dialog.Title class="text-sm font-semibold text-slate-50">일괄 입력</Dialog.Title>
						<p class="truncate text-[11px] text-slate-500">
							쉼표로 구분 · 픽은 탑 / 정글 / 미드 / 원딜 / 서폿 순서
						</p>
					</div>
				</div>
				<Dialog.Close
					disabled={applying}
					class="grid size-8 shrink-0 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					aria-label="일괄 입력 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>

			<div class="min-h-0 overflow-y-auto p-4">
				<div class="grid gap-3 lg:grid-cols-2">
					{#each plan.fields as field (field.definition.key)}
						<div
							class="grid min-w-0 gap-1.5 border-l-2 pl-2 {field.definition.side === 'BLUE'
								? 'border-blue-300/70'
								: 'border-red-300/70'}"
						>
							<label
								class="text-xs font-semibold text-slate-200"
								for={`scrim-bulk-input-${field.definition.key}`}
							>
								{field.definition.label}
							</label>
							<input
								id={`scrim-bulk-input-${field.definition.key}`}
								type="text"
								value={values[field.definition.key]}
								placeholder={field.definition.placeholder}
								autocomplete="off"
								class="h-9 min-w-0 border border-[var(--color-app-border)] bg-slate-950/35 px-2.5 text-sm font-medium text-slate-100 outline-none transition placeholder:text-slate-600 focus:border-cyan-300/65 disabled:cursor-not-allowed disabled:opacity-50"
								{disabled}
								oninput={(event) => updateValue(field.definition.key, event.currentTarget.value)}
								onkeydown={(event) => {
									if (event.key !== 'Enter' || event.isComposing) return;
									event.preventDefault();
									void applyAll();
								}}
							/>
							{#if field.feedback.length > 0}
								<div class="grid gap-0.5" aria-live="polite">
									{#each field.feedback as feedback (`${feedback.severity}-${feedback.message}`)}
										<p
											class={`text-[11px] leading-4 ${feedback.severity === 'error' ? 'text-rose-200' : 'text-slate-500'}`}
										>
											{feedback.message}
										</p>
									{/each}
								</div>
							{/if}
						</div>
					{/each}
				</div>
			</div>

			<footer
				class="flex flex-wrap items-center justify-between gap-3 border-t border-[var(--color-app-border)] px-4 py-3"
			>
				<p class="min-w-0 text-[11px] leading-4 text-slate-500">
					빈 항목과 매칭할 수 없는 이름은 기존 슬롯을 바꾸지 않습니다.
					{#if hardFearless}
						이전 세트에서 선택한 챔피언도 검사합니다.{/if}
				</p>
				<div class="flex shrink-0 items-center gap-2">
					<Dialog.Close
						disabled={applying}
						class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-slate-100 disabled:opacity-35"
					>
						취소
					</Dialog.Close>
					<button
						type="button"
						class="inline-flex h-9 items-center justify-center rounded-sm bg-cyan-400/12 px-4 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/18 disabled:cursor-not-allowed disabled:opacity-50"
						disabled={disabled || !plan.canApply}
						title={plan.hasValidationErrors
							? '입력 오류를 해결한 뒤 적용할 수 있습니다.'
							: plan.operations.length === 0
								? '변경할 유효한 챔피언을 입력하세요.'
								: undefined}
						onclick={() => void applyAll()}
					>
						{applying
							? '적용 중'
							: `일괄 적용${plan.operations.length > 0 ? ` · ${plan.operations.length}` : ''}`}
					</button>
				</div>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
