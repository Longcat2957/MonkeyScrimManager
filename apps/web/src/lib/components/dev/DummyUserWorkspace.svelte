<script lang="ts">
	import { Dialog } from 'bits-ui';
	import { untrack } from 'svelte';
	import {
		Archive,
		ArchiveRestore,
		Boxes,
		Check,
		Pencil,
		Plus,
		Search,
		Trash2,
		Unlink,
		UserPlus,
		X
	} from 'lucide-svelte';
	import {
		bulkCreateDummyUsers,
		createDummyUser,
		deleteDummyGuildMembership,
		deleteDummyRiotAccount,
		deleteDummyUser,
		fetchDummyUserDeletionImpact,
		fetchDummyUsers,
		putDummyGuildMembership,
		putDummyChampionPreferences,
		putDummyRiotAccount,
		setDummyUserArchiveState,
		updateDummyUser
	} from '$lib/api/dummyUsers';
	import {
		createDummyGroup,
		deleteDummyGroup,
		fetchDummyGroups,
		replaceDummyGroupMembers,
		updateDummyGroup
	} from '$lib/api/dummyGroups';
	import { devToolsErrorText } from '$lib/api/devTools';
	import { createOperationIdempotencyKeys } from '$lib/api/operationIdempotency';
	import type {
		CreateDummyUserInput,
		DummyGroup,
		DummyGuildCatalogItem,
		DummyGuildMembership,
		DummyLane,
		DummyLaneMmr,
		DummyPrivacy,
		DummyUser,
		DummyUserDeletionImpact,
		DummyUserState
	} from '$lib/contracts/dummyUsers';
	import { createDummyUserInputSchema } from '$lib/contracts/dummyUsers';
	import DummyMemberPicker from './DummyMemberPicker.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import UserChampionPreferencesEditor from '$lib/components/workflow/UserChampionPreferencesEditor.svelte';
	import type { ChampionPreferenceLane, ChampionPreferences } from '$lib/contracts/workflow';
	import type { ChampionCatalog } from '$lib/datadragon';

	interface Props {
		initialUsers: DummyUser[];
		initialGroups: DummyGroup[];
		initialTotal: number;
		initialGuildId?: string | null;
		guilds: DummyGuildCatalogItem[];
		championCatalog: ChampionCatalog;
	}

	type UserStateFilter = 'ALL' | DummyUserState;
	type LaneFilter = 'ALL' | DummyLane;
	type CreateMode = 'SINGLE' | 'BULK';

	let {
		initialUsers,
		initialGroups,
		initialTotal,
		initialGuildId = null,
		guilds,
		championCatalog
	}: Props = $props();
	const initialState = untrack(() => {
		const firstGroup = initialGroups[0];
		return {
			users: [...initialUsers],
			totalUsers: initialTotal,
			guildFilter: initialGuildId ?? 'ALL',
			groups: [...initialGroups],
			selectedUserId: initialUsers[0]?.userId ?? null,
			editingGroupId: firstGroup?.id ?? null,
			groupIsNew: firstGroup === undefined,
			groupName: firstGroup?.name ?? '',
			groupDescription: firstGroup?.description ?? '',
			groupMemberIds:
				firstGroup?.members
					.slice()
					.sort((left, right) => left.position - right.position)
					.map((member) => member.userId) ?? []
		};
	});
	let users = $state(initialState.users);
	let totalUsers = $state(initialState.totalUsers);
	let loadingMore = $state(false);
	let groups = $state(initialState.groups);
	let selectedUserId = $state<string | null>(initialState.selectedUserId);

	let query = $state('');
	let stateFilter = $state<UserStateFilter>('ACTIVE');
	let guildFilter = $state(initialState.guildFilter);
	let laneFilter = $state<LaneFilter>('ALL');
	let groupFilter = $state('ALL');
	let notice = $state('');
	let errorMessage = $state('');
	let submitting = $state(false);
	const mutationKeys = createOperationIdempotencyKeys();

	let createOpen = $state(false);
	let createMode = $state<CreateMode>('SINGLE');
	let createStableKey = $state('player-01');
	let createDisplayName = $state('테스트 플레이어 01');
	let createAvatarSeed = $state('');
	let createNotes = $state('');
	let bulkStablePrefix = $state('player');
	let bulkNamePrefix = $state('테스트 플레이어');
	let bulkCount = $state(10);
	let bulkGuildId = $state('');
	let bulkBalancedLanes = $state(true);
	let bulkMmrMode = $state<'FIXED' | 'RANGE'>('FIXED');
	let bulkFixedMmr = $state(1500);
	let bulkMmrMin = $state(1350);
	let bulkMmrMax = $state(1750);
	let bulkGroupId = $state('');
	let createPrivacy = $state<DummyPrivacy>({
		publicProfileEnabled: false,
		showRiotIds: false,
		allowExternalRecordSearch: false
	});

	let editOpen = $state(false);
	let editDisplayName = $state('');
	let editAvatarSeed = $state('');
	let editNotes = $state('');
	let editPrivacy = $state<DummyPrivacy>({
		publicProfileEnabled: false,
		showRiotIds: false,
		allowExternalRecordSearch: false
	});

	let membershipOpen = $state(false);
	let editingGuildId = $state<string | null>(null);
	let membershipGuildId = $state('');
	let membershipDisplayName = $state('');
	let membershipStatus = $state<'ACTIVE' | 'INACTIVE'>('ACTIVE');
	let membershipLane = $state<'' | DummyLane>('');
	let membershipMmr = $state<number | ''>('');
	let membershipExistingMmr = $state<DummyLaneMmr | null>(null);
	let championsOpen = $state(false);
	let deletionOpen = $state(false);
	let deletionImpact = $state<DummyUserDeletionImpact | null>(null);
	let deletionLoading = $state(false);

	let riotOpen = $state(false);
	let riotPlatform = $state('KR');
	let riotRoutingRegion = $state('ASIA');
	let riotGameName = $state('');
	let riotTagLine = $state('DUMMY');
	let riotProfileIconId = $state<number | ''>('');
	let riotMainPosition = $state<'' | DummyLane | 'FLEX'>('');

	let groupsOpen = $state(false);
	let editingGroupId = $state<string | null>(initialState.editingGroupId);
	let groupIsNew = $state(initialState.groupIsNew);
	let groupName = $state(initialState.groupName);
	let groupDescription = $state(initialState.groupDescription);
	let groupMemberIds = $state(initialState.groupMemberIds);

	const groupById = $derived(new Map(groups.map((group) => [group.id, group])));
	const selectedUser = $derived(users.find((user) => user.userId === selectedUserId) ?? null);
	const filteredUsers = $derived(
		users.filter((user) => {
			const normalizedQuery = query.trim().toLocaleLowerCase();
			const matchesQuery =
				!normalizedQuery ||
				user.displayName.toLocaleLowerCase().includes(normalizedQuery) ||
				user.stableKey.toLocaleLowerCase().includes(normalizedQuery) ||
				user.userId.toLocaleLowerCase().includes(normalizedQuery);
			const matchesState = stateFilter === 'ALL' || user.state === stateFilter;
			const matchesGuild =
				guildFilter === 'ALL' ||
				user.guildMemberships.some((membership) => membership.guildId === guildFilter);
			const matchesLane =
				laneFilter === 'ALL' ||
				user.guildMemberships.some((membership) => membership.mainLane === laneFilter);
			const matchesGroup = groupFilter === 'ALL' || user.groupIds.includes(groupFilter);
			return matchesQuery && matchesState && matchesGuild && matchesLane && matchesGroup;
		})
	);
	const createPreview = $derived.by(() => buildCreatePreview());
	const selectedBulkGuild = $derived(guilds.find((guild) => guild.guildId === bulkGuildId) ?? null);
	const selectedBulkGroup = $derived(groups.find((group) => group.id === bulkGroupId) ?? null);
	const membershipGuild = $derived(
		guilds.find((guild) => guild.guildId === membershipGuildId) ?? null
	);
	const createPreviewValid = $derived(
		createPreview.length > 0 &&
			createPreview.every((input) => createDummyUserInputSchema.safeParse(input).success) &&
			new Set(createPreview.map((input) => input.stableKey)).size === createPreview.length &&
			createPreview.every((input) => !users.some((user) => user.stableKey === input.stableKey)) &&
			(createMode === 'SINGLE' ||
				((!bulkGuildId || selectedBulkGuild !== null) &&
					(!bulkGroupId ||
						(selectedBulkGroup !== null &&
							selectedBulkGroup.memberCount + createPreview.length <= 100)) &&
					(!selectedBulkGuild?.activeSeasonId ||
						(bulkMmrMode === 'FIXED'
							? validMmr(bulkFixedMmr)
							: validMmr(bulkMmrMin) && validMmr(bulkMmrMax) && bulkMmrMin <= bulkMmrMax))))
	);
	const membershipMmrValid = $derived(membershipMmr === '' || validMmr(membershipMmr));

	function buildCreatePreview(): CreateDummyUserInput[] {
		if (createMode === 'SINGLE') {
			return [
				compactCreateInput({
					stableKey: createStableKey.trim().toLocaleLowerCase(),
					displayName: createDisplayName.trim(),
					avatarSeed: createAvatarSeed.trim() || undefined,
					notes: createNotes.trim() || undefined,
					privacy: createPrivacy
				})
			];
		}
		const safeCount = Number.isInteger(bulkCount) ? Math.max(0, Math.min(bulkCount, 100)) : 0;
		const width = Math.max(2, String(safeCount).length);
		return Array.from({ length: safeCount }, (_, index) => {
			const number = String(index + 1).padStart(width, '0');
			return compactCreateInput({
				stableKey: `${bulkStablePrefix.trim().toLocaleLowerCase()}-${number}`,
				displayName: `${bulkNamePrefix.trim()} ${number}`,
				privacy: createPrivacy
			});
		});
	}

	function compactCreateInput(input: CreateDummyUserInput): CreateDummyUserInput {
		return Object.fromEntries(
			Object.entries(input).filter(([, value]) => value !== undefined)
		) as CreateDummyUserInput;
	}

	function bulkPreviewLane(index: number): DummyLane | null {
		if (!bulkGuildId || !bulkBalancedLanes) return null;
		return (['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'] as const)[index % 5] ?? null;
	}

	function bulkPreviewMmr(index: number): number | null {
		if (!selectedBulkGuild?.activeSeasonId) return null;
		if (bulkMmrMode === 'FIXED') return bulkFixedMmr;
		if (createPreview.length <= 1) return Math.round((bulkMmrMin + bulkMmrMax) / 2);
		return Math.round(
			bulkMmrMin + ((bulkMmrMax - bulkMmrMin) * index) / (createPreview.length - 1)
		);
	}

	function laneMmr(value: number) {
		return { top: value, jungle: value, mid: value, bottom: value, support: value };
	}

	function validMmr(value: number): boolean {
		return Number.isFinite(value) && value >= 0 && value <= 10_000;
	}

	async function submitCreate() {
		if (!createPreviewValid || submitting) return;
		submitting = true;
		clearMessages();
		try {
			const inputs =
				createMode === 'BULK' && bulkGuildId
					? createPreview.map((input, index) => {
							const mmr = bulkPreviewMmr(index);
							return {
								...input,
								guildMemberships: [
									{
										guildId: bulkGuildId,
										displayName: input.displayName,
										status: 'ACTIVE' as const,
										mainLane: bulkPreviewLane(index),
										...(mmr === null ? {} : { laneMmr: laneMmr(mmr) })
									}
								]
							};
						})
					: createPreview;
			const createOperation = createMode === 'SINGLE' ? 'create-user' : 'bulk-create-users';
			const createCommand = createMode === 'SINGLE' ? inputs[0]! : { users: inputs };
			const createKey = mutationKeys.keyFor(createOperation, createCommand);
			let created =
				createMode === 'SINGLE'
					? [await createDummyUser(inputs[0]!, createKey)]
					: (await bulkCreateDummyUsers(inputs, createKey)).users;
			mutationKeys.complete(createOperation);
			const compositionFailures: string[] = [];

			if (createMode === 'BULK' && selectedBulkGroup) {
				const previousIds = selectedBulkGroup.members
					.slice()
					.sort((left, right) => left.position - right.position)
					.map((member) => member.userId);
				const nextIds = [...new Set([...previousIds, ...created.map((user) => user.userId)])];
				const groupOperation = `group-members:${selectedBulkGroup.id}`;
				const groupCommand = {
					memberIds: nextIds,
					expectedRevision: selectedBulkGroup.revision
				};
				try {
					const updatedGroup = await replaceDummyGroupMembers(
						selectedBulkGroup.id,
						nextIds,
						selectedBulkGroup.revision,
						mutationKeys.keyFor(groupOperation, groupCommand)
					);
					mutationKeys.complete(groupOperation);
					groups = groups.map((group) => (group.id === updatedGroup.id ? updatedGroup : group));
					created = created.map((user) => ({
						...user,
						groupIds: user.groupIds.includes(updatedGroup.id)
							? user.groupIds
							: [...user.groupIds, updatedGroup.id]
					}));
				} catch (cause) {
					compositionFailures.push(`그룹 배치: ${devToolsErrorText(cause)}`);
				}
			}

			users = [...created, ...users];
			totalUsers += created.length;
			selectedUserId = created[0]?.userId ?? selectedUserId;
			notice = `${created.length}명의 더미 사용자를 생성했습니다.`;
			if (compositionFailures.length > 0) {
				errorMessage = `사용자는 생성됐지만 후속 배치 ${compositionFailures.length}건이 실패했습니다. ${compositionFailures.slice(0, 3).join(' / ')}`;
			}
			createOpen = false;
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			submitting = false;
		}
	}

	async function loadMoreUsers() {
		if (loadingMore || users.length >= totalUsers) return;
		loadingMore = true;
		clearMessages();
		try {
			const page = await fetchDummyUsers({ limit: 100, offset: users.length });
			const known = new Set(users.map((user) => user.userId));
			users = [...users, ...page.users.filter((user) => !known.has(user.userId))];
			totalUsers = page.total;
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			loadingMore = false;
		}
	}

	function openEdit(user: DummyUser) {
		editDisplayName = user.displayName;
		editAvatarSeed = user.avatarSeed ?? '';
		editNotes = user.notes ?? '';
		editPrivacy = { ...user.privacy };
		clearMessages();
		editOpen = true;
	}

	async function submitEdit() {
		if (!selectedUser || !editDisplayName.trim() || submitting) return;
		submitting = true;
		clearMessages();
		const operation = `update-user:${selectedUser.userId}`;
		const command = {
			displayName: editDisplayName.trim(),
			avatarSeed: editAvatarSeed.trim() || null,
			notes: editNotes.trim() || null,
			privacy: editPrivacy,
			expectedRevision: selectedUser.revision
		};
		try {
			const updated = await updateDummyUser(
				selectedUser.userId,
				command,
				mutationKeys.keyFor(operation, command)
			);
			mutationKeys.complete(operation);
			replaceUser(updated);
			notice = `${updated.displayName} 프로필을 저장했습니다.`;
			editOpen = false;
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			submitting = false;
		}
	}

	async function toggleArchive(user: DummyUser) {
		if (submitting) return;
		submitting = true;
		clearMessages();
		const action = user.state === 'ACTIVE' ? 'archive' : 'restore';
		const operation = `${action}-user:${user.userId}`;
		const command = { expectedRevision: user.revision };
		try {
			const updated = await setDummyUserArchiveState(
				user.userId,
				action,
				user.revision,
				mutationKeys.keyFor(operation, command)
			);
			mutationKeys.complete(operation);
			replaceUser(updated);
			notice = `${updated.displayName} 사용자를 ${action === 'archive' ? '보관' : '복구'}했습니다.`;
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			submitting = false;
		}
	}

	async function openDeletion(user: DummyUser) {
		selectedUserId = user.userId;
		deletionImpact = null;
		deletionLoading = true;
		clearMessages();
		deletionOpen = true;
		try {
			deletionImpact = await fetchDummyUserDeletionImpact(user.userId);
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			deletionLoading = false;
		}
	}

	async function confirmDelete() {
		if (!selectedUser || !deletionImpact?.canDelete || submitting) return;
		const deletedUser = selectedUser;
		const operation = `delete-user:${deletedUser.userId}`;
		submitting = true;
		clearMessages();
		try {
			await deleteDummyUser(
				deletedUser.userId,
				mutationKeys.keyFor(operation, { userId: deletedUser.userId })
			);
			mutationKeys.complete(operation);
			users = users.filter((user) => user.userId !== deletedUser.userId);
			totalUsers = Math.max(0, totalUsers - 1);
			selectedUserId = users[0]?.userId ?? null;
			deletionOpen = false;
			notice = `${deletedUser.displayName} 더미 사용자를 삭제했습니다.`;
			try {
				groups = (await fetchDummyGroups()).groups;
			} catch (cause) {
				errorMessage = `사용자는 삭제됐지만 그룹 목록을 갱신하지 못했습니다. ${devToolsErrorText(cause)}`;
			}
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			submitting = false;
		}
	}

	async function archiveFromDeletion() {
		if (!selectedUser || selectedUser.state === 'ARCHIVED') return;
		await toggleArchive(selectedUser);
		if (!errorMessage) deletionOpen = false;
	}

	function openMembership(user: DummyUser, membership: DummyGuildMembership | null = null) {
		selectedUserId = user.userId;
		editingGuildId = membership?.guildId ?? null;
		membershipGuildId =
			membership?.guildId ?? guilds.find((guild) => guild.status === 'ACTIVE')?.guildId ?? '';
		membershipDisplayName = membership?.displayName ?? user.displayName;
		membershipStatus = membership?.status ?? 'ACTIVE';
		membershipLane = membership?.mainLane ?? '';
		membershipExistingMmr = membership?.laneMmr ?? null;
		const targetGuild = guilds.find((guild) => guild.guildId === membershipGuildId);
		membershipMmr = membership ? '' : targetGuild?.activeSeasonId ? 1500 : '';
		clearMessages();
		membershipOpen = true;
	}

	function selectMembershipGuild(guildId: string) {
		membershipGuildId = guildId;
		membershipExistingMmr = null;
		membershipMmr = guilds.find((guild) => guild.guildId === guildId)?.activeSeasonId ? 1500 : '';
	}

	async function submitMembership() {
		if (!selectedUser || !membershipGuildId || !membershipDisplayName.trim() || submitting) return;
		const operation = `guild-membership:${selectedUser.userId}:${membershipGuildId}`;
		const command = {
			displayName: membershipDisplayName.trim(),
			status: membershipStatus,
			mainLane: membershipLane || null,
			...(membershipMmr === '' ? {} : { laneMmr: laneMmr(membershipMmr) })
		};
		submitting = true;
		clearMessages();
		try {
			const updated = await putDummyGuildMembership(
				selectedUser.userId,
				membershipGuildId,
				command,
				mutationKeys.keyFor(operation, command)
			);
			mutationKeys.complete(operation);
			replaceUser(updated);
			notice = `${updated.displayName} 길드 배치를 저장했습니다.`;
			membershipOpen = false;
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			submitting = false;
		}
	}

	async function removeMembership(user: DummyUser, membership: DummyGuildMembership) {
		if (submitting) return;
		const operation = `delete-guild-membership:${user.userId}:${membership.guildId}`;
		submitting = true;
		clearMessages();
		try {
			const updated = await deleteDummyGuildMembership(
				user.userId,
				membership.guildId,
				mutationKeys.keyFor(operation, {
					userId: user.userId,
					guildId: membership.guildId
				})
			);
			mutationKeys.complete(operation);
			replaceUser(updated);
			notice = `${membership.guildName} 배치를 해제했습니다.`;
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			submitting = false;
		}
	}

	function openRiotAccount(user: DummyUser) {
		selectedUserId = user.userId;
		riotPlatform = user.riotAccount?.platform ?? 'KR';
		riotRoutingRegion = user.riotAccount?.routingRegion ?? 'ASIA';
		riotGameName = user.riotAccount?.gameName ?? user.displayName.slice(0, 16);
		riotTagLine = user.riotAccount?.tagLine ?? 'DUMMY';
		riotProfileIconId = user.riotAccount?.profileIconId ?? '';
		riotMainPosition = user.riotAccount?.mainPosition ?? '';
		clearMessages();
		riotOpen = true;
	}

	async function saveRiotAccount() {
		if (!selectedUser || !riotGameName.trim() || !riotTagLine.trim() || submitting) return;
		const operation = `riot-account:${selectedUser.userId}`;
		const command = {
			platform: riotPlatform.trim().toLocaleUpperCase(),
			routingRegion: riotRoutingRegion.trim().toLocaleUpperCase(),
			gameName: riotGameName.trim(),
			tagLine: riotTagLine.trim(),
			...(riotProfileIconId === '' ? {} : { profileIconId: riotProfileIconId }),
			...(riotMainPosition === '' ? {} : { mainPosition: riotMainPosition })
		};
		submitting = true;
		clearMessages();
		try {
			const updated = await putDummyRiotAccount(
				selectedUser.userId,
				command,
				mutationKeys.keyFor(operation, command)
			);
			mutationKeys.complete(operation);
			replaceUser(updated);
			notice = `${updated.displayName} 가상 Riot 계정을 저장했습니다.`;
			riotOpen = false;
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			submitting = false;
		}
	}

	async function removeRiotAccount(user: DummyUser) {
		if (submitting || !user.riotAccount) return;
		const operation = `delete-riot-account:${user.userId}`;
		submitting = true;
		clearMessages();
		try {
			const updated = await deleteDummyRiotAccount(
				user.userId,
				mutationKeys.keyFor(operation, { userId: user.userId })
			);
			mutationKeys.complete(operation);
			replaceUser(updated);
			notice = `${updated.displayName} 가상 Riot 계정을 제거했습니다.`;
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			submitting = false;
		}
	}

	function openChampionPreferences(user: DummyUser) {
		selectedUserId = user.userId;
		clearMessages();
		championsOpen = true;
	}

	async function saveChampionLane(
		lane: ChampionPreferenceLane,
		championIds: string[]
	): Promise<ChampionPreferences> {
		if (!selectedUser) throw new Error('선호 챔피언을 저장할 사용자를 찾을 수 없습니다.');
		const operation = `champion-preferences:${selectedUser.userId}:${lane}`;
		const command = { championIds };
		try {
			const updated = await putDummyChampionPreferences(
				selectedUser.userId,
				lane,
				championIds,
				mutationKeys.keyFor(operation, command)
			);
			mutationKeys.complete(operation);
			replaceUser(updated);
			notice = `${updated.displayName} ${lane} 선호 챔피언을 저장했습니다.`;
			return updated.championPreferences;
		} catch (cause) {
			throw new Error(devToolsErrorText(cause));
		}
	}

	function openGroups() {
		const first = groups[0] ?? null;
		if (first) selectGroup(first);
		else startNewGroup();
		clearMessages();
		groupsOpen = true;
	}

	function selectGroup(group: DummyGroup) {
		editingGroupId = group.id;
		groupIsNew = false;
		groupName = group.name;
		groupDescription = group.description ?? '';
		groupMemberIds = group.members
			.slice()
			.sort((left, right) => left.position - right.position)
			.map((member) => member.userId);
	}

	function startNewGroup() {
		editingGroupId = null;
		groupIsNew = true;
		groupName = '';
		groupDescription = '';
		groupMemberIds = [];
	}

	async function saveGroup() {
		if (!groupName.trim() || submitting) return;
		submitting = true;
		clearMessages();
		try {
			if (groupIsNew) {
				const operation = 'create-group';
				const command = {
					name: groupName.trim(),
					description: groupDescription.trim() || undefined,
					memberIds: groupMemberIds
				};
				const created = await createDummyGroup(command, mutationKeys.keyFor(operation, command));
				mutationKeys.complete(operation);
				groups = [...groups, created];
				applyGroupMemberships(created.id, [], groupMemberIds);
				selectGroup(created);
				notice = `${created.name} 그룹을 생성했습니다.`;
				return;
			}

			const original = groups.find((group) => group.id === editingGroupId);
			if (!original) return;
			let updated = original;
			if (
				groupName.trim() !== original.name ||
				(groupDescription.trim() || null) !== original.description
			) {
				const operation = `update-group:${original.id}`;
				const command = {
					name: groupName.trim(),
					description: groupDescription.trim() || null,
					expectedRevision: original.revision
				};
				updated = await updateDummyGroup(
					original.id,
					command,
					mutationKeys.keyFor(operation, command)
				);
				mutationKeys.complete(operation);
				groups = groups.map((group) => (group.id === updated.id ? updated : group));
			}
			const originalIds = orderedGroupMemberIds(original);
			if (JSON.stringify(originalIds) !== JSON.stringify(groupMemberIds)) {
				const operation = `group-members:${updated.id}`;
				const desiredMemberIds = [...groupMemberIds];
				const command = {
					memberIds: desiredMemberIds,
					expectedRevision: updated.revision
				};
				try {
					updated = await replaceDummyGroupMembers(
						updated.id,
						desiredMemberIds,
						updated.revision,
						mutationKeys.keyFor(operation, command)
					);
					mutationKeys.complete(operation);
					applyGroupMemberships(updated.id, originalIds, desiredMemberIds);
				} catch (cause) {
					await recoverGroupAfterMemberFailure(updated.id, desiredMemberIds, operation, cause);
					return;
				}
			}
			groups = groups.map((group) => (group.id === updated.id ? updated : group));
			selectGroup(updated);
			notice = `${updated.name} 그룹을 저장했습니다.`;
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			submitting = false;
		}
	}

	async function removeGroup() {
		const group = groups.find((entry) => entry.id === editingGroupId);
		if (!group || submitting) return;
		const operation = `delete-group:${group.id}`;
		submitting = true;
		clearMessages();
		try {
			await deleteDummyGroup(group.id, mutationKeys.keyFor(operation, { groupId: group.id }));
			mutationKeys.complete(operation);
			groups = groups.filter((entry) => entry.id !== group.id);
			applyGroupMemberships(
				group.id,
				group.members.map((member) => member.userId),
				[]
			);
			const next = groups[0] ?? null;
			if (next) selectGroup(next);
			else startNewGroup();
			notice = `${group.name} 그룹을 삭제했습니다.`;
		} catch (cause) {
			errorMessage = devToolsErrorText(cause);
		} finally {
			submitting = false;
		}
	}

	async function recoverGroupAfterMemberFailure(
		groupId: string,
		desiredMemberIds: string[],
		operation: string,
		memberFailure: unknown
	) {
		const memberFailureText = devToolsErrorText(memberFailure);
		try {
			const refreshedGroups = (await fetchDummyGroups()).groups;
			groups = refreshedGroups;
			const refreshed = refreshedGroups.find((group) => group.id === groupId);
			if (!refreshed) {
				const previousLocalIds = users
					.filter((user) => user.groupIds.includes(groupId))
					.map((user) => user.userId);
				applyGroupMemberships(groupId, previousLocalIds, []);
				startNewGroup();
				errorMessage = `구성원 저장에 실패했고 해당 그룹이 더 이상 존재하지 않습니다. ${memberFailureText}`;
				return;
			}

			const previousLocalIds = users
				.filter((user) => user.groupIds.includes(groupId))
				.map((user) => user.userId);
			const refreshedMemberIds = orderedGroupMemberIds(refreshed);
			applyGroupMemberships(groupId, previousLocalIds, refreshedMemberIds);
			selectGroup(refreshed);
			if (sameIds(refreshedMemberIds, desiredMemberIds)) {
				mutationKeys.complete(operation);
				errorMessage = `구성원 저장 응답을 확인하지 못했지만 서버 최신 상태에는 반영되어 있습니다. ${memberFailureText}`;
				return;
			}

			errorMessage = `그룹 정보는 저장됐지만 구성원 저장에 실패했습니다. 서버 최신 상태로 복구했습니다. ${memberFailureText}`;
		} catch (refreshFailure) {
			errorMessage = `그룹 정보는 저장됐지만 구성원 저장에 실패했고 최신 상태도 다시 불러오지 못했습니다. 구성원 저장: ${memberFailureText} / 상태 갱신: ${devToolsErrorText(refreshFailure)}`;
		}
	}

	function orderedGroupMemberIds(group: DummyGroup): string[] {
		return group.members
			.slice()
			.sort((left, right) => left.position - right.position)
			.map((member) => member.userId);
	}

	function sameIds(left: string[], right: string[]): boolean {
		return JSON.stringify(left) === JSON.stringify(right);
	}

	function applyGroupMemberships(groupId: string, previousIds: string[], nextIds: string[]) {
		const previous = new Set(previousIds);
		const next = new Set(nextIds);
		users = users.map((user) => {
			if (!previous.has(user.userId) && !next.has(user.userId)) return user;
			const groupIds = user.groupIds.filter((id) => id !== groupId);
			if (next.has(user.userId)) groupIds.push(groupId);
			return { ...user, groupIds };
		});
	}

	function replaceUser(updated: DummyUser) {
		users = users.map((user) => (user.userId === updated.userId ? updated : user));
	}

	function clearMessages() {
		notice = '';
		errorMessage = '';
	}

	function userLaneLabel(user: DummyUser): string {
		const lanes = new Set(
			user.guildMemberships.flatMap((membership) =>
				membership.mainLane ? [membership.mainLane] : []
			)
		);
		return lanes.size > 0 ? [...lanes].join(' / ') : '미지정';
	}

	function groupNames(user: DummyUser): string[] {
		return user.groupIds.flatMap((id) => {
			const group = groupById.get(id);
			return group ? [group.name] : [];
		});
	}

	function blockingDeletionRows(impact: DummyUserDeletionImpact) {
		return [
			['길드 소속', impact.blockingReferences.guildMemberships],
			['일반 모집 참가', impact.blockingReferences.recruitmentParticipants],
			['경매 모집 참가', impact.blockingReferences.auctionParticipants],
			['경기 참가', impact.blockingReferences.gameParticipants],
			['로그인 세션', impact.blockingReferences.userSessions],
			['Discord 신원', impact.blockingReferences.discordIdentities],
			['실제 Riot 계정', impact.blockingReferences.realRiotAccounts]
		] as const;
	}

	function removableDeletionRows(impact: DummyUserDeletionImpact) {
		return [
			['그룹 소속', impact.removableReferences.groupMemberships],
			['선호 챔피언', impact.removableReferences.championPreferences],
			['가상 Riot 계정', impact.removableReferences.syntheticRiotAccounts]
		] as const;
	}
</script>

<div class="space-y-5">
	<WorkspaceHeader kicker="개발 도구" title="더미 사용자">
		{#snippet actions()}
			<button
				type="button"
				onclick={openGroups}
				class="inline-flex h-9 items-center gap-2 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-300 hover:border-cyan-400/35 hover:text-cyan-100"
			>
				<Boxes size={15} aria-hidden="true" /> 그룹 관리
			</button>
			<button
				type="button"
				onclick={() => {
					clearMessages();
					createOpen = true;
				}}
				class="inline-flex h-9 items-center gap-2 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 hover:bg-cyan-400/15"
			>
				<UserPlus size={15} aria-hidden="true" /> 사용자 생성
			</button>
		{/snippet}
	</WorkspaceHeader>

	{#if notice || errorMessage}
		<div
			class={`flex min-h-10 items-center gap-2 border px-3 text-xs ${
				errorMessage
					? 'border-rose-300/20 bg-rose-400/5 text-rose-200'
					: 'border-emerald-300/20 bg-emerald-400/5 text-emerald-200'
			}`}
			role={errorMessage ? 'alert' : 'status'}
		>
			{#if !errorMessage}<Check size={14} aria-hidden="true" />{/if}
			<span class="min-w-0 break-words">{errorMessage || notice}</span>
		</div>
	{/if}

	<section
		class="overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)]"
	>
		<div
			class="grid gap-2 border-b border-[var(--color-app-border-muted)] bg-[var(--color-app-bg-soft)] p-3 md:grid-cols-[minmax(220px,1fr)_116px_160px_130px_150px]"
		>
			<label
				class="flex h-10 min-w-0 items-center gap-2 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3"
			>
				<Search size={15} class="shrink-0 text-slate-500" aria-hidden="true" />
				<input
					bind:value={query}
					placeholder="이름, stable key, user ID"
					aria-label="더미 사용자 검색"
					class="min-w-0 flex-1 bg-transparent text-sm text-slate-100 outline-none placeholder:text-slate-600"
				/>
			</label>
			<select
				bind:value={stateFilter}
				aria-label="사용자 상태 필터"
				class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-300 outline-none"
			>
				<option value="ALL">전체 상태</option>
				<option value="ACTIVE">활성</option>
				<option value="ARCHIVED">보관</option>
			</select>
			<select
				bind:value={guildFilter}
				aria-label="길드 필터"
				class="h-10 min-w-0 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-300 outline-none"
			>
				<option value="ALL">전체 길드</option>
				{#each guilds as guild (guild.guildId)}
					<option value={guild.guildId}>{guild.displayName}</option>
				{/each}
			</select>
			<select
				bind:value={laneFilter}
				aria-label="라인 필터"
				class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-300 outline-none"
			>
				<option value="ALL">전체 라인</option>
				<option value="TOP">TOP</option>
				<option value="JUNGLE">JUNGLE</option>
				<option value="MID">MID</option>
				<option value="BOTTOM">BOTTOM</option>
				<option value="SUPPORT">SUPPORT</option>
			</select>
			<select
				bind:value={groupFilter}
				aria-label="그룹 필터"
				class="h-10 min-w-0 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-xs font-semibold text-slate-300 outline-none"
			>
				<option value="ALL">전체 그룹</option>
				{#each groups as group (group.id)}
					<option value={group.id}>{group.name}</option>
				{/each}
			</select>
		</div>

		<div class="grid min-h-[520px] xl:grid-cols-[minmax(0,1.35fr)_minmax(360px,0.65fr)]">
			<div class="min-w-0 border-b border-[var(--color-app-border)] xl:border-r xl:border-b-0">
				<div
					class="hidden grid-cols-[minmax(180px,1fr)_120px_110px_100px] border-b border-[var(--color-app-border-muted)] px-4 py-2 text-[10px] font-semibold text-slate-600 md:grid"
				>
					<span>사용자</span><span>그룹</span><span>길드</span><span>주 라인</span>
				</div>
				<div class="divide-y divide-[var(--color-app-border-muted)]">
					{#each filteredUsers as user (user.userId)}
						<button
							type="button"
							onclick={() => (selectedUserId = user.userId)}
							class={`grid w-full gap-3 px-4 py-3 text-left md:grid-cols-[minmax(180px,1fr)_120px_110px_100px] md:items-center ${
								selectedUserId === user.userId
									? 'bg-cyan-400/8'
									: 'hover:bg-[var(--color-app-hover)]'
							} ${user.state === 'ARCHIVED' ? 'opacity-55' : ''}`}
						>
							<span class="flex min-w-0 items-center gap-3">
								<span
									class="grid size-9 shrink-0 place-items-center border border-cyan-400/20 bg-cyan-400/8 text-xs font-bold text-cyan-100"
								>
									{user.displayName.slice(0, 1)}
								</span>
								<span class="min-w-0">
									<strong class="block truncate text-xs text-slate-100">{user.displayName}</strong>
									<small class="block truncate font-mono text-[10px] text-slate-600"
										>{user.stableKey}</small
									>
								</span>
							</span>
							<span class="truncate text-xs text-slate-400">
								{groupNames(user).join(', ') || '-'}
							</span>
							<span class="text-xs text-slate-400">{user.guildMemberships.length}개</span>
							<span class="text-xs font-semibold text-slate-300">{userLaneLabel(user)}</span>
						</button>
					{:else}
						<div class="px-4 py-16 text-center text-sm text-slate-600">검색 결과가 없습니다.</div>
					{/each}
				</div>
				{#if users.length < totalUsers}
					<div
						class="flex items-center justify-between border-t border-[var(--color-app-border-muted)] px-4 py-3"
					>
						<span class="text-[10px] text-slate-600">{users.length}/{totalUsers}명 불러옴</span>
						<button
							type="button"
							disabled={loadingMore}
							onclick={loadMoreUsers}
							class="h-8 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-300 hover:border-cyan-400/35 hover:text-cyan-100 disabled:opacity-35"
						>
							{loadingMore ? '불러오는 중' : '더 불러오기'}
						</button>
					</div>
				{/if}
			</div>

			<aside class="min-w-0 bg-[var(--color-app-bg-soft)]">
				{#if selectedUser}
					<header
						class="flex min-h-14 items-center justify-between gap-3 border-b border-[var(--color-app-border-muted)] px-4 py-2"
					>
						<div class="min-w-0">
							<div class="flex min-w-0 items-center gap-2">
								<h2 class="truncate text-sm font-semibold text-slate-50">
									{selectedUser.displayName}
								</h2>
								<span
									class={`shrink-0 border px-1.5 py-0.5 text-[9px] font-semibold ${
										selectedUser.state === 'ACTIVE'
											? 'border-emerald-300/25 text-emerald-200'
											: 'border-slate-600 text-slate-500'
									}`}
								>
									{selectedUser.state === 'ACTIVE' ? '활성' : '보관'}
								</span>
							</div>
							<p class="truncate font-mono text-[10px] text-slate-600">{selectedUser.userId}</p>
						</div>
						<div class="flex shrink-0 gap-1">
							<button
								type="button"
								onclick={() => openEdit(selectedUser)}
								aria-label="사용자 편집"
								title="사용자 편집"
								class="grid size-8 place-items-center border border-[var(--color-app-border)] text-slate-400 hover:border-cyan-400/35 hover:text-cyan-100"
							>
								<Pencil size={14} />
							</button>
							<button
								type="button"
								disabled={submitting}
								onclick={() => toggleArchive(selectedUser)}
								aria-label={selectedUser.state === 'ACTIVE' ? '사용자 보관' : '사용자 복구'}
								title={selectedUser.state === 'ACTIVE' ? '사용자 보관' : '사용자 복구'}
								class={`grid size-8 place-items-center border disabled:opacity-35 ${
									selectedUser.state === 'ACTIVE'
										? 'border-amber-300/25 text-amber-200 hover:bg-amber-400/10'
										: 'border-emerald-300/25 text-emerald-200 hover:bg-emerald-400/10'
								}`}
							>
								{#if selectedUser.state === 'ACTIVE'}
									<Archive size={14} />
								{:else}
									<ArchiveRestore size={14} />
								{/if}
							</button>
							<button
								type="button"
								disabled={submitting}
								onclick={() => openDeletion(selectedUser)}
								aria-label="사용자 삭제 검토"
								title="사용자 삭제 검토"
								class="grid size-8 place-items-center border border-rose-300/25 text-rose-200 hover:bg-rose-400/10 disabled:opacity-35"
							>
								<Trash2 size={14} />
							</button>
						</div>
					</header>

					<div class="divide-y divide-[var(--color-app-border-muted)]">
						<section class="grid grid-cols-[88px_minmax(0,1fr)] gap-3 px-4 py-3 text-xs">
							<span class="text-slate-600">Stable key</span>
							<strong class="truncate font-mono font-medium text-slate-300"
								>{selectedUser.stableKey}</strong
							>
							<span class="text-slate-600">그룹</span>
							<div class="flex min-w-0 flex-wrap gap-1">
								{#each groupNames(selectedUser) as name (name)}
									<span
										class="border border-cyan-400/20 bg-cyan-400/8 px-1.5 py-0.5 text-[10px] text-cyan-100"
										>{name}</span
									>
								{:else}
									<span class="text-slate-500">-</span>
								{/each}
							</div>
							<span class="text-slate-600">Revision</span>
							<strong class="font-mono font-medium text-slate-300">{selectedUser.revision}</strong>
						</section>

						<section class="px-4 py-3">
							<div class="mb-2 flex items-center justify-between gap-3">
								<h3 class="text-xs font-semibold text-slate-300">길드 배치</h3>
								<button
									type="button"
									disabled={selectedUser.state === 'ARCHIVED'}
									onclick={() => openMembership(selectedUser)}
									class="inline-flex h-7 items-center gap-1 px-2 text-[11px] font-semibold text-cyan-100 hover:bg-cyan-400/10 disabled:opacity-35"
								>
									<Plus size={13} aria-hidden="true" /> 배치 추가
								</button>
							</div>
							<div
								class="divide-y divide-[var(--color-app-border-muted)] border border-[var(--color-app-border-muted)]"
							>
								{#each selectedUser.guildMemberships as membership (membership.guildId)}
									<div class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-2 px-3 py-2">
										<span class="min-w-0">
											<strong class="block truncate text-xs text-slate-200"
												>{membership.guildName}</strong
											>
											<small class="block truncate text-[10px] text-slate-600">
												{membership.displayName} · {membership.mainLane ?? '라인 미지정'} · {membership.status ===
												'ACTIVE'
													? '활성'
													: '비활성'}
											</small>
										</span>
										<span class="flex gap-1">
											<button
												type="button"
												onclick={() => openMembership(selectedUser, membership)}
												aria-label={`${membership.guildName} 배치 편집`}
												title="배치 편집"
												class="grid size-7 place-items-center text-slate-500 hover:bg-[var(--color-app-hover)] hover:text-slate-200"
											>
												<Pencil size={13} />
											</button>
											<button
												type="button"
												disabled={submitting}
												onclick={() => removeMembership(selectedUser, membership)}
												aria-label={`${membership.guildName} 배치 해제`}
												title="배치 해제"
												class="grid size-7 place-items-center text-rose-300/70 hover:bg-rose-400/10 hover:text-rose-200 disabled:opacity-35"
											>
												<Trash2 size={13} />
											</button>
										</span>
									</div>
								{:else}
									<p class="px-3 py-5 text-center text-xs text-slate-600">길드 배치 없음</p>
								{/each}
							</div>
						</section>

						<section class="grid grid-cols-[minmax(0,1fr)_auto] gap-3 px-4 py-3 text-xs">
							<div>
								<h3 class="font-semibold text-slate-300">개인정보</h3>
								<p class="mt-1 text-[10px] leading-5 text-slate-600">
									프로필 {selectedUser.privacy.publicProfileEnabled ? '공개' : '비공개'} · Riot 연결 {selectedUser
										.privacy.showRiotIds
										? '공개'
										: '비공개'} · 외부 내전 검색 {selectedUser.privacy.allowExternalRecordSearch
										? '허용'
										: '차단'}
								</p>
							</div>
							<span class="self-start text-[10px] text-slate-600">DUMMY</span>
						</section>

						<section class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 px-4 py-3">
							<div class="min-w-0">
								<div class="flex items-center gap-2">
									<h3 class="text-xs font-semibold text-slate-300">Riot 계정</h3>
									<span
										class="border border-amber-300/25 bg-amber-300/8 px-1.5 py-0.5 text-[9px] font-semibold text-amber-200"
										>가상</span
									>
								</div>
								{#if selectedUser.riotAccount}
									<p class="mt-1 truncate text-xs text-slate-400">
										{selectedUser.riotAccount.gameName}#{selectedUser.riotAccount.tagLine}
									</p>
									<p class="mt-0.5 text-[10px] text-slate-600">
										{selectedUser.riotAccount.platform} · {selectedUser.riotAccount.mainPosition ??
											'포지션 미지정'}
									</p>
								{:else}
									<p class="mt-1 text-[10px] text-slate-600">연결 없음</p>
								{/if}
							</div>
							<div class="flex gap-1">
								<button
									type="button"
									onclick={() => openRiotAccount(selectedUser)}
									aria-label="가상 Riot 계정 편집"
									title="가상 Riot 계정 편집"
									class="grid size-8 place-items-center border border-[var(--color-app-border)] text-slate-400 hover:border-cyan-400/35 hover:text-cyan-100"
									><Pencil size={14} /></button
								>
								{#if selectedUser.riotAccount}<button
										type="button"
										disabled={submitting}
										onclick={() => removeRiotAccount(selectedUser)}
										aria-label="가상 Riot 계정 제거"
										title="가상 Riot 계정 제거"
										class="grid size-8 place-items-center border border-rose-300/25 text-rose-200 hover:bg-rose-400/10 disabled:opacity-35"
										><Unlink size={14} /></button
									>{/if}
							</div>
						</section>

						<section class="grid grid-cols-[minmax(0,1fr)_auto] items-center gap-3 px-4 py-3">
							<div class="min-w-0">
								<h3 class="text-xs font-semibold text-slate-300">선호 챔피언</h3>
								<p class="mt-1 text-[10px] text-slate-600">
									{Object.values(selectedUser.championPreferences).reduce(
										(total, ids) => total + ids.length,
										0
									)}개 등록
								</p>
							</div>
							<button
								type="button"
								onclick={() => openChampionPreferences(selectedUser)}
								aria-label="선호 챔피언 편집"
								title="선호 챔피언 편집"
								class="grid size-8 place-items-center border border-[var(--color-app-border)] text-slate-400 hover:border-cyan-400/35 hover:text-cyan-100"
								><Pencil size={14} /></button
							>
						</section>

						{#if selectedUser.notes}
							<section class="px-4 py-3">
								<h3 class="text-xs font-semibold text-slate-300">메모</h3>
								<p class="mt-1 whitespace-pre-wrap text-xs leading-5 text-slate-500">
									{selectedUser.notes}
								</p>
							</section>
						{/if}
					</div>
				{:else}
					<div class="grid min-h-[360px] place-items-center px-4 text-sm text-slate-600">
						사용자를 선택하세요.
					</div>
				{/if}
			</aside>
		</div>
	</section>
</div>

<Dialog.Root bind:open={deletionOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/65 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-lg -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50">사용자 삭제 검토</Dialog.Title>
				<Dialog.Close
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)]"
					aria-label="삭제 검토 닫기"><X size={16} /></Dialog.Close
				>
			</header>
			<div class="p-4">
				{#if deletionLoading}
					<div class="flex h-24 items-center justify-center gap-2 text-xs text-slate-500">
						<span
							class="size-4 animate-spin rounded-full border-2 border-cyan-300/25 border-t-cyan-200"
						></span> 참조 확인 중
					</div>
				{:else if deletionImpact}
					<div
						class={`border px-3 py-2 text-xs ${deletionImpact.canDelete ? 'border-emerald-300/20 bg-emerald-400/5 text-emerald-200' : 'border-amber-300/20 bg-amber-400/5 text-amber-200'}`}
					>
						{deletionImpact.canDelete
							? '삭제할 수 있습니다.'
							: '기록 참조가 남아 있어 삭제할 수 없습니다.'}
					</div>
					<div class="mt-3 grid gap-3 sm:grid-cols-2">
						<section class="border border-[var(--color-app-border-muted)]">
							<h3
								class="border-b border-[var(--color-app-border-muted)] px-3 py-2 text-[10px] font-semibold text-slate-500"
							>
								차단 참조
							</h3>
							<div class="divide-y divide-[var(--color-app-border-muted)]">
								{#each blockingDeletionRows(deletionImpact) as row (row[0])}<div
										class="flex justify-between px-3 py-1.5 text-xs"
									>
										<span class="text-slate-500">{row[0]}</span><strong
											class={row[1] > 0 ? 'text-rose-200' : 'text-slate-600'}>{row[1]}</strong
										>
									</div>{/each}
							</div>
						</section>
						<section class="border border-[var(--color-app-border-muted)]">
							<h3
								class="border-b border-[var(--color-app-border-muted)] px-3 py-2 text-[10px] font-semibold text-slate-500"
							>
								함께 제거
							</h3>
							<div class="divide-y divide-[var(--color-app-border-muted)]">
								{#each removableDeletionRows(deletionImpact) as row (row[0])}<div
										class="flex justify-between px-3 py-1.5 text-xs"
									>
										<span class="text-slate-500">{row[0]}</span><strong class="text-slate-300"
											>{row[1]}</strong
										>
									</div>{/each}
							</div>
						</section>
					</div>
				{/if}
				{#if errorMessage}<p class="mt-3 text-xs text-rose-200" role="alert">{errorMessage}</p>{/if}
			</div>
			<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
				<Dialog.Close
					class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400"
					>취소</Dialog.Close
				>
				{#if deletionImpact && !deletionImpact.canDelete && selectedUser?.state === 'ACTIVE'}<button
						type="button"
						disabled={submitting}
						onclick={archiveFromDeletion}
						class="inline-flex h-9 items-center gap-2 border border-amber-300/30 bg-amber-300/8 px-3 text-xs font-semibold text-amber-100 disabled:opacity-35"
						><Archive size={14} /> 대신 보관</button
					>{/if}
				<button
					type="button"
					disabled={!deletionImpact?.canDelete || submitting}
					onclick={confirmDelete}
					class="inline-flex h-9 items-center gap-2 border border-rose-300/30 bg-rose-400/10 px-3 text-xs font-semibold text-rose-100 disabled:opacity-35"
					><Trash2 size={14} /> 영구 삭제</button
				>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={createOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/65 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 max-h-[calc(100vh-1.5rem)] w-[calc(100vw-1.5rem)] max-w-3xl -translate-x-1/2 -translate-y-1/2 overflow-y-auto border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50">더미 사용자 생성</Dialog.Title>
				<Dialog.Close
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)]"
					aria-label="생성 닫기"><X size={16} /></Dialog.Close
				>
			</header>

			<div class="p-4">
				<div
					class="mb-4 inline-flex border border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] p-1"
				>
					<button
						type="button"
						onclick={() => (createMode = 'SINGLE')}
						class={`h-8 px-3 text-xs font-semibold ${createMode === 'SINGLE' ? 'bg-cyan-400/10 text-cyan-100' : 'text-slate-500'}`}
						>단일</button
					>
					<button
						type="button"
						onclick={() => (createMode = 'BULK')}
						class={`h-8 px-3 text-xs font-semibold ${createMode === 'BULK' ? 'bg-cyan-400/10 text-cyan-100' : 'text-slate-500'}`}
						>일괄</button
					>
				</div>

				<div class="grid gap-3 md:grid-cols-2">
					{#if createMode === 'SINGLE'}
						<label class="grid gap-1.5 text-xs font-semibold text-slate-500">
							<span>Stable key</span>
							<input
								bind:value={createStableKey}
								maxlength="64"
								class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 font-mono text-sm text-slate-100 outline-none focus:border-cyan-400/45"
							/>
						</label>
						<label class="grid gap-1.5 text-xs font-semibold text-slate-500">
							<span>표시 이름</span>
							<input
								bind:value={createDisplayName}
								maxlength="100"
								class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
							/>
						</label>
						<label class="grid gap-1.5 text-xs font-semibold text-slate-500">
							<span>Avatar seed</span>
							<input
								bind:value={createAvatarSeed}
								maxlength="128"
								class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
							/>
						</label>
						<label class="grid gap-1.5 text-xs font-semibold text-slate-500 md:row-span-2">
							<span>메모</span>
							<textarea
								bind:value={createNotes}
								maxlength="1000"
								class="min-h-24 resize-y border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
							></textarea>
						</label>
					{:else}
						<label class="grid gap-1.5 text-xs font-semibold text-slate-500">
							<span>Stable key 접두사</span>
							<input
								bind:value={bulkStablePrefix}
								maxlength="60"
								class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 font-mono text-sm text-slate-100 outline-none focus:border-cyan-400/45"
							/>
						</label>
						<label class="grid gap-1.5 text-xs font-semibold text-slate-500">
							<span>표시 이름 접두사</span>
							<input
								bind:value={bulkNamePrefix}
								maxlength="95"
								class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
							/>
						</label>
						<label class="grid gap-1.5 text-xs font-semibold text-slate-500">
							<span>생성 인원</span>
							<input
								type="number"
								min="1"
								max="100"
								bind:value={bulkCount}
								class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 font-mono text-sm text-slate-100 outline-none focus:border-cyan-400/45"
							/>
						</label>
						<label class="grid gap-1.5 text-xs font-semibold text-slate-500">
							<span>대상 길드</span>
							<select
								bind:value={bulkGuildId}
								class="h-10 min-w-0 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
							>
								<option value="">배치하지 않음</option>
								{#each guilds as guild (guild.guildId)}<option value={guild.guildId}
										>{guild.displayName}</option
									>{/each}
							</select>
						</label>
						<label class="grid gap-1.5 text-xs font-semibold text-slate-500">
							<span>생성 후 그룹</span>
							<select
								bind:value={bulkGroupId}
								class="h-10 min-w-0 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
							>
								<option value="">그룹 없음</option>
								{#each groups as group (group.id)}<option value={group.id}
										>{group.name} · {group.memberCount}명</option
									>{/each}
							</select>
						</label>
						<label
							class="flex h-10 items-center gap-2 self-end border border-[var(--color-app-border-muted)] px-3 text-xs font-semibold text-slate-300"
						>
							<input
								type="checkbox"
								bind:checked={bulkBalancedLanes}
								disabled={!bulkGuildId}
								class="size-4 accent-cyan-400 disabled:opacity-35"
							/>
							5개 라인 균등 배치
						</label>
						<div class="grid gap-1.5 text-xs font-semibold text-slate-500">
							<span>초기 MMR</span>
							<div class="grid grid-cols-[92px_minmax(0,1fr)] gap-2">
								<select
									bind:value={bulkMmrMode}
									disabled={!selectedBulkGuild?.activeSeasonId}
									class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-2 text-xs text-slate-100 outline-none disabled:opacity-40"
									><option value="FIXED">고정</option><option value="RANGE">범위</option></select
								>
								{#if bulkMmrMode === 'FIXED'}
									<input
										type="number"
										bind:value={bulkFixedMmr}
										min="0"
										max="10000"
										disabled={!selectedBulkGuild?.activeSeasonId}
										aria-label="고정 초기 MMR"
										class="h-10 min-w-0 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 font-mono text-sm text-slate-100 outline-none disabled:opacity-40"
									/>
								{:else}
									<span class="grid grid-cols-2 gap-2"
										><input
											type="number"
											bind:value={bulkMmrMin}
											min="0"
											max="10000"
											disabled={!selectedBulkGuild?.activeSeasonId}
											aria-label="최소 초기 MMR"
											class="h-10 min-w-0 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-2 font-mono text-sm text-slate-100 outline-none disabled:opacity-40"
										/><input
											type="number"
											bind:value={bulkMmrMax}
											min="0"
											max="10000"
											disabled={!selectedBulkGuild?.activeSeasonId}
											aria-label="최대 초기 MMR"
											class="h-10 min-w-0 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-2 font-mono text-sm text-slate-100 outline-none disabled:opacity-40"
										/></span
									>
								{/if}
							</div>
							{#if selectedBulkGuild && !selectedBulkGuild.activeSeasonId}<small
									class="font-normal text-amber-300/80">활성 시즌 없음 · MMR 제외</small
								>{/if}
						</div>
					{/if}
				</div>

				<fieldset class="mt-4 border border-[var(--color-app-border-muted)] p-3">
					<legend class="px-1 text-[11px] font-semibold text-slate-500">초기 개인정보</legend>
					<div class="grid gap-2 sm:grid-cols-3">
						<label class="flex items-center gap-2 text-xs text-slate-300"
							><input
								type="checkbox"
								bind:checked={createPrivacy.publicProfileEnabled}
								class="size-4 accent-cyan-400"
							/> 프로필 공개</label
						>
						<label class="flex items-center gap-2 text-xs text-slate-300"
							><input
								type="checkbox"
								bind:checked={createPrivacy.showRiotIds}
								class="size-4 accent-cyan-400"
							/> Riot 연결 공개</label
						>
						<label class="flex items-center gap-2 text-xs text-slate-300"
							><input
								type="checkbox"
								bind:checked={createPrivacy.allowExternalRecordSearch}
								class="size-4 accent-cyan-400"
							/> 외부 내전 검색 허용</label
						>
					</div>
				</fieldset>

				<div class="mt-4 overflow-hidden border border-[var(--color-app-border-muted)]">
					<div class="flex h-9 items-center justify-between bg-[var(--color-app-bg-soft)] px-3">
						<strong class="text-[11px] text-slate-400">생성 미리보기</strong>
						<span
							class={`text-[10px] font-semibold ${createPreviewValid ? 'text-emerald-300' : 'text-rose-300'}`}
							>{createPreview.length}명</span
						>
					</div>
					<div
						class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_70px_70px] gap-3 border-y border-[var(--color-app-border-muted)] px-3 py-1.5 text-[9px] font-semibold text-slate-600"
					>
						<span>STABLE KEY</span><span>표시 이름</span><span>라인</span><span>MMR</span>
					</div>
					<div class="max-h-44 divide-y divide-[var(--color-app-border-muted)] overflow-y-auto">
						{#each createPreview as candidate, index (candidate.stableKey)}
							<div
								class="grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)_70px_70px] gap-3 px-3 py-2 text-xs"
							>
								<span class="truncate font-mono text-slate-500">{candidate.stableKey}</span>
								<strong class="truncate font-medium text-slate-200">{candidate.displayName}</strong>
								<span class="font-semibold text-slate-400"
									>{createMode === 'BULK' ? (bulkPreviewLane(index) ?? '-') : '-'}</span
								>
								<span class="font-mono text-slate-400"
									>{createMode === 'BULK' ? (bulkPreviewMmr(index) ?? '-') : '-'}</span
								>
							</div>
						{/each}
					</div>
				</div>
				{#if errorMessage}<p class="mt-3 text-xs text-rose-200" role="alert">{errorMessage}</p>{/if}
			</div>
			<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
				<Dialog.Close
					class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400"
					>취소</Dialog.Close
				>
				<button
					type="button"
					disabled={!createPreviewValid || submitting}
					onclick={submitCreate}
					class="h-9 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 disabled:opacity-35"
					>{submitting ? '생성 중' : `${createPreview.length}명 생성`}</button
				>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={editOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/65 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-lg -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50">사용자 편집</Dialog.Title>
				<Dialog.Close
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)]"
					aria-label="편집 닫기"><X size={16} /></Dialog.Close
				>
			</header>
			<div class="grid gap-3 p-4">
				<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
					><span>표시 이름</span><input
						bind:value={editDisplayName}
						maxlength="100"
						class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
					/></label
				>
				<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
					><span>Avatar seed</span><input
						bind:value={editAvatarSeed}
						maxlength="128"
						class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
					/></label
				>
				<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
					><span>메모</span><textarea
						bind:value={editNotes}
						maxlength="1000"
						class="min-h-24 resize-y border border-[var(--color-app-border)] bg-[var(--color-app-control)] p-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
					></textarea></label
				>
				<div class="grid gap-2 border border-[var(--color-app-border-muted)] p-3 sm:grid-cols-3">
					<label class="flex items-center gap-2 text-xs text-slate-300"
						><input
							type="checkbox"
							bind:checked={editPrivacy.publicProfileEnabled}
							class="size-4 accent-cyan-400"
						/> 프로필 공개</label
					>
					<label class="flex items-center gap-2 text-xs text-slate-300"
						><input
							type="checkbox"
							bind:checked={editPrivacy.showRiotIds}
							class="size-4 accent-cyan-400"
						/> Riot 연결 공개</label
					>
					<label class="flex items-center gap-2 text-xs text-slate-300"
						><input
							type="checkbox"
							bind:checked={editPrivacy.allowExternalRecordSearch}
							class="size-4 accent-cyan-400"
						/> 외부 내전 검색 허용</label
					>
				</div>
				{#if errorMessage}<p class="text-xs text-rose-200" role="alert">{errorMessage}</p>{/if}
			</div>
			<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
				<Dialog.Close
					class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400"
					>취소</Dialog.Close
				><button
					type="button"
					disabled={!editDisplayName.trim() || submitting}
					onclick={submitEdit}
					class="h-9 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 disabled:opacity-35"
					>{submitting ? '저장 중' : '저장'}</button
				>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={membershipOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/65 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-md -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50">길드 배치</Dialog.Title>
				<Dialog.Close
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)]"
					aria-label="길드 배치 닫기"><X size={16} /></Dialog.Close
				>
			</header>
			<div class="grid gap-3 p-4">
				<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
					><span>길드</span><select
						value={membershipGuildId}
						onchange={(event) => selectMembershipGuild(event.currentTarget.value)}
						disabled={editingGuildId !== null}
						class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none disabled:opacity-55"
						><option value="" disabled>길드 선택</option
						>{#each guilds as guild (guild.guildId)}<option value={guild.guildId}
								>{guild.displayName}</option
							>{/each}</select
					></label
				>
				<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
					><span>길드 표시 이름</span><input
						bind:value={membershipDisplayName}
						maxlength="100"
						class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
					/></label
				>
				<div class="grid grid-cols-2 gap-3">
					<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
						><span>상태</span><select
							bind:value={membershipStatus}
							class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none"
							><option value="ACTIVE">활성</option><option value="INACTIVE">비활성</option></select
						></label
					>
					<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
						><span>주 라인</span><select
							bind:value={membershipLane}
							class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none"
							><option value="">미지정</option><option value="TOP">TOP</option><option
								value="JUNGLE">JUNGLE</option
							><option value="MID">MID</option><option value="BOTTOM">BOTTOM</option><option
								value="SUPPORT">SUPPORT</option
							></select
						></label
					>
				</div>
				<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
					><span>공통 초기 MMR</span><input
						type="number"
						bind:value={membershipMmr}
						min="0"
						max="10000"
						disabled={!membershipGuild?.activeSeasonId}
						placeholder={membershipExistingMmr ? '비우면 기존 라인별 MMR 유지' : '1500'}
						class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 font-mono text-sm text-slate-100 outline-none placeholder:text-slate-600 disabled:opacity-40"
					/>{#if membershipExistingMmr}<small class="font-normal text-slate-600"
							>현재 TOP {membershipExistingMmr.top} · JGL {membershipExistingMmr.jungle} · MID {membershipExistingMmr.mid}
							· BOT {membershipExistingMmr.bottom} · SUP {membershipExistingMmr.support}</small
						>{:else if membershipGuild && !membershipGuild.activeSeasonId}<small
							class="font-normal text-amber-300/80">활성 시즌이 없어 MMR을 저장하지 않습니다.</small
						>{/if}</label
				>
				{#if errorMessage}<p class="text-xs text-rose-200" role="alert">{errorMessage}</p>{/if}
			</div>
			<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
				<Dialog.Close
					class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400"
					>취소</Dialog.Close
				><button
					type="button"
					disabled={!membershipGuildId ||
						!membershipDisplayName.trim() ||
						!membershipMmrValid ||
						submitting}
					onclick={submitMembership}
					class="h-9 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 disabled:opacity-35"
					>{submitting ? '저장 중' : '배치 저장'}</button
				>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={riotOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/65 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 w-[calc(100vw-1.5rem)] max-w-lg -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<div class="flex items-center gap-2">
					<Dialog.Title class="text-sm font-semibold text-slate-50">Riot 계정 편집</Dialog.Title
					><span
						class="border border-amber-300/25 bg-amber-300/8 px-1.5 py-0.5 text-[9px] font-semibold text-amber-200"
						>가상</span
					>
				</div>
				<Dialog.Close
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)]"
					aria-label="Riot 계정 편집 닫기"><X size={16} /></Dialog.Close
				>
			</header>
			<div class="grid gap-3 p-4">
				<div class="grid grid-cols-2 gap-3">
					<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
						><span>Platform</span><input
							bind:value={riotPlatform}
							maxlength="16"
							class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 font-mono text-sm text-slate-100 outline-none focus:border-cyan-400/45"
						/></label
					>
					<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
						><span>Routing region</span><input
							bind:value={riotRoutingRegion}
							maxlength="16"
							class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 font-mono text-sm text-slate-100 outline-none focus:border-cyan-400/45"
						/></label
					>
				</div>
				<div class="grid grid-cols-[minmax(0,1fr)_120px] gap-3">
					<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
						><span>Game name</span><input
							bind:value={riotGameName}
							maxlength="64"
							class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
						/></label
					>
					<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
						><span>Tag line</span><input
							bind:value={riotTagLine}
							maxlength="16"
							class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
						/></label
					>
				</div>
				<div class="grid grid-cols-2 gap-3">
					<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
						><span>Profile icon ID</span><input
							type="number"
							min="0"
							max="2147483647"
							bind:value={riotProfileIconId}
							class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 font-mono text-sm text-slate-100 outline-none focus:border-cyan-400/45"
						/></label
					>
					<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
						><span>주 포지션</span><select
							bind:value={riotMainPosition}
							class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none"
							><option value="">미지정</option><option value="TOP">TOP</option><option
								value="JUNGLE">JUNGLE</option
							><option value="MID">MID</option><option value="BOTTOM">BOTTOM</option><option
								value="SUPPORT">SUPPORT</option
							><option value="FLEX">FLEX</option></select
						></label
					>
				</div>
				{#if errorMessage}<p class="text-xs text-rose-200" role="alert">{errorMessage}</p>{/if}
			</div>
			<footer class="flex justify-end gap-2 border-t border-[var(--color-app-border)] px-4 py-3">
				<Dialog.Close
					class="h-9 border border-[var(--color-app-border)] px-3 text-xs font-semibold text-slate-400"
					>취소</Dialog.Close
				><button
					type="button"
					disabled={!riotGameName.trim() || !riotTagLine.trim() || submitting}
					onclick={saveRiotAccount}
					class="h-9 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 disabled:opacity-35"
					>{submitting ? '저장 중' : '가상 계정 저장'}</button
				>
			</footer>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={championsOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/65 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 max-h-[calc(100vh-1.5rem)] w-[calc(100vw-1.5rem)] max-w-6xl -translate-x-1/2 -translate-y-1/2 overflow-y-auto border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50"
					>더미 사용자 선호 챔피언</Dialog.Title
				>
				<Dialog.Close
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)]"
					aria-label="선호 챔피언 편집 닫기"><X size={16} /></Dialog.Close
				>
			</header>
			{#if selectedUser}
				{#key selectedUser.userId}
					<UserChampionPreferencesEditor
						catalog={championCatalog}
						initialPreferences={selectedUser.championPreferences}
						onSaveLane={saveChampionLane}
					/>
				{/key}
			{/if}
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>

<Dialog.Root bind:open={groupsOpen}>
	<Dialog.Portal>
		<Dialog.Overlay class="fixed inset-0 z-50 bg-black/65 backdrop-blur-sm" />
		<Dialog.Content
			class="fixed top-1/2 left-1/2 z-50 max-h-[calc(100vh-1.5rem)] w-[calc(100vw-1.5rem)] max-w-5xl -translate-x-1/2 -translate-y-1/2 overflow-hidden border border-[var(--color-app-border)] bg-[var(--color-app-surface)] shadow-2xl shadow-black/50 outline-none"
		>
			<header
				class="flex h-12 items-center justify-between border-b border-[var(--color-app-border)] px-4"
			>
				<Dialog.Title class="text-sm font-semibold text-slate-50">더미 그룹 관리</Dialog.Title>
				<Dialog.Close
					class="grid size-8 place-items-center text-slate-400 hover:bg-[var(--color-app-hover)]"
					aria-label="그룹 관리 닫기"><X size={16} /></Dialog.Close
				>
			</header>
			<div
				class="grid max-h-[calc(100vh-8rem)] overflow-y-auto md:grid-cols-[220px_minmax(0,1fr)] md:overflow-hidden"
			>
				<aside
					class="border-b border-[var(--color-app-border)] bg-[var(--color-app-bg-soft)] md:border-r md:border-b-0"
				>
					<div
						class="flex h-11 items-center justify-between border-b border-[var(--color-app-border-muted)] px-3"
					>
						<span class="text-[11px] font-semibold text-slate-500">그룹</span><button
							type="button"
							onclick={startNewGroup}
							aria-label="새 그룹"
							title="새 그룹"
							class="grid size-7 place-items-center text-cyan-100 hover:bg-cyan-400/10"
							><Plus size={14} /></button
						>
					</div>
					<div
						class="max-h-48 divide-y divide-[var(--color-app-border-muted)] overflow-y-auto md:max-h-[calc(100vh-12rem)]"
					>
						{#each groups as group (group.id)}
							<button
								type="button"
								onclick={() => selectGroup(group)}
								class={`w-full px-3 py-3 text-left ${!groupIsNew && editingGroupId === group.id ? 'bg-cyan-400/8' : 'hover:bg-[var(--color-app-hover)]'}`}
								><strong class="block truncate text-xs text-slate-200">{group.name}</strong><small
									class="mt-0.5 block text-[10px] text-slate-600"
									>{group.memberCount}명 · r{group.revision}</small
								></button
							>
						{/each}
						{#if groupIsNew}<div class="px-3 py-3 text-xs font-semibold text-cyan-100">
								새 그룹
							</div>{/if}
					</div>
				</aside>

				<div class="max-h-[calc(100vh-12rem)] overflow-y-auto p-4">
					<div class="grid gap-3 sm:grid-cols-[minmax(0,1fr)_minmax(0,1.5fr)]">
						<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
							><span>그룹 이름</span><input
								bind:value={groupName}
								maxlength="100"
								class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
							/></label
						>
						<label class="grid gap-1.5 text-xs font-semibold text-slate-500"
							><span>설명</span><input
								bind:value={groupDescription}
								maxlength="500"
								class="h-10 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-3 text-sm text-slate-100 outline-none focus:border-cyan-400/45"
							/></label
						>
					</div>
					<div class="mt-4">
						<DummyMemberPicker
							{users}
							selectedIds={groupMemberIds}
							onSelectionChange={(ids) => (groupMemberIds = ids)}
							disabled={submitting}
						/>
					</div>
					{#if errorMessage}<p class="mt-3 text-xs text-rose-200" role="alert">
							{errorMessage}
						</p>{/if}
					<div
						class="mt-4 flex items-center justify-between gap-3 border-t border-[var(--color-app-border-muted)] pt-3"
					>
						{#if !groupIsNew}<button
								type="button"
								disabled={submitting}
								onclick={removeGroup}
								class="inline-flex h-9 items-center gap-2 border border-rose-300/25 px-3 text-xs font-semibold text-rose-200 hover:bg-rose-400/10 disabled:opacity-35"
								><Trash2 size={14} aria-hidden="true" /> 그룹 삭제</button
							>{:else}<span></span>{/if}
						<button
							type="button"
							disabled={!groupName.trim() || submitting}
							onclick={saveGroup}
							class="h-9 border border-cyan-400/40 bg-cyan-400/10 px-3 text-xs font-semibold text-cyan-100 disabled:opacity-35"
							>{submitting ? '저장 중' : '그룹 저장'}</button
						>
					</div>
				</div>
			</div>
		</Dialog.Content>
	</Dialog.Portal>
</Dialog.Root>
