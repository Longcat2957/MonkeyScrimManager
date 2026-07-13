<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { SvelteSet } from 'svelte/reactivity';
	import GuildAuditLogPanel from '$lib/components/workflow/GuildAuditLogPanel.svelte';
	import GuildMemberManagementPanel from '$lib/components/workflow/GuildMemberManagementPanel.svelte';
	import GuildSettingsAdminPanel from '$lib/components/workflow/GuildSettingsAdminPanel.svelte';
	import {
		adjustGuildMemberMmr,
		fetchGuildAdminMembers,
		GuildAdminMembersApiError,
		updateGuildMemberStatus
	} from '$lib/api/guildAdminMembers';
	import {
		fetchGuildAdminSettings,
		GuildAdminSettingsApiError,
		updateGuildAdminSettings
	} from '$lib/api/guildAdminSettings';
	import { fetchGuildAuditLogs, GuildAuditLogsApiError } from '$lib/api/guildAuditLogs';
	import type {
		GuildAdminSettingsSnapshot,
		GuildAuditEntry,
		GuildAuditLogsSnapshot
	} from '$lib/contracts/workflow';
	import type {
		GuildMemberStatusCommand,
		GuildMmrAdjustmentCommand,
		GuildSettingsUpdateCommand
	} from '$lib/realtime/admin';
	import type { PageProps } from './$types';

	type AdminTab = 'members' | 'settings' | 'logs';
	interface AuditPaginationState {
		source: GuildAuditLogsSnapshot;
		entries: GuildAuditEntry[];
		nextCursor: string | null;
		loading: boolean;
		error: string | null;
	}

	let { data }: PageProps = $props();

	const activeTab = $derived(parseTab(page.url.searchParams.get('tab')));
	let memberSnapshot = $derived(data.memberSnapshot);
	let settingsSnapshot = $derived(data.settingsSnapshot);
	let auditPagination = $state<AuditPaginationState | null>(null);
	const currentAuditPagination = $derived(
		auditPagination?.source === data.auditSnapshot ? auditPagination : null
	);
	const auditEntries = $derived(
		currentAuditPagination?.entries ?? data.auditSnapshot?.entries ?? []
	);
	const auditNextCursor = $derived(
		currentAuditPagination?.nextCursor ?? data.auditSnapshot?.nextCursor ?? null
	);
	const auditLoadingMore = $derived(currentAuditPagination?.loading ?? false);
	const auditLoadMoreError = $derived(currentAuditPagination?.error ?? null);
	const consumedAuditCursors = new SvelteSet<string>();
	let consumedAuditSource: GuildAuditLogsSnapshot | null = null;

	function parseTab(value: string | null): AdminTab {
		return value === 'settings' || value === 'logs' ? value : 'members';
	}

	async function handleMmrAdjustment(command: GuildMmrAdjustmentCommand) {
		try {
			memberSnapshot = await adjustGuildMemberMmr(data.guildId, command);
		} catch (cause) {
			await refreshAfterConflict(cause);
			throw cause;
		}
	}

	async function handleMemberStatus(command: GuildMemberStatusCommand) {
		try {
			memberSnapshot = await updateGuildMemberStatus(data.guildId, command);
		} catch (cause) {
			await refreshAfterConflict(cause);
			throw cause;
		}
	}

	async function refreshAfterConflict(cause: unknown) {
		if (
			cause instanceof GuildAdminMembersApiError &&
			cause.status === 409 &&
			cause.code === 'revision_conflict'
		) {
			memberSnapshot = await fetchGuildAdminMembers(data.guildId);
		}
	}

	async function handleSettingsSave(
		command: GuildSettingsUpdateCommand
	): Promise<GuildAdminSettingsSnapshot> {
		try {
			const snapshot = await updateGuildAdminSettings(data.guildId, command);
			settingsSnapshot = snapshot;
			return snapshot;
		} catch (cause) {
			if (
				cause instanceof GuildAdminSettingsApiError &&
				cause.status === 409 &&
				cause.code === 'revision_conflict'
			) {
				settingsSnapshot = await fetchGuildAdminSettings(data.guildId);
			}
			throw cause;
		}
	}

	async function handleAuditLoadMore() {
		const source = data.auditSnapshot;
		if (!source) return;
		if (consumedAuditSource !== source) {
			consumedAuditCursors.clear();
			consumedAuditSource = source;
		}
		const cursor = auditNextCursor;
		if (!cursor || auditLoadingMore || consumedAuditCursors.has(cursor)) return;

		const existingEntries = auditEntries;
		auditPagination = {
			source,
			entries: existingEntries,
			nextCursor: cursor,
			loading: true,
			error: null
		};
		consumedAuditCursors.add(cursor);
		try {
			const snapshot = await fetchGuildAuditLogs(data.guildId, { limit: 50, cursor });
			if (data.auditSnapshot !== source) return;
			const knownIds = new Set(existingEntries.map((entry) => entry.id));
			const mergedEntries = [
				...existingEntries,
				...snapshot.entries.filter((entry) => !knownIds.has(entry.id))
			];

			const repeatedCursor =
				snapshot.nextCursor === cursor || consumedAuditCursors.has(snapshot.nextCursor ?? '');
			auditPagination = {
				source,
				entries: mergedEntries,
				nextCursor: repeatedCursor ? null : snapshot.nextCursor,
				loading: false,
				error: repeatedCursor ? '중복된 페이지 정보가 반환되어 추가 조회를 중단했습니다.' : null
			};
		} catch (cause) {
			consumedAuditCursors.delete(cursor);
			if (data.auditSnapshot !== source) return;
			auditPagination = {
				source,
				entries: existingEntries,
				nextCursor: cursor,
				loading: false,
				error:
					cause instanceof GuildAuditLogsApiError
						? cause.message
						: '감사 로그를 더 불러오지 못했습니다.'
			};
		}
	}
</script>

{#if activeTab === 'members'}
	{#if memberSnapshot}
		<GuildMemberManagementPanel
			members={memberSnapshot.members}
			revision={memberSnapshot.revision}
			currentUserId={data.user.discordId}
			dummyUsersHref={data.user.serviceAdmin
				? `${resolve('/dev/users')}?guildId=${encodeURIComponent(data.guildId)}`
				: null}
			onAdjustMmr={handleMmrAdjustment}
			onSetStatus={handleMemberStatus}
		/>
	{/if}
{:else if activeTab === 'settings'}
	{#if settingsSnapshot}
		<GuildSettingsAdminPanel snapshot={settingsSnapshot} onSave={handleSettingsSave} />
	{/if}
{:else}
	<GuildAuditLogPanel
		entries={auditEntries}
		nextCursor={auditNextCursor}
		loadingMore={auditLoadingMore}
		loadMoreError={auditLoadMoreError}
		currentUserId={data.user.discordId}
		onLoadMore={handleAuditLoadMore}
	/>
{/if}
