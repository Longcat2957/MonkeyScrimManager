import { afterEach, describe, expect, it, vi } from 'vitest';
import type { GuildAdminMembersSnapshot } from '$lib/contracts/workflow';
import {
	adjustGuildMemberMmr,
	fetchGuildAdminMembers,
	updateGuildMemberStatus
} from './guildAdminMembers';

const guildId = '1525851181157580861';
const memberId = '1525847677542207548';
const requestId = '01900000-0000-7000-8000-000000000001';
const idempotencyKey = '01900000-0000-7000-8000-000000000002';
const snapshot: GuildAdminMembersSnapshot = {
	revision: 4,
	updatedAt: '2026-07-13T03:00:00Z',
	updatedBy: memberId,
	members: [
		{
			id: memberId,
			identityKind: 'DISCORD',
			discordName: 'Longcat',
			riotId: 'Longcat#KR1',
			role: 'OPERATOR',
			mainLane: null,
			laneMmr: { TOP: 1500, JUNGLE: 1618, MID: 1500, BOTTOM: 1500, SUPPORT: 1500 },
			registered: true,
			status: 'ACTIVE',
			lastActiveAt: null
		}
	]
};

function envelope(data: GuildAdminMembersSnapshot = snapshot): Response {
	return Response.json({ data, meta: { requestId } });
}

function fetchMock(response: () => Response) {
	return vi.fn(async (...args: Parameters<typeof globalThis.fetch>): Promise<Response> => {
		void args;
		return response();
	});
}

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('guild admin members API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads the strict authoritative member snapshot', async () => {
		const fetcher = fetchMock(() => envelope());
		installBrowser(fetcher);

		await expect(fetchGuildAdminMembers(guildId)).resolves.toEqual(snapshot);
		expect(fetcher).toHaveBeenCalledWith(
			`/api/guilds/${guildId}/members`,
			expect.objectContaining({ credentials: 'same-origin' })
		);
	});

	it('treats a pre-identity-kind member response as a Discord identity during rolling reloads', async () => {
		const legacyMember = { ...snapshot.members[0] } as Record<string, unknown>;
		delete legacyMember.identityKind;
		const fetcher = fetchMock(() =>
			Response.json({
				data: { ...snapshot, members: [legacyMember] },
				meta: { requestId }
			})
		);
		installBrowser(fetcher);

		await expect(fetchGuildAdminMembers(guildId)).resolves.toMatchObject({
			members: [{ identityKind: 'DISCORD' }]
		});
	});

	it('sends an MMR command with CSRF and a browser idempotency key', async () => {
		const fetcher = fetchMock(() => envelope({ ...snapshot, revision: 5 }));
		installBrowser(fetcher);

		await expect(
			adjustGuildMemberMmr(guildId, {
				kind: 'ADJUST_MMR',
				idempotencyKey,
				memberId,
				lane: 'JUNGLE',
				delta: 15,
				note: '누락 경기 반영',
				expectedRevision: 4
			})
		).resolves.toMatchObject({ revision: 5 });

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/members/${memberId}/mmr`);
		expect(init).toMatchObject({ method: 'PATCH', credentials: 'same-origin' });
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
		expect(JSON.parse(String(init?.body))).toEqual({
			lane: 'JUNGLE',
			delta: 15,
			note: '누락 경기 반영',
			expectedRevision: 4
		});
	});

	it('sends only the status command fields and returns the refreshed snapshot', async () => {
		const authoritative = {
			...snapshot,
			revision: 5,
			members: [{ ...snapshot.members[0]!, status: 'INACTIVE' as const }]
		};
		const fetcher = fetchMock(() => envelope(authoritative));
		installBrowser(fetcher);

		await expect(
			updateGuildMemberStatus(guildId, {
				kind: 'SET_MEMBER_STATUS',
				memberId,
				status: 'INACTIVE',
				expectedRevision: 4
			})
		).resolves.toEqual(authoritative);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/members/${memberId}/status`);
		expect(JSON.parse(String(init?.body))).toEqual({ status: 'INACTIVE', expectedRevision: 4 });
		expect(new Headers(init?.headers).has('Idempotency-Key')).toBe(false);
	});

	it('preserves a revision conflict and rejects malformed success data', async () => {
		const conflict = fetchMock(() =>
			Response.json(
				{
					error: {
						code: 'revision_conflict',
						message: '다른 관리자가 먼저 변경했습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(conflict);
		await expect(
			updateGuildMemberStatus(guildId, {
				kind: 'SET_MEMBER_STATUS',
				memberId,
				status: 'INACTIVE',
				expectedRevision: 3
			})
		).rejects.toMatchObject({ status: 409, code: 'revision_conflict' });

		const malformed = fetchMock(() =>
			Response.json({
				data: { ...snapshot, members: [{ ...snapshot.members[0], laneMmr: {} }] },
				meta: { requestId }
			})
		);
		vi.stubGlobal('fetch', malformed);
		await expect(fetchGuildAdminMembers(guildId)).rejects.toMatchObject({
			status: 200,
			code: 'invalid_response'
		});
	});

	it('fails before mutation when the CSRF cookie is absent', async () => {
		const fetcher = fetchMock(() => envelope());
		installBrowser(fetcher, 'other=value');

		await expect(
			updateGuildMemberStatus(guildId, {
				kind: 'SET_MEMBER_STATUS',
				memberId,
				status: 'INACTIVE',
				expectedRevision: 4
			})
		).rejects.toMatchObject({ status: 0, code: 'csrf_token_missing' });
		expect(fetcher).not.toHaveBeenCalled();
	});

	it('normalizes transport failures without exposing connection details', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			throw new Error('connect ECONNREFUSED password=secret');
		});
		installBrowser(fetcher);

		await expect(fetchGuildAdminMembers(guildId)).rejects.toMatchObject({
			message: 'API 서버에 연결할 수 없습니다.',
			status: 0,
			code: 'network_error'
		});
	});
});
