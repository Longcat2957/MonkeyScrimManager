<script lang="ts">
	import { Check, Save, ShieldCheck } from 'lucide-svelte';
	import { untrack } from 'svelte';
	import { updatePrivacySettings } from '$lib/api/privacy';
	import type { PrivacySettings } from '$lib/contracts/workflow';

	type PrivacyKey = keyof PrivacySettings;

	interface Props {
		initialSettings: PrivacySettings;
	}

	const settings: Array<{ key: PrivacyKey; title: string; description: string }> = [
		{
			key: 'publicProfile',
			title: '외부 프로필 공개',
			description: '같은 서버에 속하지 않은 사용자에게 기본 프로필을 표시합니다.'
		},
		{
			key: 'showRiotIds',
			title: '연결된 Riot ID 공개',
			description: '사용자 및 길드 프로필에 Discord 계정과 연결된 Riot ID를 표시합니다.'
		},
		{
			key: 'allowExternalRecordSearch',
			title: '외부 전적 검색 허용',
			description: '공개 서버에서 외부 사용자가 내 내전 전적을 검색할 수 있습니다.'
		}
	];

	let { initialSettings }: Props = $props();
	const startingSettings = untrack(() => ({ ...initialSettings }));
	let savedSettings = $state<PrivacySettings>({ ...startingSettings });
	let draftSettings = $state<PrivacySettings>({ ...startingSettings });
	let savedNotice = $state(false);
	let saving = $state(false);
	let error = $state<string | null>(null);

	const dirty = $derived(
		settings.some((setting) => draftSettings[setting.key] !== savedSettings[setting.key])
	);

	function toggle(key: PrivacyKey) {
		if (saving) return;
		draftSettings = { ...draftSettings, [key]: !draftSettings[key] };
		savedNotice = false;
		error = null;
	}

	async function save() {
		if (!dirty || saving) return;
		saving = true;
		savedNotice = false;
		error = null;
		try {
			const settings = await updatePrivacySettings({ ...draftSettings });
			savedSettings = { ...settings };
			draftSettings = { ...settings };
			savedNotice = true;
		} catch (saveError) {
			error =
				saveError instanceof Error
					? saveError.message
					: '개인정보 설정 저장 중 오류가 발생했습니다.';
		} finally {
			saving = false;
		}
	}
</script>

<section
	class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
>
	<header
		class="flex min-h-14 flex-wrap items-center justify-between gap-3 border-b border-[var(--color-app-border)] px-4 py-2"
	>
		<div class="flex min-w-0 items-center gap-3">
			<ShieldCheck size={18} class="shrink-0 text-cyan-200" aria-hidden="true" />
			<div class="min-w-0">
				<h2 class="truncate text-sm font-semibold text-slate-50">개인정보</h2>
				<p class="text-xs text-slate-500">외부 공개 범위</p>
			</div>
		</div>
		<div class="flex items-center gap-2">
			{#if error}
				<span class="text-xs font-semibold text-rose-300" aria-live="assertive">{error}</span>
			{/if}
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
				class="inline-flex h-8 items-center gap-1.5 border border-cyan-400/45 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 enabled:hover:bg-cyan-400/15 disabled:opacity-35"
				disabled={!dirty || saving}
				onclick={save}
			>
				<Save size={14} aria-hidden="true" />
				{saving ? '저장 중...' : '저장'}
			</button>
		</div>
	</header>

	<div class="divide-y divide-[var(--color-app-border-muted)]">
		{#each settings as setting (setting.key)}
			<div class="grid gap-3 px-4 py-4 sm:grid-cols-[minmax(0,1fr)_52px] sm:items-center">
				<div class="min-w-0">
					<strong class="block text-sm text-slate-100">{setting.title}</strong>
					<p class="mt-1 text-xs leading-5 text-slate-500">{setting.description}</p>
				</div>
				<button
					type="button"
					role="switch"
					aria-checked={draftSettings[setting.key]}
					aria-label={setting.title}
					disabled={saving}
					onclick={() => toggle(setting.key)}
					class={`relative h-6 w-11 rounded-full border transition ${
						draftSettings[setting.key]
							? 'border-cyan-300/50 bg-cyan-400/25'
							: 'border-slate-600 bg-slate-800'
					}`}
				>
					<span
						class={`absolute top-0.5 size-4.5 rounded-full bg-slate-100 transition ${
							draftSettings[setting.key] ? 'left-5.5' : 'left-0.5'
						}`}
					></span>
				</button>
			</div>
		{/each}
	</div>

	<div
		class="flex items-start gap-2 border-t border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] px-4 py-3"
	>
		<ShieldCheck size={14} class="mt-0.5 shrink-0 text-slate-500" aria-hidden="true" />
		<p class="text-xs leading-5 text-slate-500">
			같은 Discord 서버의 참가자와 운영자는 해당 서버의 내전 기록과 MMR을 계속 확인할 수 있습니다.
		</p>
	</div>
</section>
