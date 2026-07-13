import { afterEach, describe, expect, it, vi } from 'vitest';

import { MonkeyBotApiClient, MonkeyBotApiContractError, MonkeyBotApiError } from './client.js';
import type {
	Actor,
	DiscordCatalogSyncRequest,
	RecruitmentSnapshot,
	SetupGuildData
} from './contracts.js';

const guildId = '200000000000000001';
const userId = '100000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const requestId = '01900000-0000-7000-8000-000000000001';
const serviceToken = 's'.repeat(32);

const actor: Actor = {
	id: userId,
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
		revision: 3,
		channelId: null,
		messageId: null,
		updatedAt: '2026-07-13T10:00:00Z'
	},
	participants: [{ id: userId, displayName: 'Longcat', preferredLanes: ['MID'] }],
	links: { entry: `/g/${guildId}/recruitments/${recruitmentId}/entry` }
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
			return String(input).endsWith('/internal/guilds/200000000000000001/setup')
				? envelope(setupData)
				: envelope(snapshot);
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
				seasonName: '시즌 1'
			},
			context
		);
		await client.createRecruitment(guildId, { actor }, context);
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
	});

	it('rejects success envelopes with unknown fields', async () => {
		const fetchMock = vi.fn(async () =>
			Response.json({ data: snapshot, meta: { requestId }, unexpected: true })
		);
		const client = createClient(fetchMock);

		await expect(
			client.createRecruitment(guildId, { actor }, { idempotencyKey: '400000000000000002' })
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
			{ actor },
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
