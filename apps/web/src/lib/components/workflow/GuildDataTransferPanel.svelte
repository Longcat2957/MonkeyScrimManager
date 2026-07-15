<script lang="ts">
	import { Dialog } from 'bits-ui';
	import {
		AlertTriangle,
		Check,
		Download,
		FileJson,
		FileUp,
		LoaderCircle,
		ShieldCheck,
		Upload,
		X
	} from 'lucide-svelte';
	import {
		applyGuildMemberSeasonMmrSeedImport,
		exportGuildMemberSeasonMmrSeed,
		GuildDataTransferApiError,
		previewGuildMemberSeasonMmrSeedImport,
		serializeGuildMemberSeasonMmrSeedArchive
	} from '$lib/api/guildDataTransfer';
	import {
		memberSeasonMmrSeedMaxMembers,
		memberSeasonMmrSeedArchiveSchema,
		type GuildDataTransferPreview,
		type MemberSeasonMmrSeedArchive
	} from '$lib/contracts/guildDataTransfer';

	interface Props {
		guildId: string;
		guildName: string;
		activeMemberCount: number;
		currentSeasonName: string;
	}

	let { guildId, guildName, activeMemberCount, currentSeasonName }: Props = $props();

	let archiveInput = $state<globalThis.HTMLInputElement>();
	let selectedFileName = $state<string | null>(null);
	let selectedArchive = $state<MemberSeasonMmrSeedArchive | null>(null);
	let preview = $state<GuildDataTransferPreview | null>(null);
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
	const hasChanges = $derived((preview?.summary.changes ?? 0) > 0);
	const isPreviewExpired = $derived(
		Boolean(preview) &&
			(!Number.isFinite(Date.parse(preview?.expiresAt ?? '')) ||
				Date.parse(preview?.expiresAt ?? '') <= previewClock)
	);
	const canOpenApplyDialog = $derived(
		Boolean(preview) && !hasPreviewConflicts && hasChanges && !isPreviewExpired
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
			const { archive, fileName } = await exportGuildMemberSeasonMmrSeed(guildId);
			const blob = new Blob([serializeGuildMemberSeasonMmrSeedArchive(archive)], {
				type: 'application/json;charset=utf-8'
			});
			const url = URL.createObjectURL(blob);
			const anchor = document.createElement('a');
			anchor.href = url;
			anchor.download = fileName;
			anchor.style.display = 'none';
			document.body.append(anchor);
			anchor.click();
			anchor.remove();
			URL.revokeObjectURL(url);
			notice = 'MMR 백업 파일을 준비했습니다.';
		} catch (cause) {
			exportError = errorText(cause, 'MMR 백업을 만들지 못했습니다.');
		} finally {
			exporting = false;
		}
	}

	function selectArchiveFile() {
		archiveInput?.click();
	}

	async function loadArchiveFile(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		if (!file) return;

		selectedFileName = file.name;
		selectedArchive = null;
		preview = null;
		previewClock = Date.now();
		importError = '';
		notice = '';
		applyDialogOpen = false;
		confirmationValue = '';
		applyIntent = '';
		applyIdempotencyKey = '';

		try {
			const payload: unknown = JSON.parse(await file.text());
			if (
				typeof payload === 'object' &&
				payload !== null &&
				Array.isArray((payload as { members?: unknown }).members) &&
				(payload as { members: unknown[] }).members.length > memberSeasonMmrSeedMaxMembers
			) {
				throw new Error(
					`MMR 백업 파일은 최대 ${memberSeasonMmrSeedMaxMembers.toLocaleString()}명까지 지원합니다.`
				);
			}
			const parsed = memberSeasonMmrSeedArchiveSchema.safeParse(payload);
			if (!parsed.success) {
				throw new Error('선택한 파일은 지원되는 MonkeyBot 멤버·시즌 MMR 백업 JSON이 아닙니다.');
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
		preview = null;
		previewClock = Date.now();
		applyDialogOpen = false;
		confirmationValue = '';
		applyIntent = '';
		applyIdempotencyKey = '';
		try {
			preview = await previewGuildMemberSeasonMmrSeedImport(guildId, selectedArchive);
		} catch (cause) {
			importError = errorText(cause, '가져오기 검증을 완료하지 못했습니다.');
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

	function preventCloseWhileApplying(event: Event) {
		if (applying) event.preventDefault();
	}

	async function applyImport() {
		if (!preview || !canApply) return;
		const intent = JSON.stringify([preview.importId, preview.digest, preview.targetRevision]);
		if (intent !== applyIntent) {
			applyIntent = intent;
			applyIdempotencyKey = globalThis.crypto.randomUUID();
		}

		applying = true;
		importError = '';
		notice = '';
		try {
			const result = await applyGuildMemberSeasonMmrSeedImport(guildId, {
				importId: preview.importId,
				expectedDigest: preview.digest,
				expectedTargetRevision: preview.targetRevision,
				idempotencyKey: applyIdempotencyKey
			});
			notice = `${result.summary.changes}개 레인 MMR을 반영했습니다.`;
			applyDialogOpen = false;
			confirmationValue = '';
			selectedArchive = null;
			selectedFileName = null;
			preview = null;
			previewClock = Date.now();
		} catch (cause) {
			importError = errorText(cause, '가져오기를 적용하지 못했습니다.');
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
						<Download size={16} aria-hidden="true" />
					</span>
					<div class="min-w-0">
						<h2 class="truncate text-sm font-semibold text-slate-50">데이터 내보내기</h2>
						<p class="text-xs text-slate-500">현재 활성 멤버의 시즌 레인 MMR 백업</p>
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
						<Download size={14} aria-hidden="true" /> JSON 내보내기
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
					<span class="block text-[10px] font-semibold text-slate-600">내보낼 멤버</span>
					<strong class="mt-1 block text-sm text-slate-100">{activeMemberCount}명</strong>
				</div>
				<div class="px-4 py-3">
					<span class="block text-[10px] font-semibold text-slate-600">기준 시즌</span>
					<strong class="mt-1 block truncate text-sm text-slate-100">{currentSeasonName}</strong>
				</div>
			</div>
			<p
				class="border-t border-[var(--color-app-border-muted)] px-4 py-2.5 text-xs leading-5 text-slate-500"
			>
				암호화하지 않은 읽고 편집할 수 있는 JSON 파일입니다.
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
					>
						<Upload size={16} aria-hidden="true" />
					</span>
					<div class="min-w-0">
						<h2 class="truncate text-sm font-semibold text-slate-50">데이터 가져오기</h2>
						<p class="text-xs text-slate-500">
							MonkeyBot 멤버·시즌 MMR 백업 JSON만 지원 · 최대 1,000명
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
					aria-label="가져올 MonkeyBot MMR 백업 JSON 파일"
					onchange={(event) => void loadArchiveFile(event)}
				/>
			</header>

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
							<strong class="block truncate text-xs text-slate-200">
								{selectedFileName ?? '선택한 파일 없음'}
							</strong>
							{#if selectedArchive}
								<span class="mt-0.5 block truncate text-[11px] text-slate-500">
									{selectedArchive.source.guildName} · {selectedArchive.source.season.name} · {selectedArchive
										.members.length}명
								</span>
							{:else}
								<span class="mt-0.5 block text-[11px] text-slate-600"
									>JSON 파일을 선택해 주세요.</span
								>
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
							<ShieldCheck size={14} aria-hidden="true" /> 검증
						{/if}
					</button>
				</div>

				{#if preview}
					<div class="grid gap-4 px-4 py-4">
						<div class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_auto] sm:items-end">
							<div class="min-w-0">
								<span class="block text-[10px] font-semibold text-slate-600">가져올 원본</span>
								<strong class="mt-1 block truncate text-sm text-slate-100">
									{preview.source.guildName} · {preview.source.season.name}
								</strong>
								<p class="mt-1 text-xs text-slate-500">
									{formatDateTime(preview.expiresAt)}까지 이 검증 결과를 적용할 수 있습니다.
								</p>
							</div>
							{#if hasPreviewConflicts}
								<span class="inline-flex items-center gap-1 text-xs font-semibold text-amber-200">
									<AlertTriangle size={14} aria-hidden="true" /> 충돌 {preview.conflicts.length}건
								</span>
							{:else if isPreviewExpired}
								<span class="text-xs font-semibold text-amber-200">검증 결과 만료</span>
							{:else if hasChanges}
								<span class="inline-flex items-center gap-1 text-xs font-semibold text-emerald-300">
									<Check size={14} aria-hidden="true" /> 적용 가능
								</span>
							{:else}
								<span class="text-xs font-semibold text-slate-500">변경할 MMR 없음</span>
							{/if}
						</div>

						<dl
							class="grid grid-cols-2 gap-px border border-[var(--color-app-border-muted)] bg-[var(--color-app-border-muted)] text-xs sm:grid-cols-4"
						>
							<div class="min-w-0 bg-[var(--color-app-surface)] px-3 py-2.5">
								<dt class="text-[10px] font-semibold text-slate-600">파일 멤버</dt>
								<dd class="mt-1 text-sm font-semibold text-slate-200">
									{preview.summary.archiveMembers}
								</dd>
							</div>
							<div class="min-w-0 bg-[var(--color-app-surface)] px-3 py-2.5">
								<dt class="text-[10px] font-semibold text-slate-600">매칭 멤버</dt>
								<dd class="mt-1 text-sm font-semibold text-slate-200">
									{preview.summary.matchedMembers}
								</dd>
							</div>
							<div class="min-w-0 bg-[var(--color-app-surface)] px-3 py-2.5">
								<dt class="text-[10px] font-semibold text-slate-600">레인 항목</dt>
								<dd class="mt-1 text-sm font-semibold text-slate-200">
									{preview.summary.laneEntries}
								</dd>
							</div>
							<div class="min-w-0 bg-[var(--color-app-surface)] px-3 py-2.5">
								<dt class="text-[10px] font-semibold text-slate-600">변경 예정</dt>
								<dd class="mt-1 text-sm font-semibold text-cyan-100">{preview.summary.changes}</dd>
							</div>
						</dl>

						{#if hasPreviewConflicts}
							<div class="overflow-x-auto border border-amber-300/20">
								<div class="min-w-[620px]">
									<div
										class="grid grid-cols-[130px_150px_140px_minmax(180px,1fr)] gap-x-3 border-b border-amber-300/15 bg-amber-400/5 px-3 py-2 text-[10px] font-semibold text-amber-200/75"
									>
										<span>Discord ID</span><span>멤버</span><span>사유</span><span>상세</span>
									</div>
									<div class="divide-y divide-amber-300/10">
										{#each preview.conflicts as conflict (`${conflict.discordId}:${conflict.reason}`)}
											<div
												class="grid grid-cols-[130px_150px_140px_minmax(180px,1fr)] gap-x-3 px-3 py-2.5 text-xs"
											>
												<span class="truncate font-mono text-slate-500">{conflict.discordId}</span>
												<span class="truncate text-slate-300">{conflict.displayName}</span>
												<span class="truncate text-amber-200">{conflict.reason}</span>
												<span class="break-words text-slate-400">{conflict.message}</span>
											</div>
										{/each}
									</div>
								</div>
							</div>
						{:else if !hasChanges}
							<p
								class="border border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-3 py-2.5 text-xs leading-5 text-slate-500"
							>
								현재 길드의 MMR과 동일합니다. 적용할 변경이 없습니다.
							</p>
						{/if}

						<div class="flex justify-end">
							<button
								type="button"
								disabled={!canOpenApplyDialog || applying}
								onclick={openApplyDialog}
								class="inline-flex h-9 items-center justify-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 hover:bg-amber-400/16 disabled:cursor-not-allowed disabled:opacity-35"
							>
								<Upload size={14} aria-hidden="true" /> 가져오기 적용
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
				<Check size={14} aria-hidden="true" />
				{notice}
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
				<Dialog.Title class="text-sm font-semibold text-slate-50">MMR 가져오기 확인</Dialog.Title>
				<Dialog.Close
					disabled={applying}
					class="grid size-8 place-items-center text-slate-400 hover:text-slate-100 disabled:opacity-35"
					aria-label="MMR 가져오기 확인 창 닫기"
				>
					<X size={16} aria-hidden="true" />
				</Dialog.Close>
			</header>
			{#if preview}
				<div class="grid gap-4 p-4">
					<div class="border border-amber-300/20 bg-amber-400/6 px-3 py-3">
						<strong class="block text-sm text-amber-100">
							{preview.summary.changes}개 레인 MMR을 변경합니다.
						</strong>
						<p class="mt-1 text-xs leading-5 text-slate-400">
							대상 길드의 기존 활성 멤버만 Discord ID로 매칭합니다. 경기·전적·승패 수는 옮기지
							않습니다.
						</p>
					</div>
					<label class="grid gap-1.5 text-xs font-semibold text-slate-500">
						확인을 위해 길드 이름 입력
						<input
							bind:value={confirmationValue}
							disabled={applying}
							placeholder={guildName}
							aria-label="가져오기 확인 길드 이름"
							class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none placeholder:text-slate-600 disabled:opacity-55"
						/>
					</label>
				</div>
				<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] p-3">
					<Dialog.Close
						disabled={applying}
						class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400 disabled:opacity-35"
					>
						취소
					</Dialog.Close>
					<button
						type="button"
						disabled={!canApply}
						onclick={() => void applyImport()}
						class="inline-flex h-9 items-center gap-1.5 border border-amber-300/35 bg-amber-400/10 px-3 text-xs font-semibold text-amber-100 hover:bg-amber-400/16 disabled:cursor-not-allowed disabled:opacity-35"
					>
						{#if applying}
							<LoaderCircle size={14} class="animate-spin" aria-hidden="true" /> 적용 중
						{:else}
							<Upload size={14} aria-hidden="true" /> MMR 적용
						{/if}
					</button>
				</footer>
			{/if}
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
