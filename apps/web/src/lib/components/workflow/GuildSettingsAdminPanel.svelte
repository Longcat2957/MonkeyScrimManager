<script lang="ts">
	import { Check, Lock, RotateCcw, Save, Settings } from 'lucide-svelte';
	import { untrack } from 'svelte';
	import {
		mockDiscordChannels,
		mockDiscordRoles,
		mockGuildSeasons,
		type GuildAdminSettings
	} from '$lib/fixtures/guildAdmin';
	import type { GuildSettingsUpdateCommand } from '$lib/realtime/admin';

	interface Props {
		settings: GuildAdminSettings;
		revision: number;
		onSave: (command: GuildSettingsUpdateCommand) => void | Promise<void>;
	}

	let { settings, revision, onSave }: Props = $props();
	let savedSettings = $state<GuildAdminSettings>(untrack(() => ({ ...settings })));
	let draftSettings = $state<GuildAdminSettings>(untrack(() => ({ ...settings })));
	let savedNotice = $state(false);
	let submitting = $state(false);

	const dirty = $derived(JSON.stringify(savedSettings) !== JSON.stringify(draftSettings));
	const activeSeason = $derived(
		mockGuildSeasons.find((season) => season.id === draftSettings.seasonId) ?? mockGuildSeasons[0]
	);

	function resetDraft() {
		draftSettings = { ...savedSettings };
		savedNotice = false;
	}

	async function save() {
		if (!dirty || submitting) return;
		submitting = true;
		try {
			await onSave({
				kind: 'UPDATE_GUILD_SETTINGS',
				settings: { ...draftSettings },
				expectedRevision: revision
			});
			savedSettings = { ...draftSettings };
			savedNotice = true;
		} finally {
			submitting = false;
		}
	}

	function toggleExternalVisibility() {
		draftSettings = {
			...draftSettings,
			isExternallyPublic: !draftSettings.isExternallyPublic
		};
		savedNotice = false;
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
					<p class="text-xs text-slate-500">Discord 연동과 현재 시즌</p>
				</div>
			</div>
			<div class="flex items-center gap-2">
				{#if savedNotice}
					<span class="inline-flex items-center gap-1 text-xs font-semibold text-emerald-300">
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
					disabled={!dirty}
					onclick={save}
					class="inline-flex h-8 items-center gap-1.5 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 disabled:opacity-35"
				>
					<Save size={14} aria-hidden="true" />
					{submitting ? '저장 중' : '저장'}
				</button>
			</div>
		</header>

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
					oninput={() => (savedNotice = false)}
					class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
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
					onchange={() => (savedNotice = false)}
					class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none"
				>
					{#each mockDiscordRoles as role (role.id)}
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
					bind:value={draftSettings.recruitmentChannelId}
					onchange={() => (savedNotice = false)}
					class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none"
				>
					{#each mockDiscordChannels as channel (channel.id)}
						<option value={channel.id}>{channel.name}</option>
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
					bind:value={draftSettings.logChannelId}
					onchange={() => (savedNotice = false)}
					class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none"
				>
					{#each mockDiscordChannels as channel (channel.id)}
						<option value={channel.id}>{channel.name}</option>
					{/each}
				</select>
			</div>

			<div
				class="grid gap-3 px-4 py-4 sm:grid-cols-[minmax(0,1fr)_minmax(16rem,22rem)] sm:items-center"
			>
				<div>
					<strong class="block text-sm text-slate-100">현재 시즌</strong>
					<p class="mt-1 text-xs text-slate-500">전적과 길드 MMR이 집계되는 시즌</p>
				</div>
				<div
					class="flex h-10 items-center justify-between gap-3 border border-[var(--color-app-border)] bg-slate-950/20 px-3"
				>
					<span class="truncate text-sm font-semibold text-slate-200">
						{activeSeason?.name} · {activeSeason?.state}
					</span>
					<Lock size={14} class="shrink-0 text-slate-600" aria-label="읽기 전용" />
				</div>
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
					onclick={toggleExternalVisibility}
					class={`relative h-6 w-11 rounded-full border transition ${
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
