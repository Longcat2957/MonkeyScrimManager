import { afterEach, describe, expect, it, vi } from 'vitest';
import type { GuildAdminSettingsSnapshot } from '$lib/contracts/workflow';
import { fetchGuildAdminSettings, updateGuildAdminSettings } from './guildAdminSettings';

const guildId = '1525851181157580861';
const operatorId = '1525847677542207548';
const requestId = '01900000-0000-7000-8000-000000000001';
const snapshot: GuildAdminSettingsSnapshot = {
	settings: {
		displayName: 'MonkeyBotTest',
		operatorRoleId: '1525889769761149148',
		recruitmentChannelId: null,
		entryChannelId: '1525900000000000002',
		resultChannelId: null,
		logChannelId: '1525900000000000003',
		isExternallyPublic: false
	},
	revision: 3,
	updatedAt: '2026-07-13T08:00:00Z',
	updatedBy: operatorId,
	catalogUpdatedAt: '2026-07-13T07:59:00Z',
	roles: [{ id: '1525889769761149148', name: '내전 운영진' }],
	channels: [
		{ id: '1525900000000000001', name: '내전-모집', parentName: '내전' },
		{ id: '1525900000000000002', name: '내전-엔트리', parentName: '내전' },
		{ id: '1525900000000000003', name: '운영-로그', parentName: null }
	]
};

function envelope(data: GuildAdminSettingsSnapshot = snapshot): Response {
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

describe('guild admin settings API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads the strict authoritative settings snapshot', async () => {
		const fetcher = fetchMock(() => envelope());
		installBrowser(fetcher);

		await expect(fetchGuildAdminSettings(guildId)).resolves.toEqual(snapshot);
		expect(fetcher).toHaveBeenCalledWith(
			`/api/guilds/${guildId}/settings`,
			expect.objectContaining({ credentials: 'same-origin' })
		);
		expect(new Headers(fetcher.mock.calls[0]![1]?.headers).get('accept')).toBe('application/json');
	});

	it('puts all settings fields including the result channel and expected revision with CSRF', async () => {
		const authoritative: GuildAdminSettingsSnapshot = {
			...snapshot,
			settings: { ...snapshot.settings, resultChannelId: '1525900000000000001' },
			revision: 4
		};
		const fetcher = fetchMock(() => envelope(authoritative));
		installBrowser(fetcher, 'other=value; monkeybot_csrf=csrf-token');

		await expect(
			updateGuildAdminSettings(guildId, {
				kind: 'UPDATE_GUILD_SETTINGS',
				settings: authoritative.settings,
				expectedRevision: 3
			})
		).resolves.toEqual(authoritative);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/settings`);
		expect(init).toMatchObject({ method: 'PUT', credentials: 'same-origin' });
		const headers = new Headers(init?.headers);
		expect(headers.get('accept')).toBe('application/json');
		expect(headers.get('content-type')).toBe('application/json');
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(JSON.parse(String(init?.body))).toEqual({
			...authoritative.settings,
			expectedRevision: 3
		});
	});

	it('preserves backend conflict details and rejects a non-strict success payload', async () => {
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
			updateGuildAdminSettings(guildId, {
				kind: 'UPDATE_GUILD_SETTINGS',
				settings: snapshot.settings,
				expectedRevision: 2
			})
		).rejects.toMatchObject({ status: 409, code: 'revision_conflict' });

		const malformed = fetchMock(() =>
			Response.json({
				data: { ...snapshot, unexpected: true },
				meta: { requestId }
			})
		);
		vi.stubGlobal('fetch', malformed);
		await expect(fetchGuildAdminSettings(guildId)).rejects.toMatchObject({
			status: 200,
			code: 'invalid_response'
		});
	});

	it('fails before mutation when the CSRF cookie is absent', async () => {
		const fetcher = fetchMock(() => envelope());
		installBrowser(fetcher, 'other=value');

		await expect(
			updateGuildAdminSettings(guildId, {
				kind: 'UPDATE_GUILD_SETTINGS',
				settings: snapshot.settings,
				expectedRevision: 3
			})
		).rejects.toMatchObject({ status: 0, code: 'csrf_token_missing' });
		expect(fetcher).not.toHaveBeenCalled();
	});

	it('normalizes network failures without exposing transport details', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			throw new Error('connect ECONNREFUSED password=secret');
		});
		installBrowser(fetcher);

		await expect(fetchGuildAdminSettings(guildId)).rejects.toMatchObject({
			message: 'API 서버에 연결할 수 없습니다.',
			status: 0,
			code: 'network_error'
		});
	});
});
