<script lang="ts">
	import { Dialog } from 'bits-ui';
	import {
		AlertTriangle,
		Check,
		DatabaseBackup,
		Download,
		FileJson,
		FileUp,
		History,
		LoaderCircle,
		ShieldCheck,
		Upload,
		X
	} from 'lucide-svelte';
	import {
		applyGuildFullSnapshotImport,
		exportGuildFullSnapshot,
		GuildDataTransferApiError,
		previewGuildFullSnapshotImport,
		serializeGuildFullSnapshotArchive
	} from '$lib/api/guildDataTransfer';
	import {
		guildFullSnapshotArchiveSchema,
		guildFullSnapshotMaxBytes,
		type GuildFullDataTransferPreview,
		type GuildFullSnapshotArchive
	} from '$lib/contracts/guildFullDataTransfer';

	interface Props {
		guildId: string;
		guildName: string;
		activeMemberCount: number;
		currentSeasonName: string;
	}

	let { guildId, guildName, activeMemberCount, currentSeasonName }: Props = $props();

	let archiveInput = $state<globalThis.HTMLInputElement>();
	let selectedFileName = $state<string | null>(null);
	let selectedArchive = $state<GuildFullSnapshotArchive | null>(null);
	let preview = $state<GuildFullDataTransferPreview | null>(null);
	let exportError = $state('');
	let importError = $state('');
	let notice = $state('');
	let exporting = $state(false);
	let previewing = $state(false);
	let applying = $state(false);
	let applyDialogOpen = $state(false);
	let confirmationValue = $state('');
	let applyIntent = $state('');
	let applyIdempotencyKey = $state('');
	let previewClock = $state(Date.now());

	const hasPreviewConflicts = $derived((preview?.conflicts.length ?? 0) > 0);
	const hasRows = $derived((preview?.summary.totalRows ?? 0) > 0);
	const isPreviewExpired = $derived(
		Boolean(preview) &&
			(!Number.isFinite(Date.parse(preview?.expiresAt ?? '')) ||
				Date.parse(preview?.expiresAt ?? '') <= previewClock)
	);
	const canOpenApplyDialog = $derived(
		Boolean(preview) && !hasPreviewConflicts && hasRows && !isPreviewExpired
	);
	const canApply = $derived(
		canOpenApplyDialog && confirmationValue.trim() === guildName && !applying
	);

	$effect(() => {
		if (!preview) return;
		const expiresAt = Date.parse(preview.expiresAt);
		if (!Number.isFinite(expiresAt)) return;
		const now = Date.now();
		previewClock = now;
		const timeout = globalThis.setTimeout(
			() => {
				previewClock = Date.now();
			},
			Math.max(0, expiresAt - now) + 1
		);
		return () => globalThis.clearTimeout(timeout);
	});

	async function exportArchive() {
		if (exporting) return;
		exporting = true;
		exportError = '';
		notice = '';
		try {
			const { archive, fileName, summary } = await exportGuildFullSnapshot(guildId);
			const blob = new globalThis.Blob([serializeGuildFullSnapshotArchive(archive)], {
				type: 'application/json;charset=utf-8'
			});
			const url = globalThis.URL.createObjectURL(blob);
			const anchor = globalThis.document.createElement('a');
			anchor.href = url;
			anchor.download = fileName;
			anchor.style.display = 'none';
			globalThis.document.body.append(anchor);
			anchor.click();
			anchor.remove();
			globalThis.URL.revokeObjectURL(url);
			notice = `전적과 MMR 이력을 포함한 ${formatCount(summary.totalRows)}개 행의 전체 백업을 준비했습니다.`;
		} catch (cause) {
			exportError = errorText(cause, '전체 데이터 백업을 만들지 못했습니다.');
		} finally {
			exporting = false;
		}
	}

	function selectArchiveFile() {
		archiveInput?.click();
	}

	function resetPreviewState() {
		preview = null;
		previewClock = Date.now();
		applyDialogOpen = false;
		confirmationValue = '';
		applyIntent = '';
		applyIdempotencyKey = '';
	}

	async function loadArchiveFile(event: globalThis.Event) {
		const input = event.currentTarget as globalThis.HTMLInputElement;
		const file = input.files?.[0];
		if (!file) return;

		selectedFileName = file.name;
		selectedArchive = null;
		resetPreviewState();
		importError = '';
		notice = '';

		try {
			if (file.size > guildFullSnapshotMaxBytes) {
				throw new Error('전체 백업 파일은 최대 16 MiB까지 지원합니다.');
			}
			const payload: unknown = JSON.parse(await file.text());
			const parsed = guildFullSnapshotArchiveSchema.safeParse(payload);
			if (!parsed.success) {
				throw new Error('선택한 파일은 지원되는 MonkeyBot 전체 데이터 백업 JSON이 아닙니다.');
			}
			selectedArchive = parsed.data;
		} catch (cause) {
			importError =
				cause instanceof SyntaxError
					? '선택한 파일이 올바른 JSON 형식이 아닙니다.'
					: errorText(cause, '선택한 파일을 읽지 못했습니다.');
		} finally {
			input.value = '';
		}
	}

	async function previewImport() {
		if (!selectedArchive || previewing) return;
		previewing = true;
		importError = '';
		notice = '';
		resetPreviewState();
		try {
			preview = await previewGuildFullSnapshotImport(guildId, selectedArchive);
		} catch (cause) {
			importError = errorText(cause, '전체 데이터 가져오기 검증을 완료하지 못했습니다.');
		} finally {
			previewing = false;
		}
	}

	function openApplyDialog() {
		if (!canOpenApplyDialog) return;
		confirmationValue = '';
		importError = '';
		applyDialogOpen = true;
	}

	function preventCloseWhileApplying(event: globalThis.Event) {
		if (applying) event.preventDefault();
	}

	async function applyImport() {
		if (!preview || !canApply) return;
		const intent = JSON.stringify([preview.importId, preview.digest, preview.targetDigest]);
		if (intent !== applyIntent) {
			applyIntent = intent;
			applyIdempotencyKey = globalThis.crypto.randomUUID();
		}

		applying = true;
		importError = '';
		notice = '';
		try {
			const result = await applyGuildFullSnapshotImport(guildId, {
				importId: preview.importId,
				expectedDigest: preview.digest,
				expectedTargetDigest: preview.targetDigest,
				idempotencyKey: applyIdempotencyKey
			});
			notice = `전적과 MMR 이력을 포함한 ${formatCount(result.summary.totalRows)}개 행을 반영했습니다.${
				result.remappedSourceGuild ? ` 원본 길드 데이터를 ${guildName}에 재매핑했습니다.` : ''
			}${
				result.usedTargetActiveSeason
					? ` 백업의 활성 시즌 데이터는 ${currentSeasonName}에 연결했습니다.`
					: ''
			}${
				result.rebasedDisplayNumbers
					? ' 기존 기록과 겹치지 않도록 내전 표시 번호를 이어 붙였습니다.'
					: ''
			}`;
			applyDialogOpen = false;
			confirmationValue = '';
			selectedArchive = null;
			selectedFileName = null;
			preview = null;
			previewClock = Date.now();
		} catch (cause) {
			importError = errorText(cause, '전체 데이터 가져오기를 적용하지 못했습니다.');
		} finally {
			applying = false;
		}
	}

	function errorText(cause: unknown, fallback: string): string {
		if (cause instanceof GuildDataTransferApiError) {
			return `${cause.message}${cause.requestId ? ` (요청 ID: ${cause.requestId})` : ''}`;
		}
		return cause instanceof Error && cause.message ? cause.message : fallback;
	}

	function formatDateTime(value: string): string {
		return new Intl.DateTimeFormat('ko-KR', {
			timeZone: 'Asia/Seoul',
			year: 'numeric',
			month: '2-digit',
			day: '2-digit',
			hour: '2-digit',
			minute: '2-digit',
			hourCycle: 'h23'
		}).format(new Date(value));
	}

	function formatCount(value: number): string {
		return value.toLocaleString('ko-KR');
	}

	function selectedRowCount(archive: GuildFullSnapshotArchive): number {
		return Object.values(archive.tables).reduce((total, rows) => total + rows.length, 0);
	}
</script>

<Dialog.Root bind:open={applyDialogOpen}>
	<div class="grid gap-4">
		<section
			class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
		>
			<header
				class="flex min-h-14 flex-wrap items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
			>
				<div class="flex min-w-0 items-center gap-3">
					<span
						class="grid size-8 shrink-0 place-items-center border border-cyan-400/20 bg-cyan-400/8 text-cyan-200"
					>
						<DatabaseBackup size={16} aria-hidden="true" />
					</span>
					<div class="min-w-0">
						<h2 class="truncate text-sm font-semibold text-slate-50">전체 데이터 내보내기</h2>
						<p class="text-xs text-slate-500">멤버·시즌·전적·픽/밴·경매·MMR 원장·감사 기록</p>
					</div>
				</div>
				<button
					type="button"
					disabled={exporting}
					onclick={() => void exportArchive()}
					class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/15 disabled:cursor-not-allowed disabled:opacity-35"
				>
					{#if exporting}
						<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 준비 중
					{:else}
						<Download size={14} aria-hidden="true" /> 전체 JSON 내보내기
					{/if}
				</button>
			</header>

			{#if exportError}
				<p
					role="alert"
					class="border-b border-rose-300/15 bg-rose-400/5 px-4 py-2.5 text-xs leading-5 font-semibold text-rose-200"
				>
					{exportError}
				</p>
			{/if}

			<div
				class="grid divide-y divide-[var(--color-app-border-muted)] sm:grid-cols-2 sm:divide-x sm:divide-y-0"
			>
				<div class="px-4 py-3">
					<span class="block text-[10px] font-semibold text-slate-600">현재 활성 멤버</span>
					<strong class="mt-1 block text-sm text-slate-100">{activeMemberCount}명</strong>
				</div>
				<div class="px-4 py-3">
					<span class="block text-[10px] font-semibold text-slate-600">현재 시즌</span>
					<strong class="mt-1 block truncate text-sm text-slate-100">{currentSeasonName}</strong>
				</div>
			</div>
			<p
				class="border-t border-[var(--color-app-border-muted)] px-4 py-2.5 text-xs leading-5 text-slate-500"
			>
				세션·OAuth 비밀값·Discord 동기화 캐시·Riot 조회 캐시는 제외합니다. 파일에는 개인정보와 운영
				이력이 포함되므로 안전하게 보관하세요.
			</p>
		</section>

		<section
			class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
		>
			<header
				class="flex min-h-14 flex-wrap items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
			>
				<div class="flex min-w-0 items-center gap-3">
					<span
						class="grid size-8 shrink-0 place-items-center border border-cyan-400/20 bg-cyan-400/8 text-cyan-200"
						><Upload size={16} aria-hidden="true" /></span
					>
					<div class="min-w-0">
						<h2 class="truncate text-sm font-semibold text-slate-50">전체 데이터 가져오기</h2>
						<p class="text-xs text-slate-500">
							길드 ID가 달라도 적용 가능한 GUILD_FULL_SNAPSHOT v1 JSON
						</p>
					</div>
				</div>
				<button
					type="button"
					disabled={previewing || applying}
					onclick={selectArchiveFile}
					class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-300 hover:bg-[var(--color-app-hover)] disabled:cursor-not-allowed disabled:opacity-35"
				>
					<FileUp size={14} aria-hidden="true" /> 파일 선택
				</button>
				<input
					bind:this={archiveInput}
					type="file"
					accept="application/json,.json"
					class="sr-only"
					aria-label="가져올 MonkeyBot 전체 데이터 백업 JSON 파일"
					onchange={(event) => void loadArchiveFile(event)}
				/>
			</header>

			<div
				class="border-b border-amber-300/15 bg-amber-400/5 px-4 py-2.5 text-xs leading-5 text-amber-100/80"
			>
				현재 활성 시즌에 전적·MMR·진행 데이터가 있으면 적용을 차단합니다. 닫힌 시즌 이력은 보존하며,
				미리보기는 PostgreSQL 제약을 실제로 검사한 뒤 모두 롤백합니다.
			</div>

			{#if importError}
				<p
					role="alert"
					class="border-b border-rose-300/15 bg-rose-400/5 px-4 py-2.5 text-xs leading-5 font-semibold text-rose-200"
				>
					{importError}
				</p>
			{/if}

			<div class="divide-y divide-[var(--color-app-border-muted)]">
				<div class="flex flex-wrap items-center justify-between gap-3 px-4 py-3">
					<div class="flex min-w-0 items-center gap-2">
						<FileJson size={16} class="shrink-0 text-slate-500" aria-hidden="true" />
						<div class="min-w-0">
							<strong class="block truncate text-xs text-slate-200"
								>{selectedFileName ?? '선택한 파일 없음'}</strong
							>
							{#if selectedArchive}
								<span class="mt-0.5 block truncate text-[11px] text-slate-500"
									>{selectedArchive.source.guildName} · {formatCount(
										selectedRowCount(selectedArchive)
									)}개 행</span
								>
							{:else}
								<span class="mt-0.5 block text-[11px] text-slate-600">최대 16 MiB JSON 파일</span>
							{/if}
						</div>
					</div>
					<button
						type="button"
						disabled={!selectedArchive || previewing || applying}
						onclick={() => void previewImport()}
						class="inline-flex h-9 shrink-0 items-center justify-center gap-1.5 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/15 disabled:cursor-not-allowed disabled:opacity-35"
					>
						{#if previewing}
							<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 검증 중
						{:else}
							<ShieldCheck size={14} aria-hidden="true" /> 안전성 검증
						{/if}
					</button>
				</div>

				{#if preview}
					<div class="grid gap-4 px-4 py-4">
						<div class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
							<div class="min-w-0">
								<span class="block text-[10px] font-semibold text-slate-600">검증한 원본</span>
								<strong class="mt-1 block truncate text-sm text-slate-100"
									>{preview.source.guildName}</strong
								>
								<p class="mt-1 text-xs text-slate-500">
									{formatDateTime(preview.expiresAt)}까지 적용할 수 있습니다.
								</p>
							</div>
							{#if hasPreviewConflicts}
								<span class="inline-flex items-center gap-1 text-xs font-semibold text-amber-200"
									><AlertTriangle size={14} aria-hidden="true" /> 충돌 {preview.conflicts
										.length}건</span
								>
							{:else if isPreviewExpired}
								<span class="text-xs font-semibold text-amber-200">검증 결과 만료</span>
							{:else}
								<span class="inline-flex items-center gap-1 text-xs font-semibold text-emerald-300"
									><Check size={14} aria-hidden="true" /> 제약 검증 통과</span
								>
							{/if}
						</div>

						<dl
							class="grid grid-cols-2 gap-px border border-[var(--color-app-border-muted)] bg-[var(--color-app-border-muted)] text-xs sm:grid-cols-3 lg:grid-cols-6"
						>
							{#each [['전체 행', preview.summary.totalRows], ['멤버', preview.summary.members], ['일반 내전', preview.summary.series], ['경기 전적', preview.summary.games], ['경매 대회', preview.summary.auctionTournaments], ['MMR 원장', preview.summary.mmrLedgerEntries]] as item (item[0])}
								<div class="min-w-0 bg-[var(--color-app-surface)] px-3 py-2.5">
									<dt class="text-[10px] font-semibold text-slate-600">{item[0]}</dt>
									<dd class="mt-1 text-sm font-semibold text-slate-200">
										{formatCount(Number(item[1]))}
									</dd>
								</div>
							{/each}
						</dl>

						{#if preview.remapsSourceGuild}
							<div
								class="flex items-start gap-2 border border-amber-300/20 bg-amber-400/5 px-3 py-2.5 text-xs text-amber-100"
							>
								<AlertTriangle size={14} class="mt-0.5 shrink-0" aria-hidden="true" />
								<p>
									원본 {preview.source.guildName} ({preview.source.guildId})의 길드 데이터를 현재
									대상
									{guildName} ({guildId})로 재매핑합니다. 전적·MMR·대회 기록은 보존하고, 원본 길드의
									모집글·결과 카드 채널/메시지 연결은 제거합니다. 현재 Discord 설정과 역할/채널
									카탈로그는 덮어쓰지 않습니다.
								</p>
							</div>
						{/if}

						{#if preview.usesTargetActiveSeason && !hasPreviewConflicts}
							<div
								class="flex items-start gap-2 border border-amber-300/20 bg-amber-400/5 px-3 py-2.5 text-xs text-amber-100"
							>
								<AlertTriangle size={14} class="mt-0.5 shrink-0" aria-hidden="true" />
								<p>
									백업의 활성 시즌 데이터는 현재 활성 시즌인 {currentSeasonName}에 연결합니다. 현재
									시즌 자체는 삭제하거나 새로 만들지 않습니다.
								</p>
							</div>
						{/if}

						{#if preview.rebasesDisplayNumbers && !hasPreviewConflicts}
							<div
								class="flex items-start gap-2 border border-amber-300/20 bg-amber-400/5 px-3 py-2.5 text-xs text-amber-100"
							>
								<AlertTriangle size={14} class="mt-0.5 shrink-0" aria-hidden="true" />
								<p>
									기존 닫힌 시즌의 내전 번호와 겹치지 않도록 가져오는 모집·일반전·경매 번호를 현재
									번호 뒤로 이어 붙입니다.
								</p>
							</div>
						{/if}

						{#if hasPreviewConflicts}
							<div class="divide-y divide-amber-300/10 border border-amber-300/20">
								{#each preview.conflicts as conflict (`${conflict.reason}:${conflict.table ?? 'target'}`)}
									<div
										class="grid gap-1 px-3 py-2.5 text-xs sm:grid-cols-[180px_minmax(0,1fr)_auto] sm:gap-3"
									>
										<span class="font-mono text-amber-200">{conflict.reason}</span>
										<span class="text-slate-400">{conflict.message}</span>
										<span class="text-slate-500">{formatCount(conflict.rowCount)}개 행</span>
									</div>
								{/each}
							</div>
						{/if}

						<div class="flex justify-end">
							<button
								type="button"
								disabled={!canOpenApplyDialog || applying}
								onclick={openApplyDialog}
								class="inline-flex h-9 items-center justify-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 hover:bg-amber-400/16 disabled:cursor-not-allowed disabled:opacity-35"
							>
								<Upload size={14} aria-hidden="true" /> 전체 데이터 적용
							</button>
						</div>
					</div>
				{/if}
			</div>
		</section>

		{#if notice}
			<p
				role="status"
				class="flex items-center gap-1.5 border border-emerald-300/15 bg-emerald-400/5 px-3 py-2.5 text-xs font-semibold text-emerald-300"
			>
				<Check size={14} aria-hidden="true" />{notice}
			</p>
		{/if}
	</div>

	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/60 backdrop-blur-sm" />
		<Dialog.Content
			onInteractOutside={preventCloseWhileApplying}
			onEscapeKeydown={preventCloseWhileApplying}
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-md -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50"
					>전체 데이터 가져오기 확인</Dialog.Title
				>
				<Dialog.Close
					disabled={applying}
					class="grid size-8 place-items-center text-slate-400 hover:text-slate-100 disabled:opacity-35"
					aria-label="전체 데이터 가져오기 확인 창 닫기"
					><X size={16} aria-hidden="true" /></Dialog.Close
				>
			</header>
			{#if preview}
				<div class="grid gap-4 p-4">
					<div class="border border-amber-300/20 bg-amber-400/6 px-3 py-3">
						<strong class="flex items-center gap-1.5 text-sm text-amber-100"
							><History size={15} aria-hidden="true" />{formatCount(preview.summary.totalRows)}개
							행을 가져옵니다.</strong
						>
						<p class="mt-1 text-xs leading-5 text-slate-400">
							일반전 {formatCount(preview.summary.games)}경기, 경매 {formatCount(
								preview.summary.auctionTournaments
							)}개 대회, MMR 원장 {formatCount(preview.summary.mmrLedgerEntries)}건을 포함합니다.
							현재 길드 설정과 Discord 역할 동기화 값은 덮어쓰지 않습니다.
						</p>
						{#if preview.remapsSourceGuild}
							<p class="mt-2 text-xs leading-5 font-semibold text-amber-100">
								{preview.source.guildId} → {guildId} 길드 ID 재매핑과 원본 Discord 게시물 연결 제거를
								함께 적용합니다.
							</p>
						{/if}
					</div>
					<label class="grid gap-1.5 text-xs font-semibold text-slate-500">
						확인을 위해 길드 이름 입력
						<input
							bind:value={confirmationValue}
							disabled={applying}
							placeholder={guildName}
							aria-label="전체 데이터 가져오기 확인 길드 이름"
							class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none placeholder:text-slate-600 disabled:opacity-55"
						/>
					</label>
				</div>
				<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] p-3">
					<Dialog.Close
						disabled={applying}
						class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
						>취소</Dialog.Close
					>
					<button
						type="button"
						disabled={!canApply}
						onclick={() => void applyImport()}
						class="inline-flex h-9 items-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 hover:bg-amber-400/16 disabled:cursor-not-allowed disabled:opacity-35"
					>
						{#if applying}
							<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 적용 중
						{:else}
							<Upload size={14} aria-hidden="true" /> 전체 데이터 적용
						{/if}
					</button>
				</footer>
			{/if}
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
