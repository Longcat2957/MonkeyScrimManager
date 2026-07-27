import { afterEach, describe, expect, it, vi } from 'vitest';

import { MonkeyBotApiClient, MonkeyBotApiContractError, MonkeyBotApiError } from './client.js';
import type {
	Actor,
	AuctionRecruitmentSnapshot,
	DiscordCatalogSyncRequest,
	RecruitmentSnapshot,
	SetupGuildData
} from './contracts.js';

const guildId = '200000000000000001';
const userId = '100000000000000001';
const dummyUserId = 'dmy_01900000-0000-7000-8000-000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const auctionRecruitmentId = '01900000-0000-7000-8000-000000000102';
const auctionTournamentId = '01900000-0000-7000-8000-000000000103';
const requestId = '01900000-0000-7000-8000-000000000001';
const serviceToken = 's'.repeat(32);
const notificationId = '01900000-0000-7000-8000-000000000301';
const leaseToken = '01900000-0000-7000-8000-000000000302';
const expectedStartAt = '2026-07-26T11:00:00Z';
const expectedStartLabel = '2026-07-26 20:00 (KST)';

const actor: Actor = {
	id: userId,
	globalDisplayName: 'Longcat',
	displayName: 'Longcat',
	avatarHash: null,
	roleIds: ['300000000000000001']
};

const snapshot: RecruitmentSnapshot = {
	recruitment: {
		id: recruitmentId,
		displayNumber: 214,
		title: '금요 정기 내전',
		status: 'OPEN',
		targetCount: 10,
		bestOf: 3,
		hardFearless: false,
		revision: 3,
		createdById: userId,
		channelId: null,
		messageId: null,
		updatedAt: '2026-07-13T10:00:00Z'
	},
	participants: [
		{ id: userId, displayName: 'Longcat', preferredLanes: ['MID'] },
		{ id: dummyUserId, displayName: 'Dummy Jungle', preferredLanes: ['JUNGLE'] }
	],
	links: { entry: `/g/${guildId}/recruitments/${recruitmentId}/entry` }
};

const auctionSnapshot: AuctionRecruitmentSnapshot = {
	recruitment: {
		id: auctionRecruitmentId,
		displayNumber: 215,
		title: `20인 경매 내전 모집 #215 · 예상 시작 ${expectedStartLabel}`,
		format: 'TWENTY_PERSON',
		rules: { hardFearless: false, semifinalBestOf: 3, finalBestOf: 3 },
		participantCount: 20,
		joinedParticipantCount: 0,
		teamCount: 4,
		status: 'OPEN',
		revision: 0,
		channelId: null,
		messageId: null,
		createdById: userId,
		updatedAt: '2026-07-14T10:00:00Z',
		href: `/g/${guildId}/auction/recruitments/${auctionRecruitmentId}`
	},
	participants: [],
	links: { draft: null },
	permissions: { canManageMembers: false, canClose: false, canCancel: true }
};

const auctionTournamentCancelPreview = {
	tournament: {
		id: auctionTournamentId,
		displayNumber: 216,
		title: '20인 경매 내전 #216',
		status: 'IN_GAME' as const,
		revision: 9
	}
};

const auctionTournamentCancelResult = {
	tournamentId: auctionTournamentId,
	tournamentStatus: 'CANCELLED' as const,
	tournamentRevision: 10,
	sourceRecruitmentId: auctionRecruitmentId,
	sourceRecruitmentRevision: 7,
	reversedGameCount: 2
};

const auctionRecruitmentForceCancelResult = {
	recruitmentId: auctionRecruitmentId,
	recruitmentStatus: 'CANCELLED' as const,
	recruitmentRevision: 8,
	tournamentId: auctionTournamentId,
	tournamentStatus: 'CANCELLED' as const,
	tournamentRevision: 10,
	reversedGameCount: 2
};

const auctionTournamentList = {
	tournaments: [
		{
			id: auctionTournamentId,
			displayNumber: 216,
			title: '20인 경매 내전 #216',
			format: 'TWENTY_PERSON' as const,
			participantCount: 20 as const,
			teamCount: 4 as const,
			status: 'IN_GAME' as const,
			revision: 9,
			activeMemberCount: 20,
			bidMemberCount: 20,
			incompleteMatchCount: 2,
			nextMatch: {
				id: '01900000-0000-7000-8000-000000000104',
				label: '준결승 A',
				matchKey: 'SEMIFINAL_A' as const,
				status: 'READY' as const,
				confirmedGameCount: 0
			},
			championTeam: null,
			updatedAt: '2026-07-15T10:00:00Z',
			endedAt: null,
			href: `/g/${guildId}/auction/matches/01900000-0000-7000-8000-000000000104`
		}
	],
	nextCursor: null
};

const setupData: SetupGuildData = {
	guild: {
		id: guildId,
		displayName: 'MSM Seoul',
		operatorRoleId: '300000000000000001',
		currentSeason: { id: '01900000-0000-7000-8000-000000000201', name: '시즌 1' },
		dashboardHref: `/g/${guildId}`
	}
};

const registrationData = {
	member: { id: userId, displayName: 'MSM Longcat', status: 'ACTIVE' as const },
	riotAccount: {
		puuid: 'riot-puuid',
		gameName: 'Longcat',
		tagLine: 'KR1',
		platform: 'KR' as const,
		isMain: true
	},
	registration: 'CREATED' as const,
	accountLink: 'CREATED' as const
};

const catalog: DiscordCatalogSyncRequest = {
	roles: [
		{ id: '300000000000000001', name: '내전 운영진', position: 3, managed: false },
		{ id: '300000000000000002', name: '연동 역할', position: 2, managed: true }
	],
	channels: [
		{
			id: '500000000000000001',
			name: '내전-모집',
			parentName: '내전',
			position: 1,
			kind: 'TEXT'
		},
		{
			id: '500000000000000002',
			name: '공지',
			parentName: null,
			position: 2,
			kind: 'ANNOUNCEMENT'
		}
	]
};

function envelope(data: unknown): Response {
	return Response.json({ data, meta: { requestId } });
}

function createClient(fetchMock: ReturnType<typeof vi.fn>, timeoutMs = 1_000) {
	return new MonkeyBotApiClient({
		baseUrl: 'http://api.local:8080',
		serviceToken,
		timeoutMs,
		fetch: fetchMock as typeof globalThis.fetch
	});
}

describe('MonkeyBotApiClient', () => {
	afterEach(() => {
		vi.useRealTimers();
	});

	it('uses canonical paths, service bearer, and interaction idempotency keys', async () => {
		const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
			void init;
			const path = new URL(String(input)).pathname;
			if (path.endsWith('/setup')) return envelope(setupData);
			if (path.endsWith('/registrations')) return envelope(registrationData);
			return envelope(snapshot);
		});
		const client = createClient(fetchMock);
		const context = { idempotencyKey: '400000000000000001' };

		await client.setupGuild(
			guildId,
			{
				actor,
				guildName: 'MSM Seoul',
				guildIconHash: null,
				operatorRoleId: '300000000000000001',
				recruitmentChannelId: '500000000000000001',
				entryChannelId: null,
				resultChannelId: '500000000000000002',
				logChannelId: null,
				seasonName: '시즌 1'
			},
			context
		);
		await client.registerGuildMember(
			guildId,
			{ actor, gameName: 'Longcat', tagLine: 'KR1', platform: 'KR' },
			context
		);
		await client.createRecruitment(
			guildId,
			{ actor, bestOf: 3, hardFearless: false, expectedStartAt },
			context
		);
		await client.bindRecruitmentMessage(
			guildId,
			recruitmentId,
			{ actor, channelId: '500000000000000001', messageId: '600000000000000001' },
			context
		);
		await client.joinRecruitment(guildId, recruitmentId, { actor, member: actor }, context);
		await client.leaveRecruitment(guildId, recruitmentId, userId, { actor }, context);
		await client.setPreferredLanes(
			guildId,
			recruitmentId,
			userId,
			{ actor, preferredLanes: ['MID', 'JUNGLE'] },
			context
		);
		await client.startEntry(guildId, recruitmentId, { actor }, context);

		expect(
			fetchMock.mock.calls.map(([input, init]) => [String(input), (init as RequestInit).method])
		).toEqual([
			[`http://api.local:8080/internal/guilds/${guildId}/setup`, 'POST'],
			[`http://api.local:8080/internal/guilds/${guildId}/registrations`, 'POST'],
			[`http://api.local:8080/api/guilds/${guildId}/recruitments`, 'POST'],
			[`http://api.local:8080/api/guilds/${guildId}/recruitments/${recruitmentId}/message`, 'PUT'],
			[`http://api.local:8080/api/guilds/${guildId}/recruitments/${recruitmentId}/members`, 'POST'],
			[
				`http://api.local:8080/api/guilds/${guildId}/recruitments/${recruitmentId}/members/${userId}`,
				'DELETE'
			],
			[
				`http://api.local:8080/api/guilds/${guildId}/recruitments/${recruitmentId}/members/${userId}/preferred-lanes`,
				'PUT'
			],
			[`http://api.local:8080/api/guilds/${guildId}/recruitments/${recruitmentId}/close`, 'POST']
		]);

		for (const [, init] of fetchMock.mock.calls) {
			const headers = new Headers((init as RequestInit).headers);
			expect(headers.get('authorization')).toBe(`Bearer ${serviceToken}`);
			expect(headers.get('idempotency-key')).toBe(context.idempotencyKey);
			expect(headers.get('content-type')).toBe('application/json');
		}
		expect(JSON.parse(String((fetchMock.mock.calls[0]?.[1] as RequestInit).body))).toEqual({
			actor,
			guildName: 'MSM Seoul',
			guildIconHash: null,
			operatorRoleId: '300000000000000001',
			recruitmentChannelId: '500000000000000001',
			entryChannelId: null,
			resultChannelId: '500000000000000002',
			logChannelId: null,
			seasonName: '시즌 1'
		});
		expect(JSON.parse(String((fetchMock.mock.calls[2]?.[1] as RequestInit).body))).toEqual({
			actor,
			bestOf: 3,
			hardFearless: false,
			expectedStartAt
		});
	});

	it('rejects success envelopes with unknown fields', async () => {
		const fetchMock = vi.fn(async () =>
			Response.json({ data: snapshot, meta: { requestId }, unexpected: true })
		);
		const client = createClient(fetchMock);

		await expect(
			client.createRecruitment(
				guildId,
				{ actor, bestOf: 3, hardFearless: false, expectedStartAt },
				{ idempotencyKey: '400000000000000002' }
			)
		).rejects.toBeInstanceOf(MonkeyBotApiContractError);
	});

	it('creates an auction recruitment with the bot actor and interaction idempotency key', async () => {
		const fetchMock = vi.fn<typeof globalThis.fetch>(async (_input, _init) => {
			void _input;
			void _init;
			return envelope(auctionSnapshot);
		});
		const client = createClient(fetchMock);

		await expect(
			client.createAuctionRecruitment(
				guildId,
				{
					actor,
					format: 'TWENTY_PERSON',
					semifinalBestOf: 1,
					finalBestOf: 5,
					hardFearless: true,
					expectedStartAt
				},
				{ idempotencyKey: '400000000000000007' }
			)
		).resolves.toEqual(auctionSnapshot);

		const [input, init] = fetchMock.mock.calls[0] ?? [];
		expect(String(input)).toBe(`http://api.local:8080/api/guilds/${guildId}/auction-recruitments`);
		expect((init as RequestInit).method).toBe('POST');
		expect(JSON.parse(String((init as RequestInit).body))).toEqual({
			actor,
			format: 'TWENTY_PERSON',
			semifinalBestOf: 1,
			finalBestOf: 5,
			hardFearless: true,
			expectedStartAt
		});
		const headers = new Headers((init as RequestInit).headers);
		expect(headers.get('idempotency-key')).toBe('400000000000000007');
	});

	it('joins and leaves an auction recruitment with the canonical member routes', async () => {
		const fetchMock = vi.fn<typeof globalThis.fetch>(async (_input, _init) => {
			void _input;
			void _init;
			return envelope(auctionSnapshot);
		});
		const client = createClient(fetchMock);
		const context = { idempotencyKey: '400000000000000008' };

		await client.joinAuctionRecruitment(
			guildId,
			auctionRecruitmentId,
			{ actor, declaredLane: 'JUNGLE' },
			context
		);
		await client.leaveAuctionRecruitment(guildId, auctionRecruitmentId, userId, { actor }, context);

		expect(
			fetchMock.mock.calls.map(([input, init]) => [String(input), (init as RequestInit).method])
		).toEqual([
			[
				`http://api.local:8080/api/guilds/${guildId}/auction-recruitments/${auctionRecruitmentId}/members`,
				'POST'
			],
			[
				`http://api.local:8080/api/guilds/${guildId}/auction-recruitments/${auctionRecruitmentId}/members/${userId}`,
				'DELETE'
			]
		]);
		expect(JSON.parse(String((fetchMock.mock.calls[0]?.[1] as RequestInit).body))).toEqual({
			actor,
			declaredLane: 'JUNGLE'
		});
		expect(JSON.parse(String((fetchMock.mock.calls[1]?.[1] as RequestInit).body))).toEqual({
			actor
		});
		for (const [, init] of fetchMock.mock.calls) {
			const headers = new Headers((init as RequestInit).headers);
			expect(headers.get('idempotency-key')).toBe(context.idempotencyKey);
		}
	});

	it('force-adds a selected guild member through the canonical auction member route', async () => {
		const fetchMock = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => {
			void _input;
			void _init;
			return envelope(auctionSnapshot);
		});
		const client = createClient(fetchMock);
		const context = { idempotencyKey: '400000000000000010' };
		const member = {
			id: '100000000000000002',
			globalDisplayName: 'Target Player',
			displayName: 'Target Player (MSM)',
			avatarHash: null,
			roleIds: ['300000000000000002']
		};

		await expect(
			client.forceAddAuctionRecruitmentMember(
				guildId,
				auctionRecruitmentId,
				{ actor, member, declaredLane: 'JUNGLE' },
				context
			)
		).resolves.toEqual(auctionSnapshot);

		const [input, init] = fetchMock.mock.calls[0] ?? [];
		expect(String(input)).toBe(
			`http://api.local:8080/api/guilds/${guildId}/auction-recruitments/${auctionRecruitmentId}/members`
		);
		expect((init as RequestInit).method).toBe('POST');
		expect(JSON.parse(String((init as RequestInit).body))).toEqual({
			actor,
			member,
			declaredLane: 'JUNGLE'
		});
	});

	it('lists auction tournaments with service authentication and query filters', async () => {
		const fetchMock = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => {
			void _input;
			void _init;
			return envelope(auctionTournamentList);
		});
		const client = createClient(fetchMock);
		const seasonId = '01900000-0000-7000-8000-000000000201';

		await expect(
			client.listAuctionTournaments(guildId, { state: 'active', seasonId, limit: 5 })
		).resolves.toEqual(auctionTournamentList);

		const [input, init] = fetchMock.mock.calls[0] ?? [];
		expect(String(input)).toBe(
			`http://api.local:8080/api/guilds/${guildId}/auction-tournaments?state=active&seasonId=${seasonId}&limit=5`
		);
		const headers = new Headers((init as RequestInit).headers);
		expect(headers.get('authorization')).toBe(`Bearer ${serviceToken}`);
		expect((init as RequestInit).method).toBe('GET');
	});

	it('force-cancels an auction recruitment with an idempotent service request', async () => {
		const fetchMock = vi.fn(async (_input: RequestInfo | URL, _init?: RequestInit) => {
			void _input;
			void _init;
			return envelope(auctionRecruitmentForceCancelResult);
		});
		const client = createClient(fetchMock);
		const context = { idempotencyKey: '400000000000000011' };

		await expect(
			client.forceCancelAuctionRecruitment(guildId, auctionRecruitmentId, { actor }, context)
		).resolves.toEqual(auctionRecruitmentForceCancelResult);

		const [input, init] = fetchMock.mock.calls[0] ?? [];
		expect(String(input)).toBe(
			`http://api.local:8080/api/guilds/${guildId}/auction-recruitments/${auctionRecruitmentId}/force-cancel`
		);
		expect((init as RequestInit).method).toBe('POST');
		expect(JSON.parse(String((init as RequestInit).body))).toEqual({ actor });
		const headers = new Headers((init as RequestInit).headers);
		expect(headers.get('idempotency-key')).toBe(context.idempotencyKey);
	});

	it('reads the cancellation revision before cancelling an auction tournament', async () => {
		const fetchMock = vi.fn(async (input: RequestInfo | URL, _init?: RequestInit) => {
			void _init;
			const path = new URL(String(input)).pathname;
			return envelope(
				path.endsWith('/cancel-preview')
					? auctionTournamentCancelPreview
					: auctionTournamentCancelResult
			);
		});
		const client = createClient(fetchMock);

		await expect(
			client.getAuctionTournamentCancelPreview(guildId, auctionTournamentId)
		).resolves.toEqual(auctionTournamentCancelPreview);
		await expect(
			client.cancelAuctionTournament(
				guildId,
				auctionTournamentId,
				{ actor, expectedRevision: 9, reason: '운영 일정 변경' },
				{ idempotencyKey: '400000000000000009' }
			)
		).resolves.toEqual(auctionTournamentCancelResult);

		expect(
			fetchMock.mock.calls.map(([input, init]) => [String(input), (init as RequestInit).method])
		).toEqual([
			[
				`http://api.local:8080/api/guilds/${guildId}/auction-tournaments/${auctionTournamentId}/cancel-preview`,
				'GET'
			],
			[
				`http://api.local:8080/api/guilds/${guildId}/auction-tournaments/${auctionTournamentId}/cancel`,
				'POST'
			]
		]);

		const previewHeaders = new Headers((fetchMock.mock.calls[0]?.[1] as RequestInit).headers);
		expect(previewHeaders.get('authorization')).toBe(`Bearer ${serviceToken}`);
		expect(previewHeaders.has('content-type')).toBe(false);
		expect(previewHeaders.has('idempotency-key')).toBe(false);

		const cancelInit = fetchMock.mock.calls[1]?.[1] as RequestInit;
		const cancelHeaders = new Headers(cancelInit.headers);
		expect(cancelHeaders.get('authorization')).toBe(`Bearer ${serviceToken}`);
		expect(cancelHeaders.get('idempotency-key')).toBe('400000000000000009');
		expect(JSON.parse(String(cancelInit.body))).toEqual({
			actor,
			expectedRevision: 9,
			reason: '운영 일정 변경'
		});
	});

	it('preserves a cancellation-preview API error for the interaction error handler', async () => {
		const fetchMock = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'auction_tournament_not_found',
						message: '요청한 경매 내전을 찾을 수 없습니다.',
						requestId
					}
				},
				{ status: 404 }
			)
		);
		const client = createClient(fetchMock);

		const error = await client
			.getAuctionTournamentCancelPreview(guildId, auctionTournamentId)
			.catch((caught: unknown) => caught);

		expect(error).toBeInstanceOf(MonkeyBotApiError);
		expect(error).toMatchObject({
			status: 404,
			code: 'auction_tournament_not_found',
			message: '요청한 경매 내전을 찾을 수 없습니다.',
			requestId
		});
	});

	it('accepts canonical dummy participant IDs but rejects malformed internal IDs', async () => {
		const validFetch = vi.fn(async () => envelope(snapshot));
		const client = createClient(validFetch);

		await expect(
			client.createRecruitment(
				guildId,
				{ actor, bestOf: 3, hardFearless: false, expectedStartAt },
				{ idempotencyKey: '400000000000000005' }
			)
		).resolves.toMatchObject({
			participants: [
				{ id: userId, displayName: 'Longcat' },
				{ id: dummyUserId, displayName: 'Dummy Jungle' }
			]
		});

		const invalidSnapshot = structuredClone(snapshot);
		invalidSnapshot.participants[1]!.id = 'dmy_not-a-uuid';
		const invalidClient = createClient(vi.fn(async () => envelope(invalidSnapshot)));
		await expect(
			invalidClient.createRecruitment(
				guildId,
				{ actor, bestOf: 3, hardFearless: false, expectedStartAt },
				{ idempotencyKey: '400000000000000006' }
			)
		).rejects.toBeInstanceOf(MonkeyBotApiContractError);
	});

	it('replaces a Discord catalog without an idempotency header', async () => {
		const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
			void input;
			void init;
			return envelope({ syncedAt: '2026-07-13T12:34:56.000Z' });
		});
		const client = createClient(fetchMock);

		await expect(client.syncDiscordCatalog(guildId, catalog)).resolves.toEqual({
			syncedAt: '2026-07-13T12:34:56.000Z'
		});

		const [input, init] = fetchMock.mock.calls[0] ?? [];
		expect(String(input)).toBe(`http://api.local:8080/internal/guilds/${guildId}/discord-catalog`);
		expect((init as RequestInit).method).toBe('PUT');
		expect(JSON.parse(String((init as RequestInit).body))).toEqual(catalog);
		const headers = new Headers((init as RequestInit).headers);
		expect(headers.get('authorization')).toBe(`Bearer ${serviceToken}`);
		expect(headers.get('content-type')).toBe('application/json');
		expect(headers.has('idempotency-key')).toBe(false);
	});

	it('uses internal notification routes with the typed contracts and no idempotency header', async () => {
		const notifications = [
			{
				id: notificationId,
				guildId,
				leaseToken,
				leaseUntil: '2026-07-13T10:00:30Z',
				channelId: '500000000000000001',
				type: 'RECRUITMENT_MESSAGE_CREATE',
				payload: { schemaVersion: 1, recruitmentId, snapshot },
				discordNonce: 'abcdefghijklmnopqrstuv',
				attempts: 1
			}
		];
		const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
			void init;
			const path = new URL(String(input)).pathname;
			if (path.endsWith('/claim')) return envelope({ notifications });
			if (path.endsWith('/recruitment-message/ack')) {
				return envelope({
					acknowledged: true,
					deliveredAt: '2026-07-13T10:00:01Z',
					recruitment: { id: recruitmentId, revision: 4 }
				});
			}
			if (path.endsWith('/ack')) {
				return envelope({ acknowledged: true, deliveredAt: '2026-07-13T10:00:01Z' });
			}
			return envelope({ state: 'FAILED', failureCode: 'invalid_notification', attempts: 1 });
		});
		const client = createClient(fetchMock);
		const claimInput = { instanceId: 'bot-test', limit: 10 };
		const acknowledgement = {
			instanceId: 'bot-test',
			notificationId,
			leaseToken,
			discordMessageId: '600000000000000001'
		};

		await expect(client.claimNotifications(claimInput)).resolves.toEqual({ notifications });
		await expect(client.acknowledgeNotification(acknowledgement)).resolves.toMatchObject({
			acknowledged: true
		});
		await expect(client.acknowledgeRecruitmentMessage(acknowledgement)).resolves.toMatchObject({
			acknowledged: true,
			recruitment: { id: recruitmentId, revision: 4 }
		});
		await expect(
			client.nackNotification({
				instanceId: 'bot-test',
				notificationId,
				leaseToken,
				failure: { kind: 'HTTP', status: 422, message: 'Unsupported bot notification type' }
			})
		).resolves.toMatchObject({ state: 'FAILED' });

		expect(
			fetchMock.mock.calls.map(([input, init]) => [String(input), (init as RequestInit).method])
		).toEqual([
			['http://api.local:8080/internal/notifications/claim', 'POST'],
			['http://api.local:8080/internal/notifications/ack', 'POST'],
			['http://api.local:8080/internal/notifications/recruitment-message/ack', 'POST'],
			['http://api.local:8080/internal/notifications/nack', 'POST']
		]);
		for (const [, init] of fetchMock.mock.calls) {
			const headers = new Headers((init as RequestInit).headers);
			expect(headers.get('authorization')).toBe(`Bearer ${serviceToken}`);
			expect(headers.get('content-type')).toBe('application/json');
			expect(headers.has('idempotency-key')).toBe(false);
		}
	});

	it('reports durable bot heartbeats with the service bearer and strict contract', async () => {
		const fetchMock = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return envelope({ accepted: true, lastHeartbeatAt: '2026-07-15T12:00:00Z' });
		});
		const client = createClient(fetchMock);

		await expect(
			client.heartbeat({ instanceId: 'bot-main-1', version: '0.1.0', commitSha: 'abc1234' })
		).resolves.toEqual({ accepted: true, lastHeartbeatAt: '2026-07-15T12:00:00Z' });

		expect(fetchMock).toHaveBeenCalledTimes(1);
		const [input, init] = fetchMock.mock.calls[0]!;
		expect(String(input)).toBe('http://api.local:8080/internal/heartbeat');
		expect((init as RequestInit).method).toBe('POST');
		expect(JSON.parse(String((init as RequestInit).body))).toEqual({
			instanceId: 'bot-main-1',
			version: '0.1.0',
			commitSha: 'abc1234'
		});
		const headers = new Headers((init as RequestInit).headers);
		expect(headers.get('authorization')).toBe(`Bearer ${serviceToken}`);
		expect(headers.has('idempotency-key')).toBe(false);
	});

	it('strictly validates Discord catalog request and response limits', async () => {
		const fetchMock = vi.fn(async () => envelope({ syncedAt: 'not-a-timestamp' }));
		const client = createClient(fetchMock);
		const tooManyRoles: DiscordCatalogSyncRequest = {
			roles: Array.from({ length: 251 }, () => catalog.roles[0]!),
			channels: []
		};

		await expect(client.syncDiscordCatalog(guildId, tooManyRoles)).rejects.toBeInstanceOf(
			MonkeyBotApiContractError
		);
		expect(fetchMock).not.toHaveBeenCalled();

		await expect(client.syncDiscordCatalog(guildId, catalog)).rejects.toBeInstanceOf(
			MonkeyBotApiContractError
		);
	});

	it('preserves a valid API error without exposing credentials', async () => {
		const fetchMock = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'conflict',
						message: '현재 모집 revision과 충돌합니다.',
						requestId,
						details: { currentRevision: 4 }
					}
				},
				{ status: 409 }
			)
		);
		const client = createClient(fetchMock);

		const error = await client
			.startEntry(guildId, recruitmentId, { actor }, { idempotencyKey: '400000000000000003' })
			.catch((caught: unknown) => caught);

		expect(error).toBeInstanceOf(MonkeyBotApiError);
		expect(error).toMatchObject({
			status: 409,
			code: 'conflict',
			requestId,
			details: { currentRevision: 4 }
		});
		expect(String(error)).not.toContain(serviceToken);
	});

	it('aborts requests at the configured timeout', async () => {
		vi.useFakeTimers();
		const fetchMock = vi.fn(
			(_input: RequestInfo | URL, init?: RequestInit) =>
				new Promise<Response>((_resolve, reject) => {
					init?.signal?.addEventListener('abort', () => reject(init.signal?.reason), {
						once: true
					});
				})
		);
		const client = createClient(fetchMock, 100);
		const request = client.createRecruitment(
			guildId,
			{ actor, bestOf: 3, hardFearless: false, expectedStartAt },
			{ idempotencyKey: '400000000000000004' }
		);
		const assertion = expect(request).rejects.toMatchObject({
			name: 'MonkeyBotApiTransportError',
			message: 'Bot API request timed out'
		});

		await vi.advanceTimersByTimeAsync(100);
		await assertion;
	});
});
