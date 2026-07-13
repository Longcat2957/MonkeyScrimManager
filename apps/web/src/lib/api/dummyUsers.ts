import {
	createDummyUserInputSchema,
	dummyBulkCreateDataSchema,
	dummyGuildCatalogDataSchema,
	dummyUserDeletionImpactSchema,
	dummyUserDeletionResultSchema,
	dummyUserSchema,
	dummyUsersListDataSchema,
	putDummyGuildMembershipInputSchema,
	putDummyRiotAccountInputSchema,
	updateDummyUserInputSchema,
	type CreateDummyUserInput,
	type DummyGuildCatalogData,
	type DummyLane,
	type DummyUsersListData,
	type DummyUserState,
	type PutDummyGuildMembershipInput,
	type PutDummyRiotAccountInput,
	type UpdateDummyUserInput
} from '$lib/contracts/dummyUsers';
import { apiRoute } from '$lib/routing';
import { requestDevData } from './devTools';

interface DummyUserListQuery {
	query?: string;
	state?: DummyUserState;
	guildId?: string;
	groupId?: string;
	limit?: number;
	offset?: number;
}

function listPath(query: DummyUserListQuery = {}): string {
	const params: Record<string, string | number> = {};
	if (query.query?.trim()) params.query = query.query.trim();
	if (query.state) params.state = query.state;
	if (query.guildId) params.guildId = query.guildId;
	if (query.groupId) params.groupId = query.groupId;
	if (query.limit !== undefined) params.limit = query.limit;
	if (query.offset !== undefined) params.offset = query.offset;
	return apiRoute(['dev', 'dummy-users'], params);
}

export function fetchDummyUsers(query: DummyUserListQuery = {}): Promise<DummyUsersListData> {
	return requestDevData(listPath(query), dummyUsersListDataSchema);
}

export function fetchDummyGuildCatalog(): Promise<DummyGuildCatalogData> {
	return requestDevData(apiRoute(['dev', 'dummy-users', 'catalog']), dummyGuildCatalogDataSchema);
}

export function createDummyUser(
	input: CreateDummyUserInput,
	idempotencyKey: string = globalThis.crypto.randomUUID()
) {
	return requestDevData(apiRoute(['dev', 'dummy-users']), dummyUserSchema, {
		method: 'POST',
		headers: { 'Idempotency-Key': idempotencyKey },
		body: JSON.stringify(createDummyUserInputSchema.parse(input))
	});
}

export function bulkCreateDummyUsers(
	users: CreateDummyUserInput[],
	idempotencyKey: string = globalThis.crypto.randomUUID()
) {
	return requestDevData(apiRoute(['dev', 'dummy-users', 'bulk']), dummyBulkCreateDataSchema, {
		method: 'POST',
		headers: { 'Idempotency-Key': idempotencyKey },
		body: JSON.stringify({ users: users.map((user) => createDummyUserInputSchema.parse(user)) })
	});
}

export function updateDummyUser(
	userId: string,
	input: UpdateDummyUserInput,
	idempotencyKey: string = globalThis.crypto.randomUUID()
) {
	return requestDevData(apiRoute(['dev', 'dummy-users', userId]), dummyUserSchema, {
		method: 'PATCH',
		headers: { 'Idempotency-Key': idempotencyKey },
		body: JSON.stringify(updateDummyUserInputSchema.parse(input))
	});
}

export function setDummyUserArchiveState(
	userId: string,
	action: 'archive' | 'restore',
	expectedRevision: number,
	idempotencyKey: string = globalThis.crypto.randomUUID()
) {
	return requestDevData(apiRoute(['dev', 'dummy-users', userId, action]), dummyUserSchema, {
		method: 'POST',
		headers: { 'Idempotency-Key': idempotencyKey },
		body: JSON.stringify({ expectedRevision })
	});
}

export function putDummyGuildMembership(
	userId: string,
	guildId: string,
	input: PutDummyGuildMembershipInput,
	idempotencyKey: string = globalThis.crypto.randomUUID()
) {
	return requestDevData(
		apiRoute(['dev', 'dummy-users', userId, 'guilds', guildId]),
		dummyUserSchema,
		{
			method: 'PUT',
			headers: { 'Idempotency-Key': idempotencyKey },
			body: JSON.stringify(putDummyGuildMembershipInputSchema.parse(input))
		}
	);
}

export function deleteDummyGuildMembership(
	userId: string,
	guildId: string,
	idempotencyKey: string = globalThis.crypto.randomUUID()
) {
	return requestDevData(
		apiRoute(['dev', 'dummy-users', userId, 'guilds', guildId]),
		dummyUserSchema,
		{
			method: 'DELETE',
			headers: { 'Idempotency-Key': idempotencyKey },
			body: JSON.stringify({})
		}
	);
}

export function putDummyChampionPreferences(
	userId: string,
	lane: DummyLane,
	championIds: string[],
	idempotencyKey: string = globalThis.crypto.randomUUID()
) {
	return requestDevData(
		apiRoute(['dev', 'dummy-users', userId, 'champion-preferences', lane]),
		dummyUserSchema,
		{
			method: 'PUT',
			headers: { 'Idempotency-Key': idempotencyKey },
			body: JSON.stringify({ championIds })
		}
	);
}

export function putDummyRiotAccount(
	userId: string,
	input: PutDummyRiotAccountInput,
	idempotencyKey: string = globalThis.crypto.randomUUID()
) {
	return requestDevData(apiRoute(['dev', 'dummy-users', userId, 'riot-account']), dummyUserSchema, {
		method: 'PUT',
		headers: { 'Idempotency-Key': idempotencyKey },
		body: JSON.stringify(putDummyRiotAccountInputSchema.parse(input))
	});
}

export function deleteDummyRiotAccount(
	userId: string,
	idempotencyKey: string = globalThis.crypto.randomUUID()
) {
	return requestDevData(apiRoute(['dev', 'dummy-users', userId, 'riot-account']), dummyUserSchema, {
		method: 'DELETE',
		headers: { 'Idempotency-Key': idempotencyKey },
		body: JSON.stringify({})
	});
}

export function fetchDummyUserDeletionImpact(userId: string) {
	return requestDevData(
		apiRoute(['dev', 'dummy-users', userId, 'deletion-impact']),
		dummyUserDeletionImpactSchema
	);
}

export function deleteDummyUser(
	userId: string,
	idempotencyKey: string = globalThis.crypto.randomUUID()
) {
	return requestDevData(apiRoute(['dev', 'dummy-users', userId]), dummyUserDeletionResultSchema, {
		method: 'DELETE',
		headers: { 'Idempotency-Key': idempotencyKey },
		body: JSON.stringify({})
	});
}
