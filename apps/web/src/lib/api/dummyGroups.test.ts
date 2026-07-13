import { afterEach, describe, expect, it, vi } from 'vitest';
import { dummyGroupFixture, dummyUserFixture } from '$lib/contracts/dummyUsers.testFixtures';
import {
	createDummyGroup,
	deleteDummyGroup,
	fetchDummyGroups,
	replaceDummyGroupMembers,
	updateDummyGroup
} from './dummyGroups';

const requestId = '01900000-0000-7000-8000-000000000099';
const mutationKeys = {
	create: '01900000-0000-7000-8000-000000000201',
	update: '01900000-0000-7000-8000-000000000202',
	members: '01900000-0000-7000-8000-000000000203',
	delete: '01900000-0000-7000-8000-000000000204'
} as const;

describe('dummy groups API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('uses direct group mutation envelopes and revision-aware member replacement', async () => {
		const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
			if (init?.method === 'DELETE') {
				return Response.json({ data: { deleted: true }, meta: { requestId } });
			}
			return Response.json({
				data:
					String(input).endsWith('/dummy-groups') && !init?.method
						? { groups: [dummyGroupFixture] }
						: dummyGroupFixture,
				meta: { requestId }
			});
		});
		vi.stubGlobal('document', { cookie: 'monkeybot_csrf=csrf-token' });
		vi.stubGlobal('fetch', fetcher);

		await expect(fetchDummyGroups()).resolves.toEqual({ groups: [dummyGroupFixture] });
		await createDummyGroup(
			{
				name: '20인 경매 풀',
				memberIds: [dummyUserFixture.userId]
			},
			mutationKeys.create
		);
		await updateDummyGroup(
			dummyGroupFixture.id,
			{
				name: '경매 풀 A',
				expectedRevision: 2
			},
			mutationKeys.update
		);
		await replaceDummyGroupMembers(
			dummyGroupFixture.id,
			[dummyUserFixture.userId],
			2,
			mutationKeys.members
		);
		await expect(deleteDummyGroup(dummyGroupFixture.id, mutationKeys.delete)).resolves.toEqual({
			deleted: true
		});

		expect(JSON.parse(String(fetcher.mock.calls[3]![1]?.body))).toEqual({
			memberIds: [dummyUserFixture.userId],
			expectedRevision: 2
		});
		for (const [index, key] of [
			mutationKeys.create,
			mutationKeys.update,
			mutationKeys.members,
			mutationKeys.delete
		].entries()) {
			expect(new Headers(fetcher.mock.calls[index + 1]![1]?.headers).get('Idempotency-Key')).toBe(
				key
			);
		}
		expect(JSON.parse(String(fetcher.mock.calls[4]![1]?.body))).toEqual({});
	});
});
