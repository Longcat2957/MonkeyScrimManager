<script lang="ts">
	import { Check, RotateCcw, Save, Settings } from 'lucide-svelte';
	import { untrack } from 'svelte';
	import type {
		GuildAdminChannelOption,
		GuildAdminSettings,
		GuildAdminSettingsSnapshot
	} from '$lib/contracts/workflow';
	import type { GuildSettingsUpdateCommand } from '$lib/realtime/admin';

	type ChannelSettingKey = 'recruitmentChannelId' | 'entryChannelId' | 'logChannelId';

	interface Props {
		snapshot: GuildAdminSettingsSnapshot;
		onSave: (command: GuildSettingsUpdateCommand) => Promise<GuildAdminSettingsSnapshot>;
	}

	let { snapshot, onSave }: Props = $props();
	const initialSnapshot = untrack(() => snapshot);
	let appliedRevision = $state(initialSnapshot.revision);
	let savedSettings = $state<GuildAdminSettings>({ ...initialSnapshot.settings });
	let draftSettings = $state<GuildAdminSettings>({ ...initialSnapshot.settings });
	let savedNotice = $state(false);
	let submitting = $state(false);
	let saveError = $state<string | null>(null);

	const dirty = $derived(!settingsEqual(savedSettings, draftSettings));
	const missingOperatorRole = $derived(
		!snapshot.roles.some((role) => role.id === draftSettings.operatorRoleId)
	);

	$effect(() => {
		if (snapshot.revision === appliedRevision) return;
		appliedRevision = snapshot.revision;
		savedSettings = { ...snapshot.settings };
		draftSettings = { ...snapshot.settings };
		savedNotice = false;
	});

	function settingsEqual(left: GuildAdminSettings, right: GuildAdminSettings): boolean {
		return (
			left.displayName === right.displayName &&
			left.operatorRoleId === right.operatorRoleId &&
			left.recruitmentChannelId === right.recruitmentChannelId &&
			left.entryChannelId === right.entryChannelId &&
			left.logChannelId === right.logChannelId &&
			left.isExternallyPublic === right.isExternallyPublic
		);
	}

	function clearFeedback() {
		savedNotice = false;
		saveError = null;
	}

	function resetDraft() {
		draftSettings = { ...savedSettings };
		clearFeedback();
	}

	async function save() {
		if (!dirty || submitting) return;
		submitting = true;
		clearFeedback();
		try {
			const authoritative = await onSave({
				kind: 'UPDATE_GUILD_SETTINGS',
				settings: { ...draftSettings },
				expectedRevision: appliedRevision
			});
			appliedRevision = authoritative.revision;
			savedSettings = { ...authoritative.settings };
			draftSettings = { ...authoritative.settings };
			savedNotice = true;
		} catch (error) {
			saveError = error instanceof Error ? error.message : '길드 설정 저장 중 오류가 발생했습니다.';
		} finally {
			submitting = false;
		}
	}

	function updateChannel(key: ChannelSettingKey, value: string) {
		draftSettings = { ...draftSettings, [key]: value || null };
		clearFeedback();
	}

	function toggleExternalVisibility() {
		if (submitting) return;
		draftSettings = {
			...draftSettings,
			isExternallyPublic: !draftSettings.isExternallyPublic
		};
		clearFeedback();
	}

	function isMissingChannel(channelId: string | null): channelId is string {
		return Boolean(channelId) && !snapshot.channels.some((channel) => channel.id === channelId);
	}

	function channelLabel(channel: GuildAdminChannelOption): string {
		return `${channel.parentName ? `${channel.parentName} / ` : ''}#${channel.name}`;
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
				<Settings size={18} class="shrink-0 text-cyan-200" aria-hidden="true" />
				<div class="min-w-0">
					<h2 class="truncate text-sm font-semibold text-slate-50">길드 설정</h2>
					<p class="text-xs text-slate-500">Discord 연동과 공개 범위</p>
				</div>
			</div>
			<div class="flex items-center gap-2">
				{#if savedNotice}
					<span
						class="inline-flex items-center gap-1 text-xs font-semibold text-emerald-300"
						aria-live="polite"
					>
						<Check size={14} aria-hidden="true" /> 저장됨
					</span>
				{/if}
				<button
					type="button"
					disabled={!dirty || submitting}
					onclick={resetDraft}
					aria-label="길드 설정 변경 취소"
					title="변경 취소"
					class="grid size-8 place-items-center border border-[var(--color-app-border)] text-slate-400 enabled:hover:text-slate-100 disabled:opacity-35"
				>
					<RotateCcw size={14} aria-hidden="true" />
				</button>
				<button
					type="button"
					disabled={!dirty || submitting}
					onclick={save}
					class="inline-flex h-8 items-center gap-1.5 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 disabled:opacity-35"
				>
					<Save size={14} aria-hidden="true" />
					{submitting ? '저장 중' : '저장'}
				</button>
			</div>
		</header>
		{#if saveError}
			<div
				class="border-b border-rose-400/20 bg-rose-400/5 px-4 py-2.5 text-xs leading-5 font-semibold break-words text-rose-300"
				aria-live="assertive"
			>
				{saveError}
			</div>
		{/if}

		<div class="divide-y divide-[var(--color-app-border-muted)]">
			<div
				class="grid gap-3 px-4 py-4 sm:grid-cols-[minmax(0,1fr)_minmax(16rem,22rem)] sm:items-center"
			>
				<div>
					<strong class="block text-sm text-slate-100">길드 표시 이름</strong>
					<p class="mt-1 text-xs text-slate-500">웹과 Bot 메시지에 표시되는 이름</p>
				</div>
				<input
					bind:value={draftSettings.displayName}
					disabled={submitting}
					maxlength="100"
					oninput={clearFeedback}
					class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45 disabled:opacity-55"
				/>
			</div>

			<div
				class="grid gap-3 px-4 py-4 sm:grid-cols-[minmax(0,1fr)_minmax(16rem,22rem)] sm:items-center"
			>
				<div>
					<strong class="block text-sm text-slate-100">운영자 역할</strong>
					<p class="mt-1 text-xs text-slate-500">Discord 역할을 기준으로 관리 권한 확인</p>
				</div>
				<select
					bind:value={draftSettings.operatorRoleId}
					disabled={submitting}
					onchange={clearFeedback}
					class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none disabled:opacity-55"
				>
					{#if missingOperatorRole}
						<option value={draftSettings.operatorRoleId}>
							현재 설정 · Discord에서 확인 불가 ({draftSettings.operatorRoleId})
						</option>
					{/if}
					{#each snapshot.roles as role (role.id)}
						<option value={role.id}>{role.name}</option>
					{/each}
				</select>
			</div>

			<div
				class="grid gap-3 px-4 py-4 sm:grid-cols-[minmax(0,1fr)_minmax(16rem,22rem)] sm:items-center"
			>
				<div>
					<strong class="block text-sm text-slate-100">모집 채널</strong>
					<p class="mt-1 text-xs text-slate-500">내전과 경매 모집 메시지를 게시할 채널</p>
				</div>
				<select
					value={draftSettings.recruitmentChannelId ?? ''}
					disabled={submitting}
					onchange={(event) => updateChannel('recruitmentChannelId', event.currentTarget.value)}
					class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none disabled:opacity-55"
				>
					<option value="">설정 안 함</option>
					{#if isMissingChannel(draftSettings.recruitmentChannelId)}
						<option value={draftSettings.recruitmentChannelId}>
							현재 설정 · Discord에서 확인 불가 ({draftSettings.recruitmentChannelId})
						</option>
					{/if}
					{#each snapshot.channels as channel (channel.id)}
						<option value={channel.id}>{channelLabel(channel)}</option>
					{/each}
				</select>
			</div>

			<div
				class="grid gap-3 px-4 py-4 sm:grid-cols-[minmax(0,1fr)_minmax(16rem,22rem)] sm:items-center"
			>
				<div>
					<strong class="block text-sm text-slate-100">엔트리 채널</strong>
					<p class="mt-1 text-xs text-slate-500">밸런스 결과와 확정 엔트리를 게시할 채널</p>
				</div>
				<select
					value={draftSettings.entryChannelId ?? ''}
					disabled={submitting}
					onchange={(event) => updateChannel('entryChannelId', event.currentTarget.value)}
					class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none disabled:opacity-55"
				>
					<option value="">설정 안 함</option>
					{#if isMissingChannel(draftSettings.entryChannelId)}
						<option value={draftSettings.entryChannelId}>
							현재 설정 · Discord에서 확인 불가 ({draftSettings.entryChannelId})
						</option>
					{/if}
					{#each snapshot.channels as channel (channel.id)}
						<option value={channel.id}>{channelLabel(channel)}</option>
					{/each}
				</select>
			</div>

			<div
				class="grid gap-3 px-4 py-4 sm:grid-cols-[minmax(0,1fr)_minmax(16rem,22rem)] sm:items-center"
			>
				<div>
					<strong class="block text-sm text-slate-100">운영 로그 채널</strong>
					<p class="mt-1 text-xs text-slate-500">관리 작업과 오류 알림을 전송할 채널</p>
				</div>
				<select
					value={draftSettings.logChannelId ?? ''}
					disabled={submitting}
					onchange={(event) => updateChannel('logChannelId', event.currentTarget.value)}
					class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none disabled:opacity-55"
				>
					<option value="">설정 안 함</option>
					{#if isMissingChannel(draftSettings.logChannelId)}
						<option value={draftSettings.logChannelId}>
							현재 설정 · Discord에서 확인 불가 ({draftSettings.logChannelId})
						</option>
					{/if}
					{#each snapshot.channels as channel (channel.id)}
						<option value={channel.id}>{channelLabel(channel)}</option>
					{/each}
				</select>
			</div>

			<div class="grid gap-3 px-4 py-4 sm:grid-cols-[minmax(0,1fr)_52px] sm:items-center">
				<div>
					<strong class="block text-sm text-slate-100">외부 전적 공개</strong>
					<p class="mt-1 text-xs leading-5 text-slate-500">
						길드 외부 사용자의 공개 전적 조회 허용
					</p>
				</div>
				<button
					type="button"
					role="switch"
					aria-label="외부 전적 공개"
					aria-checked={draftSettings.isExternallyPublic}
					disabled={submitting}
					onclick={toggleExternalVisibility}
					class={`relative h-6 w-11 rounded-full border transition disabled:opacity-55 ${
						draftSettings.isExternallyPublic
							? 'border-cyan-300/50 bg-cyan-400/25'
							: 'border-slate-600 bg-slate-800'
					}`}
				>
					<span
						class={`absolute top-0.5 size-4.5 rounded-full bg-slate-100 transition ${
							draftSettings.isExternallyPublic ? 'left-5.5' : 'left-0.5'
						}`}
					></span>
				</button>
			</div>
		</div>
	</section>
</div>
