import { afterEach, describe, expect, it, vi } from 'vitest';
import { dummyUserFixture } from '$lib/contracts/dummyUsers.testFixtures';
import {
	bulkCreateDummyUsers,
	createDummyUser,
	deleteDummyGuildMembership,
	deleteDummyRiotAccount,
	deleteDummyUser,
	fetchDummyGuildCatalog,
	fetchDummyUserDeletionImpact,
	fetchDummyUsers,
	putDummyGuildMembership,
	putDummyChampionPreferences,
	putDummyRiotAccount,
	setDummyUserArchiveState,
	updateDummyUser
} from './dummyUsers';

const requestId = '01900000-0000-7000-8000-000000000099';
const mutationKeys = {
	create: '01900000-0000-7000-8000-000000000101',
	bulk: '01900000-0000-7000-8000-000000000102',
	update: '01900000-0000-7000-8000-000000000103',
	archive: '01900000-0000-7000-8000-000000000104',
	putMembership: '01900000-0000-7000-8000-000000000105',
	deleteMembership: '01900000-0000-7000-8000-000000000106',
	champions: '01900000-0000-7000-8000-000000000107',
	putRiot: '01900000-0000-7000-8000-000000000108',
	deleteRiot: '01900000-0000-7000-8000-000000000109',
	deleteUser: '01900000-0000-7000-8000-000000000110'
} as const;

function response(data: unknown): Response {
	return Response.json({ data, meta: { requestId } });
}

function install(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('dummy users API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads a filtered list and the independent guild catalog', async () => {
		const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
			void init;
			return String(input).endsWith('/catalog')
				? response({
						guilds: [
							{
								guildId: '1525851181157580861',
								displayName: 'MonkeyBot Test',
								status: 'ACTIVE',
								activeSeasonId: null
							}
						]
					})
				: response({ users: [dummyUserFixture], total: 1 });
		});
		install(fetcher);

		await expect(
			fetchDummyUsers({ query: 'mid', state: 'ACTIVE', groupId: dummyUserFixture.groupIds[0] })
		).resolves.toMatchObject({ total: 1 });
		await expect(fetchDummyGuildCatalog()).resolves.toMatchObject({
			guilds: [{ status: 'ACTIVE' }]
		});
		expect(String(fetcher.mock.calls[0]![0])).toContain(
			`query=mid&state=ACTIVE&groupId=${dummyUserFixture.groupIds[0]}`
		);
	});

	it('sends strict create, bulk, edit, archive and guild assignment commands', async () => {
		const fetcher = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
			void init;
			return String(input).endsWith('/bulk')
				? response({ users: [dummyUserFixture], total: 1 })
				: response(dummyUserFixture);
		});
		install(fetcher);

		const createInput = { stableKey: 'mid-player-01', displayName: '테스트 미드 01' };
		await createDummyUser(createInput, mutationKeys.create);
		await bulkCreateDummyUsers([createInput], mutationKeys.bulk);
		await updateDummyUser(
			dummyUserFixture.userId,
			{
				displayName: '변경 이름',
				expectedRevision: 3
			},
			mutationKeys.update
		);
		await setDummyUserArchiveState(dummyUserFixture.userId, 'archive', 3, mutationKeys.archive);
		await putDummyGuildMembership(
			dummyUserFixture.userId,
			'1525851181157580861',
			{
				displayName: '길드 닉네임',
				status: 'ACTIVE',
				mainLane: 'MID'
			},
			mutationKeys.putMembership
		);
		await deleteDummyGuildMembership(
			dummyUserFixture.userId,
			'1525851181157580861',
			mutationKeys.deleteMembership
		);
		await putDummyChampionPreferences(
			dummyUserFixture.userId,
			'MID',
			['Ahri'],
			mutationKeys.champions
		);

		for (const [, init] of fetcher.mock.calls) {
			const headers = new Headers(init?.headers);
			expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
			expect(init).toMatchObject({ credentials: 'same-origin' });
		}
		expect(JSON.parse(String(fetcher.mock.calls[2]![1]?.body))).toEqual({
			displayName: '변경 이름',
			expectedRevision: 3
		});
		for (const [index, key] of [
			mutationKeys.create,
			mutationKeys.bulk,
			mutationKeys.update,
			mutationKeys.archive,
			mutationKeys.putMembership,
			mutationKeys.deleteMembership,
			mutationKeys.champions
		].entries()) {
			expect(new Headers(fetcher.mock.calls[index]![1]?.headers).get('Idempotency-Key')).toBe(key);
		}
		expect(String(fetcher.mock.calls[4]![0])).toContain(
			`/dummy-users/${dummyUserFixture.userId}/guilds/1525851181157580861`
		);
	});

	it('preserves backend reason and request ID and blocks mutations without CSRF', async () => {
		const denied = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'dummy_user_conflict',
						message: '같은 stable key가 이미 존재합니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		install(denied);
		await expect(
			createDummyUser({ stableKey: 'mid-player-01', displayName: '중복 사용자' })
		).rejects.toMatchObject({
			code: 'dummy_user_conflict',
			requestId,
			message: '같은 stable key가 이미 존재합니다.'
		});

		install(vi.fn(), 'other=value');
		await expect(
			createDummyUser({ stableKey: 'new-user', displayName: '신규 사용자' })
		).rejects.toMatchObject({ code: 'csrf_token_missing' });
	});

	it('upserts and removes an explicitly synthetic Riot account', async () => {
		const withRiot = {
			...dummyUserFixture,
			riotAccount: {
				platform: 'KR',
				routingRegion: 'ASIA',
				gameName: 'Dummy Mid',
				tagLine: 'DUMMY',
				profileIconId: 29,
				mainPosition: 'MID' as const
			}
		};
		const fetcher = vi
			.fn<typeof globalThis.fetch>()
			.mockResolvedValueOnce(response(withRiot))
			.mockResolvedValueOnce(response(dummyUserFixture));
		install(fetcher);

		await expect(
			putDummyRiotAccount(
				dummyUserFixture.userId,
				{
					platform: 'KR',
					routingRegion: 'ASIA',
					gameName: 'Dummy Mid',
					tagLine: 'DUMMY',
					profileIconId: 29,
					mainPosition: 'MID'
				},
				mutationKeys.putRiot
			)
		).resolves.toEqual(withRiot);
		await expect(
			deleteDummyRiotAccount(dummyUserFixture.userId, mutationKeys.deleteRiot)
		).resolves.toEqual(dummyUserFixture);
		expect(fetcher.mock.calls[0]![1]?.method).toBe('PUT');
		expect(fetcher.mock.calls[1]![1]?.method).toBe('DELETE');
		expect(String(fetcher.mock.calls[0]![0])).toContain('/riot-account');
		expect(new Headers(fetcher.mock.calls[0]![1]?.headers).get('Idempotency-Key')).toBe(
			mutationKeys.putRiot
		);
		expect(new Headers(fetcher.mock.calls[1]![1]?.headers).get('Idempotency-Key')).toBe(
			mutationKeys.deleteRiot
		);
	});

	it('previews deletion impact before deleting an unreferenced dummy identity', async () => {
		const impact = {
			canDelete: true,
			blockingReferences: {
				guildMemberships: 0,
				recruitmentParticipants: 0,
				auctionParticipants: 0,
				gameParticipants: 0,
				userSessions: 0,
				discordIdentities: 0,
				realRiotAccounts: 0
			},
			removableReferences: {
				groupMemberships: 1,
				championPreferences: 3,
				syntheticRiotAccounts: 1
			}
		};
		const fetcher = vi
			.fn<typeof globalThis.fetch>()
			.mockResolvedValueOnce(response(impact))
			.mockResolvedValueOnce(response({ deleted: true, impact }));
		install(fetcher);

		await expect(fetchDummyUserDeletionImpact(dummyUserFixture.userId)).resolves.toEqual(impact);
		await expect(
			deleteDummyUser(dummyUserFixture.userId, mutationKeys.deleteUser)
		).resolves.toEqual({
			deleted: true,
			impact
		});
		expect(String(fetcher.mock.calls[0]![0])).toContain('/deletion-impact');
		expect(fetcher.mock.calls[1]![1]?.method).toBe('DELETE');
		expect(new Headers(fetcher.mock.calls[1]![1]?.headers).get('Idempotency-Key')).toBe(
			mutationKeys.deleteUser
		);
	});
});
