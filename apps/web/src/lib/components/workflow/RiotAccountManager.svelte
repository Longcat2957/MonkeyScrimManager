<script lang="ts">
	import { Check, Crown, Link, RefreshCw, ShieldCheck, Trash2, X } from 'lucide-svelte';
	import {
		createRiotAccount,
		deleteAccount,
		parseRiotIdInput,
		refreshAccount,
		setMainAccount
	} from '$lib/api/riot';
	import { riotProfileIconUrl, DEFAULT_PROFILE_ICON_VERSION } from '$lib/datadragon';
	import type { RiotAccount } from '$lib/contracts/workflow';

	interface Props {
		version?: string;
		initialAccounts: RiotAccount[];
	}

	let { version = DEFAULT_PROFILE_ICON_VERSION, initialAccounts }: Props = $props();
	let accounts = $derived<RiotAccount[]>([...initialAccounts]);
	let riotId = $state('');
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let actioningPuuid = $state<string | null>(null);
	let pendingDeletePuuid = $state<string | null>(null);
	let creating = $state(false);

	const sortedAccounts = $derived(
		[...accounts].sort(
			(a, b) => Number(b.isMain) - Number(a.isMain) || b.createdAt.localeCompare(a.createdAt)
		)
	);
	const mainAccount = $derived(accounts.find((account) => account.isMain) ?? null);
	const busy = $derived(creating || actioningPuuid !== null);

	function formatRiotId(account: Pick<RiotAccount, 'gameName' | 'tagLine'>): string {
		return `${account.gameName}#${account.tagLine}`;
	}

	function formatSyncedAt(value: string | null): string {
		if (!value) return '미동기화';
		const at = Date.parse(value);
		if (Number.isNaN(at)) return '동기화됨';

		const seconds = Math.max(0, Math.floor((Date.now() - at) / 1000));
		if (seconds < 60) return '방금';
		if (seconds < 3600) return `${Math.floor(seconds / 60)}분 전`;
		if (seconds < 86400) return `${Math.floor(seconds / 3600)}시간 전`;
		if (seconds < 86400 * 7) return `${Math.floor(seconds / 86400)}일 전`;
		return `${Math.floor(seconds / (86400 * 7))}주 전`;
	}

	function messageFrom(error: unknown, fallback: string): string {
		return error instanceof Error ? error.message : fallback;
	}

	async function addAccount(event: globalThis.SubmitEvent) {
		event.preventDefault();
		if (busy) return;
		error = null;
		notice = null;

		const parsed = parseRiotIdInput(riotId);
		if (!parsed) {
			error = 'GameName#TagLine 형식으로 입력하세요.';
			return;
		}

		creating = true;
		try {
			accounts = await createRiotAccount({
				gameName: parsed.gameName,
				tagLine: parsed.tagLine,
				platform: 'kr'
			});
			const created = accounts.find(
				(account) =>
					account.gameName.localeCompare(parsed.gameName, undefined, { sensitivity: 'base' }) ===
						0 &&
					account.tagLine.localeCompare(parsed.tagLine, undefined, { sensitivity: 'base' }) === 0
			);
			riotId = '';
			notice = `${created ? formatRiotId(created) : `${parsed.gameName}#${parsed.tagLine}`} 연결됨`;
		} catch (addError) {
			error = messageFrom(addError, '계정 연결 처리 중 오류가 발생했습니다.');
		} finally {
			creating = false;
		}
	}

	async function handleSetMain(puuid: string) {
		if (busy) return;
		error = null;
		notice = null;
		actioningPuuid = puuid;
		try {
			accounts = await setMainAccount(puuid);
			const main = accounts.find((account) => account.puuid === puuid);
			notice = main ? `${formatRiotId(main)} 메인 지정됨` : '메인 계정이 변경되었습니다.';
		} catch (err) {
			error = messageFrom(err, '메인 지정 중 오류가 발생했습니다.');
		} finally {
			actioningPuuid = null;
		}
	}

	async function handleRefresh(puuid: string) {
		if (busy) return;
		error = null;
		notice = null;
		actioningPuuid = puuid;
		try {
			accounts = await refreshAccount(puuid);
			const refreshed = accounts.find((account) => account.puuid === puuid);
			notice = refreshed ? `${formatRiotId(refreshed)} 동기화됨` : '계정이 동기화되었습니다.';
		} catch (err) {
			error = messageFrom(err, '동기화 중 오류가 발생했습니다.');
		} finally {
			actioningPuuid = null;
		}
	}

	async function handleDelete(puuid: string) {
		if (busy) return;
		error = null;
		notice = null;
		const deleting = accounts.find((account) => account.puuid === puuid);
		actioningPuuid = puuid;
		try {
			accounts = await deleteAccount(puuid);
			pendingDeletePuuid = null;
			notice = deleting ? `${formatRiotId(deleting)} 연결 해제됨` : '연결 해제됨';
		} catch (err) {
			error = messageFrom(err, '연결 해제 중 오류가 발생했습니다.');
		} finally {
			actioningPuuid = null;
		}
	}
</script>

<section
	class="min-w-0 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
>
	<header
		class="flex min-h-14 flex-wrap items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
	>
		<div class="flex min-w-0 items-center gap-3">
			<ShieldCheck size={20} class="text-cyan-300" aria-hidden="true" />
			<div class="min-w-0">
				<h2 class="truncate text-sm font-semibold text-slate-50">Riot 계정</h2>
				<p class="truncate text-xs text-slate-500">
					{accounts.length}개{mainAccount ? ` · 대표: ${formatRiotId(mainAccount)}` : ''}
				</p>
			</div>
		</div>
		{#if notice}
			<span
				class="inline-flex items-center gap-1 text-xs font-semibold text-emerald-300"
				aria-live="polite"
			>
				<Check size={14} aria-hidden="true" />
				{notice}
			</span>
		{/if}
	</header>

	<div class="grid min-w-0">
		<div class="min-w-0 overflow-hidden">
			<div
				class="account-table-head grid grid-cols-[minmax(240px,1fr)_120px_132px] items-center border-b border-[var(--color-app-border-muted)] bg-[var(--color-app-bg-soft)] px-4 py-2 text-[10px] font-semibold text-slate-600"
			>
				<span>계정</span><span>동기화</span><span class="text-right">관리</span>
			</div>
			{#each sortedAccounts as account (account.puuid)}
				<div class={`account-row ${account.isMain ? 'main-account' : ''}`}>
					<div class="flex min-w-0 items-center gap-3">
						{#if account.profileIconId !== null}
							<img
								src={riotProfileIconUrl(account.profileIconId, version)}
								alt=""
								class="size-11 shrink-0 object-cover border border-[var(--color-app-border)] bg-slate-950"
								loading="lazy"
							/>
						{:else}
							<div
								class="size-11 shrink-0 border border-[var(--color-app-border)] bg-slate-900 text-[9px] font-semibold text-slate-500 grid place-items-center"
							>
								RIOT
							</div>
						{/if}
						<div class="min-w-0">
							<div class="flex min-w-0 items-center gap-2">
								<strong class="truncate text-sm text-slate-100">{formatRiotId(account)}</strong>
								{#if account.isMain}
									<span
										class="inline-flex shrink-0 items-center gap-1 text-[10px] font-semibold text-amber-300"
									>
										<Crown size={12} aria-hidden="true" /> 메인
									</span>
								{/if}
							</div>
							<span class="block truncate text-[10px] text-slate-600">
								{account.platform}/{account.routingRegion}
							</span>
						</div>
					</div>
					<span class="text-xs text-slate-500">{formatSyncedAt(account.lastSyncedAt)}</span>
					<div class="flex justify-end gap-1">
						{#if !account.isMain}
							<button
								type="button"
								class="icon-button"
								disabled={busy}
								aria-label={`${formatRiotId(account)} 메인 지정`}
								title="메인 지정"
								onclick={() => handleSetMain(account.puuid)}
							>
								<Crown size={15} aria-hidden="true" />
							</button>
						{/if}
						<button
							type="button"
							class="icon-button"
							disabled={busy}
							aria-label={`${formatRiotId(account)} 동기화`}
							title="동기화"
							onclick={() => handleRefresh(account.puuid)}
						>
							<RefreshCw
								size={15}
								class={actioningPuuid === account.puuid ? 'animate-spin' : ''}
								aria-hidden="true"
							/>
						</button>
						<button
							type="button"
							class="icon-button danger"
							disabled={busy}
							aria-label={`${formatRiotId(account)} 연결 해제`}
							title="연결 해제"
							onclick={() => (pendingDeletePuuid = account.puuid)}
						>
							<Trash2 size={15} aria-hidden="true" />
						</button>
					</div>

					{#if pendingDeletePuuid === account.puuid}
						<div class="delete-confirmation">
							<strong>{formatRiotId(account)} 연결 해제</strong>
							<div class="flex gap-1">
								<button
									type="button"
									class="icon-button"
									aria-label="취소"
									title="취소"
									onclick={() => (pendingDeletePuuid = null)}
								>
									<X size={15} aria-hidden="true" />
								</button>
								<button
									type="button"
									class="confirm-delete"
									onclick={() => handleDelete(account.puuid)}
								>
									해제
								</button>
							</div>
						</div>
					{/if}
				</div>
			{:else}
				<p class="px-4 py-12 text-center text-sm text-slate-600">연결된 계정 없음</p>
			{/each}
		</div>

		<div
			class="border-t border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-4 sm:flex sm:items-end sm:gap-3"
		>
			<form class="grid min-w-0 flex-1 gap-3 sm:grid-cols-[1fr_130px]" onsubmit={addAccount}>
				<label
					class="flex h-10 min-w-0 items-center gap-2 rounded-sm border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 focus-within:border-cyan-400/50"
				>
					<Link size={15} class="shrink-0 text-slate-500" aria-hidden="true" />
					<input
						bind:value={riotId}
						class="w-full min-w-0 bg-transparent text-sm text-slate-100 outline-none placeholder:text-slate-600"
						placeholder="Riot ID (예: Longcat#KR1)"
						autocomplete="off"
					/>
				</label>
				<button
					type="submit"
					disabled={busy}
					class="inline-flex h-10 w-full min-w-0 items-center justify-center gap-1.5 rounded-sm border border-cyan-400/45 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/15 disabled:opacity-35"
				>
					{creating ? '연결 중...' : '연결'}
				</button>
			</form>
			{#if error}
				<p class="text-xs text-rose-300 sm:col-span-full" aria-live="assertive">{error}</p>
			{/if}
		</div>
	</div>
</section>

<style>
	.account-row {
		position: relative;
		display: grid;
		grid-template-columns: minmax(240px, 1fr) 120px 132px;
		min-height: 70px;
		align-items: center;
		padding: 10px 16px;
		border-bottom: 1px solid var(--color-app-border-muted);
	}
	.account-row.main-account {
		box-shadow: inset 3px 0 #f59e0b;
		background: rgb(245 158 11 / 0.035);
	}
	.icon-button {
		display: grid;
		width: 32px;
		height: 32px;
		place-items: center;
		border-radius: 2px;
		color: #64748b;
	}
	.icon-button:hover {
		background: var(--color-app-hover);
		color: #e2e8f0;
	}
	.icon-button:disabled {
		opacity: 0.35;
	}
	.icon-button.danger:hover {
		background: rgb(251 113 133 / 0.1);
		color: #fda4af;
	}
	.delete-confirmation {
		position: absolute;
		inset: 6px 8px;
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		background: #25161d;
		padding: 0 12px;
		color: #fecdd3;
		font-size: 12px;
	}
	.confirm-delete {
		height: 32px;
		border-radius: 2px;
		background: rgb(251 113 133 / 0.16);
		padding: 0 12px;
		color: #fecdd3;
		font-size: 11px;
		font-weight: 700;
	}
	@media (max-width: 720px) {
		.account-table-head {
			grid-template-columns: minmax(0, 1fr) auto;
		}
		.account-table-head > :nth-child(2) {
			display: none;
		}
		.account-row {
			grid-template-columns: minmax(0, 1fr) auto;
			gap: 8px;
		}
		.account-row > :nth-child(2) {
			display: none;
		}
	}
</style>
