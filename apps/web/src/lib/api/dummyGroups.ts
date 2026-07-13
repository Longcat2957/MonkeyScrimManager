import {
	createDummyGroupInputSchema,
	dummyDeleteResultSchema,
	dummyGroupSchema,
	dummyGroupsListDataSchema,
	type CreateDummyGroupInput
} from '$lib/contracts/dummyUsers';
import { apiRoute } from '$lib/routing';
import { requestDevData } from './devTools';

export function fetchDummyGroups() {
	return requestDevData(apiRoute(['dev', 'dummy-groups']), dummyGroupsListDataSchema);
}

export function createDummyGroup(
	input: CreateDummyGroupInput,
	idempotencyKey: string = globalThis.crypto.randomUUID()
) {
	return requestDevData(apiRoute(['dev', 'dummy-groups']), dummyGroupSchema, {
		method: 'POST',
		headers: { 'Idempotency-Key': idempotencyKey },
		body: JSON.stringify(createDummyGroupInputSchema.parse(input))
	});
}

export function updateDummyGroup(
	groupId: string,
	input: {
		name?: string;
		description?: string | null;
		expectedRevision: number;
	},
	idempotencyKey: string = globalThis.crypto.randomUUID()
) {
	return requestDevData(apiRoute(['dev', 'dummy-groups', groupId]), dummyGroupSchema, {
		method: 'PATCH',
		headers: { 'Idempotency-Key': idempotencyKey },
		body: JSON.stringify(input)
	});
}

export function replaceDummyGroupMembers(
	groupId: string,
	memberIds: string[],
	expectedRevision: number,
	idempotencyKey: string = globalThis.crypto.randomUUID()
) {
	return requestDevData(apiRoute(['dev', 'dummy-groups', groupId, 'members']), dummyGroupSchema, {
		method: 'PUT',
		headers: { 'Idempotency-Key': idempotencyKey },
		body: JSON.stringify({ memberIds, expectedRevision })
	});
}

export function deleteDummyGroup(
	groupId: string,
	idempotencyKey: string = globalThis.crypto.randomUUID()
) {
	return requestDevData(apiRoute(['dev', 'dummy-groups', groupId]), dummyDeleteResultSchema, {
		method: 'DELETE',
		headers: { 'Idempotency-Key': idempotencyKey },
		body: JSON.stringify({})
	});
}
