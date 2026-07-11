<script lang="ts">
	import { Check, Crown, Link, RefreshCw, ShieldCheck, Trash2, X } from 'lucide-svelte';

	interface RiotAccount {
		puuid: string;
		gameName: string;
		tagLine: string;
		profileIconId: number;
		isMain: boolean;
		lastSynced: string;
	}

	interface Props {
		version: string;
	}

	let { version }: Props = $props();
	let accounts = $state<RiotAccount[]>([
		{
			puuid: 'mock-puuid-longcat',
			gameName: 'Longcat',
			tagLine: 'KR1',
			profileIconId: 588,
			isMain: true,
			lastSynced: '2분 전'
		},
		{
			puuid: 'mock-puuid-tamsol',
			gameName: '탐솔',
			tagLine: 'MSM',
			profileIconId: 22,
			isMain: false,
			lastSynced: '3일 전'
		}
	]);
	let riotId = $state('');
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let busyPuuid = $state<string | null>(null);
	let pendingDeletePuuid = $state<string | null>(null);

	const sortedAccounts = $derived(
		[...accounts].sort((a, b) => Number(b.isMain) - Number(a.isMain))
	);
	const mainAccount = $derived(accounts.find((account) => account.isMain) ?? null);

	function formatRiotId(account: Pick<RiotAccount, 'gameName' | 'tagLine'>): string {
		return `${account.gameName}#${account.tagLine}`;
	}

	function parseRiotId(value: string): { gameName: string; tagLine: string } | null {
		const trimmed = value.trim();
		const separator = trimmed.lastIndexOf('#');
		if (separator <= 0 || separator >= trimmed.length - 1) return null;
		return {
			gameName: trimmed.slice(0, separator).trim(),
			tagLine: trimmed
				.slice(separator + 1)
				.trim()
				.toUpperCase()
		};
	}

	function addAccount(event: SubmitEvent) {
		event.preventDefault();
		error = null;
		notice = null;
		const parsed = parseRiotId(riotId);
		if (!parsed) {
			error = 'GameName#TagLine 형식으로 입력하세요.';
			return;
		}
		if (
			accounts.some(
				(account) =>
					account.gameName.toLocaleLowerCase() === parsed.gameName.toLocaleLowerCase() &&
					account.tagLine.toLocaleLowerCase() === parsed.tagLine.toLocaleLowerCase()
			)
		) {
			error = '이미 연결된 계정입니다.';
			return;
		}

		const next: RiotAccount = {
			puuid: `mock-puuid-${Date.now()}`,
			gameName: parsed.gameName,
			tagLine: parsed.tagLine,
			profileIconId: 7,
			isMain: accounts.length === 0,
			lastSynced: '방금'
		};
		accounts = [...accounts, next];
		riotId = '';
		notice = `${formatRiotId(next)} 연결됨`;
	}

	function setMain(account: RiotAccount) {
		accounts = accounts.map((item) => ({ ...item, isMain: item.puuid === account.puuid }));
		notice = `${formatRiotId(account)} 메인 지정됨`;
		error = null;
	}

	function refresh(account: RiotAccount) {
		busyPuuid = account.puuid;
		error = null;
		notice = null;
		window.setTimeout(() => {
			accounts = accounts.map((item) =>
				item.puuid === account.puuid ? { ...item, lastSynced: '방금' } : item
			);
			busyPuuid = null;
			notice = `${formatRiotId(account)} 동기화됨`;
		}, 450);
	}

	function removeAccount(account: RiotAccount) {
		const remaining = accounts.filter((item) => item.puuid !== account.puuid);
		if (account.isMain && remaining.length > 0) remaining[0] = { ...remaining[0], isMain: true };
		accounts = remaining;
		pendingDeletePuuid = null;
		notice = `${formatRiotId(account)} 연결 해제됨`;
		error = null;
	}

	function profileIconUrl(id: number): string {
		return `/dd/cdn/${version}/img/profileicon/${id}.png`;
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
					{accounts.length}개{mainAccount ? ` · ${formatRiotId(mainAccount)}` : ''}
				</p>
			</div>
		</div>
		{#if notice}
			<span class="inline-flex items-center gap-1 text-xs font-semibold text-emerald-300">
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
				<div class:main-account={account.isMain} class="account-row">
					<div class="flex min-w-0 items-center gap-3">
						<img
							src={profileIconUrl(account.profileIconId)}
							alt=""
							class="size-11 shrink-0 object-cover"
							loading="lazy"
						/>
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
							<span class="block truncate text-[10px] text-slate-600">KR</span>
						</div>
					</div>
					<span class="text-xs text-slate-500">{account.lastSynced}</span>
					<div class="flex justify-end gap-1">
						{#if !account.isMain}
							<button
								type="button"
								class="icon-button"
								aria-label={`${formatRiotId(account)} 메인 지정`}
								title="메인 지정"
								onclick={() => setMain(account)}
							>
								<Crown size={15} aria-hidden="true" />
							</button>
						{/if}
						<button
							type="button"
							class="icon-button"
							disabled={busyPuuid !== null}
							aria-label={`${formatRiotId(account)} 동기화`}
							title="동기화"
							onclick={() => refresh(account)}
						>
							<RefreshCw
								size={15}
								class={busyPuuid === account.puuid ? 'animate-spin' : ''}
								aria-hidden="true"
							/>
						</button>
						<button
							type="button"
							class="icon-button danger"
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
								<button type="button" class="confirm-delete" onclick={() => removeAccount(account)}
									>해제</button
								>
							</div>
						</div>
					{/if}
				</div>
			{:else}
				<p class="px-4 py-12 text-center text-sm text-slate-600">연결된 계정 없음</p>
			{/each}
		</div>

		<form
			class="grid min-w-0 gap-3 border-t border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] p-4 sm:grid-cols-[160px_minmax(0,1fr)_120px] sm:items-end"
			onsubmit={addAccount}
		>
			<div>
				<h3 class="text-sm font-semibold text-slate-100">계정 연결</h3>
				<p class="mt-1 text-xs text-slate-600">GameName#TagLine</p>
			</div>
			<div class="grid min-w-0 gap-1">
				<label
					class="flex h-10 min-w-0 items-center gap-2 rounded-sm border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 focus-within:border-cyan-400/50"
				>
					<Link size={15} class="shrink-0 text-slate-500" aria-hidden="true" />
					<input
						bind:value={riotId}
						class="w-full min-w-0 bg-transparent text-sm text-slate-100 outline-none placeholder:text-slate-600"
						placeholder="Longcat#KR1"
						autocomplete="off"
					/>
				</label>
				{#if error}<p class="truncate text-xs text-rose-300">{error}</p>{/if}
			</div>
			<button
				type="submit"
				class="inline-flex h-10 w-full min-w-0 items-center justify-center gap-1.5 rounded-sm border border-cyan-400/45 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/15"
			>
				<Link size={14} aria-hidden="true" /> 연결
			</button>
		</form>
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
