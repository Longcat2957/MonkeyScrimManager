<script lang="ts">
	import { resolve } from '$app/paths';
	import { Check, Crown, Link, RefreshCw, ShieldCheck, Trash2, X } from 'lucide-svelte';
	import { apiRoute } from '$lib/routing';
	import {
		apiErrorEnvelopeSchema,
		type RiotAccount,
		responseMetaSchema,
		riotAccountEnvelopeSchema,
		riotAccountsDataSchema
	} from '$lib/contracts/workflow';
	import { z } from 'zod';

	type RiotAccountsPayload = z.infer<typeof riotAccountsDataSchema>;
	type RiotAccountPayload = z.infer<typeof riotAccountEnvelopeSchema>;

	interface RiotAccountLookup {
		puuid?: string;
		platform?: string;
		routingRegion?: string;
		gameName?: string;
		tagLine?: string;
		profileIconId?: number | null;
	}

	interface Props {
		version: string;
		initialAccounts: RiotAccount[];
	}

	let { version, initialAccounts } = $props();
	let accounts = $state<RiotAccount[]>([]);
	let riotId = $state('');
	let platform = $state('KR');
	let routingRegion = $state('KR');
	let error = $state<string | null>(null);
	let notice = $state<string | null>(null);
	let actioningPuuid = $state<string | null>(null);
	let pendingDeletePuuid = $state<string | null>(null);
	let creating = $state(false);
	let testingRiot = $state(false);

	const sortedAccounts = $derived(
		[...accounts].sort(
			(a, b) => Number(b.isMain) - Number(a.isMain) || b.createdAt.localeCompare(a.createdAt)
		)
	);
	const mainAccount = $derived(accounts.find((account) => account.isMain) ?? null);
	const accountListRoute = apiRoute(['me', 'riot-accounts']);
	$effect(() => {
		accounts = [...initialAccounts];
	});

	function readCookie(name: string): string | null {
		const prefix = `${name}=`;
		const value = globalThis.document.cookie
			.split(';')
			.map((cookie) => cookie.trim())
			.find((cookie) => cookie.startsWith(prefix));
		return value ? decodeURIComponent(value.slice(prefix.length)) : null;
	}

	function buildHeaders(includeCsrf: boolean): Record<string, string> {
		const headers: Record<string, string> = {
			accept: 'application/json',
			'content-type': 'application/json'
		};
		if (includeCsrf) {
			const csrfToken = readCookie('monkeybot_csrf');
			if (!csrfToken) {
				throw new Error('CSRF 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.');
			}
			headers['X-CSRF-Token'] = csrfToken;
		}
		return headers;
	}

	function makeEnvelopeSchema<T>(dataSchema: z.ZodType<T>) {
		return z
			.object({
				data: dataSchema,
				meta: responseMetaSchema
			})
			.strict();
	}

	type FetchInit = Exclude<Parameters<typeof globalThis.fetch>[1], undefined>;

	async function requestRiot<T>(path: string, init: FetchInit, schema: z.ZodType<T>): Promise<T> {
		const response = await globalThis.fetch(path, init);
		const payload = await response.json().catch(() => null);

		if (!response.ok) {
			const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
			const message = parsedError.success
				? parsedError.data.error.message
				: `${response.status} 오류`;
			throw new Error(message);
		}

		const parsed = makeEnvelopeSchema(schema).safeParse(payload);
		if (!parsed.success) {
			throw new Error('API 응답 형식이 올바르지 않습니다.');
		}
		return parsed.data.data;
	}

	async function reloadAccounts() {
		const payload = await requestRiot<RiotAccountsPayload>(
			accountListRoute,
			{ method: 'GET' },
			riotAccountsDataSchema
		);
		accounts = [...payload.riotAccounts];
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

	function profileIconUrl(id: number): string {
		return `/dd/cdn/${version}/img/profileicon/${id}.png`;
	}

	function isDuplicate(puuid: string, gameName: string, tagLine: string): RiotAccount | undefined {
		const normalizedGame = gameName.toLowerCase();
		const normalizedTag = tagLine.toLowerCase();
		return accounts.find(
			(account) =>
				account.puuid === puuid ||
				(account.gameName.toLowerCase() === normalizedGame &&
					account.tagLine.toLowerCase() === normalizedTag)
		);
	}

	function toString(value: unknown): string | null {
		return typeof value === 'string' && value.trim().length > 0 ? value.trim() : null;
	}

	function toInt(value: unknown): number | null {
		if (typeof value === 'number' && Number.isFinite(value)) return Math.trunc(value);
		if (typeof value === 'string' && value.trim().length > 0) {
			const parsed = Number(value);
			return Number.isFinite(parsed) ? Math.trunc(parsed) : null;
		}
		return null;
	}

	function scanCandidates(value: unknown, list: RiotAccountLookup[]) {
		if (!value || typeof value !== 'object') return;
		if (Array.isArray(value)) {
			for (const item of value) scanCandidates(item, list);
			return;
		}

		const node = value as Record<string, unknown>;
		const puuid = toString(node.puuid);
		const gameName = toString(node.gameName) ?? toString(node.game_name);
		const tagLine = toString(node.tagLine) ?? toString(node.tag_line);
		const lookupPlatform = toString(node.platform);
		const lookupRoutingRegion = toString(node.routingRegion) ?? toString(node.routing_region);
		const profileIconId =
			toInt(node.profileIconId) ?? toInt(node.profile_icon_id) ?? toInt(node.profileiconId);

		if (puuid) {
			list.push({
				puuid,
				gameName: gameName ?? undefined,
				tagLine: tagLine ?? undefined,
				platform: lookupPlatform ?? undefined,
				routingRegion: lookupRoutingRegion ?? undefined,
				profileIconId
			});
		}

		for (const child of Object.values(node)) {
			scanCandidates(child, list);
		}
	}

	function pickRiotLookup(
		payload: unknown,
		expectedGame: string,
		expectedTag: string
	): RiotAccountLookup | null {
		const candidates: RiotAccountLookup[] = [];
		scanCandidates(payload, candidates);

		const exact = candidates.find((candidate) => {
			if (!candidate.puuid || !candidate.gameName || !candidate.tagLine) return false;
			return (
				candidate.gameName.toLowerCase() === expectedGame.toLowerCase() &&
				candidate.tagLine.toLowerCase() === expectedTag.toLowerCase()
			);
		});
		if (exact) return exact;

		const fallback = candidates.find(
			(candidate) => candidate.puuid && candidate.platform && candidate.routingRegion
		);
		return fallback ?? candidates[0] ?? null;
	}

	async function verifyRiotLookup(): Promise<RiotAccountLookup | null> {
		const parsed = parseRiotId(riotId);
		if (!parsed) throw new Error('GameName#TagLine 형식으로 입력하세요.');

		const endpoint = apiRoute(['riot', 'records'], {
			query: `${parsed.gameName}#${parsed.tagLine}`,
			region: routingRegion.toLowerCase()
		});
		testingRiot = true;
		try {
			const response = await globalThis.fetch(endpoint);
			const payload = await response.json().catch(() => null);
			if (!response.ok) {
				const message = (() => {
					const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
					return parsedError.success ? parsedError.data.error.message : `${response.status} 오류`;
				})();
				throw new Error(message);
			}
			const lookup = pickRiotLookup(payload, parsed.gameName, parsed.tagLine);
			if (!lookup) throw new Error('Riot 계정 조회 결과에서 PUUID를 추출할 수 없습니다.');
			return lookup;
		} finally {
			testingRiot = false;
		}
	}

	async function addAccount(event: globalThis.SubmitEvent) {
		event.preventDefault();
		error = null;
		notice = null;

		const parsed = parseRiotId(riotId);
		if (!parsed) {
			error = 'GameName#TagLine 형식으로 입력하세요.';
			return;
		}

		let lookup: RiotAccountLookup | null = null;
		try {
			lookup = await verifyRiotLookup();
		} catch (lookupError) {
			error =
				lookupError instanceof Error ? lookupError.message : 'Riot 계정 조회 중 오류가 발생했습니다.';
			return;
		}

		const mergedPuuid = lookup?.puuid;
		if (!mergedPuuid) {
			error = '입력한 Riot 계정을 찾을 수 없습니다.';
			return;
		}

		const normalizedTagLine = parsed.tagLine.toUpperCase();
		if (isDuplicate(mergedPuuid, parsed.gameName, normalizedTagLine)) {
			error = '이미 연결된 계정입니다.';
			return;
		}

		const body = {
			puuid: mergedPuuid,
			platform: (lookup?.platform || platform || routingRegion).toUpperCase(),
			routingRegion: (lookup?.routingRegion || routingRegion).toUpperCase(),
			gameName: parsed.gameName,
			tagLine: normalizedTagLine
		};

		creating = true;
		try {
			const created = await requestRiot<RiotAccountPayload>(
				accountListRoute,
				{
					method: 'POST',
					headers: buildHeaders(true),
					body: JSON.stringify(body)
				},
				riotAccountEnvelopeSchema
			);
			accounts = [...accounts, created.riotAccount];
			accounts = [...accounts].sort(
				(a, b) => Number(b.isMain) - Number(a.isMain) || b.createdAt.localeCompare(a.createdAt)
			);
			riotId = '';
			notice = `${formatRiotId(created.riotAccount)} 연결됨`;
		} catch (addError) {
			error =
				addError instanceof Error ? addError.message : '계정 연결 처리 중 오류가 발생했습니다.';
		} finally {
			creating = false;
		}
	}

	async function setMain(account: RiotAccount) {
		if (actioningPuuid === account.puuid) return;
		actioningPuuid = account.puuid;
		try {
			const result = await requestRiot<RiotAccountPayload>(
				resolve('/api/[...path]', { path: `me/riot-accounts/${account.puuid}/main` }),
				{
					method: 'PUT',
					headers: buildHeaders(true)
				},
				riotAccountEnvelopeSchema
			);
			accounts = accounts.map((item) =>
				item.puuid === result.riotAccount.puuid
					? { ...item, ...result.riotAccount, isMain: true }
					: { ...item, isMain: false }
			);
			notice = `${formatRiotId(result.riotAccount)} 메인 지정됨`;
		} catch (setMainError) {
			error =
				setMainError instanceof Error ? setMainError.message : '메인 지정 중 오류가 발생했습니다.';
		} finally {
			actioningPuuid = null;
		}
	}

	async function refreshAccount(account: RiotAccount) {
		if (actioningPuuid === account.puuid) return;
		actioningPuuid = account.puuid;
		try {
			const result = await requestRiot<RiotAccountPayload>(
				resolve('/api/[...path]', { path: `me/riot-accounts/${account.puuid}/refresh` }),
				{
					method: 'POST',
					headers: buildHeaders(true)
				},
				riotAccountEnvelopeSchema
			);
			accounts = accounts.map((item) =>
				item.puuid === result.riotAccount.puuid ? { ...result.riotAccount } : item
			);
			notice = `${formatRiotId(result.riotAccount)} 동기화됨`;
		} catch (refreshError) {
			error =
				refreshError instanceof Error ? refreshError.message : '동기화 중 오류가 발생했습니다.';
		} finally {
			actioningPuuid = null;
		}
	}

	async function removeAccount(account: RiotAccount) {
		if (actioningPuuid === account.puuid) return;
		actioningPuuid = account.puuid;
		try {
			await requestRiot<RiotAccountPayload>(
				resolve('/api/[...path]', { path: `me/riot-accounts/${account.puuid}` }),
				{
					method: 'DELETE',
					headers: buildHeaders(true)
				},
				riotAccountEnvelopeSchema
			);
			await reloadAccounts();
			pendingDeletePuuid = null;
			notice = `${formatRiotId(account)} 연결 해제됨`;
		} catch (deleteError) {
			error =
				deleteError instanceof Error ? deleteError.message : '연결 해제 중 오류가 발생했습니다.';
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
				<div class={`account-row ${account.isMain ? 'main-account' : ''}`}>
					<div class="flex min-w-0 items-center gap-3">
						{#if account.profileIconId !== null}
							<img
								src={profileIconUrl(account.profileIconId)}
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
								disabled={actioningPuuid !== null}
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
							disabled={actioningPuuid !== null}
							aria-label={`${formatRiotId(account)} 동기화`}
							title="동기화"
							onclick={() => refreshAccount(account)}
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
							disabled={actioningPuuid !== null}
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
								<button type="button" class="confirm-delete" onclick={() => removeAccount(account)}>
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
			{#if testingRiot}
				<p class="mb-2 text-xs text-slate-500 sm:mb-0 sm:mr-auto">Riot API 조회 중...</p>
			{/if}
			<form
				class="grid min-w-0 flex-1 gap-3 sm:grid-cols-[1fr_130px]"
				onsubmit={addAccount}
			>
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
					disabled={creating}
					class="inline-flex h-10 w-full min-w-0 items-center justify-center gap-1.5 rounded-sm border border-cyan-400/45 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/15 disabled:opacity-35"
				>
					{creating ? '연결 중...' : '연결'}
				</button>
			</form>
			{#if error}
				<p class="text-xs text-rose-300 sm:col-span-full">{error}</p>
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
