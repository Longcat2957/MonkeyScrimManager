import { describe, expect, it, vi } from 'vitest';
import { load } from './+page.server';

const guildId = '1525851181157580861';
const requestId = '01900000-0000-7000-8000-000000000001';
const memberSnapshot = {
	revision: 0,
	updatedAt: '2026-07-13T03:00:00Z',
	updatedBy: null,
	members: []
};
const settingsSnapshot = {
	settings: {
		displayName: 'MonkeyBotTest',
		operatorRoleId: '1525889769761149148',
		recruitmentChannelId: null,
		entryChannelId: null,
		logChannelId: null,
		isExternallyPublic: false
	},
	revision: 0,
	updatedAt: '2026-07-13T03:00:00Z',
	updatedBy: null,
	catalogUpdatedAt: null,
	roles: [],
	channels: []
};
const auditSnapshot = {
	entries: [
		{
			id: '01900000-0000-7000-8000-000000000010',
			principalType: 'SYSTEM',
			principalId: 'monkeybot-system',
			principalDisplayName: null,
			action: 'GUILD_SETTINGS_UPDATED',
			targetType: 'GUILD_CONFIG',
			targetId: guildId,
			targetDisplayName: 'MonkeyBotTest',
			checkpointId: null,
			requestId,
			beforeSummary: null,
			afterSummary: { isExternallyPublic: false },
			result: 'APPLIED',
			note: null,
			createdAt: '2026-07-13T03:00:00Z'
		}
	],
	nextCursor: 'next-page'
};

function defaultFetcher() {
	return vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
		const path = String(args[0]);
		const data = path.includes('/audit-logs')
			? auditSnapshot
			: path.endsWith('/settings')
				? settingsSnapshot
				: memberSnapshot;
		return Response.json({ data, meta: { requestId } });
	});
}

function run(url: string, fetcher = defaultFetcher()) {
	return load({
		fetch: fetcher,
		params: { guildId },
		url: new URL(url)
	} as never);
}

describe('guild admin server load', () => {
	it('loads the authoritative settings snapshot only for the settings tab', async () => {
		const fetcher = defaultFetcher();
		await expect(
			run(`http://localhost:5173/g/${guildId}/admin?tab=settings`, fetcher)
		).resolves.toEqual({
			guildId,
			memberSnapshot: null,
			settingsSnapshot,
			auditSnapshot: null
		});
		expect(fetcher).toHaveBeenCalledOnce();
		expect(fetcher).toHaveBeenCalledWith(`/api/guilds/${guildId}/settings`, {
			headers: { accept: 'application/json' }
		});
	});

	it('loads the authoritative member snapshot for the default members tab', async () => {
		const fetcher = defaultFetcher();

		await expect(
			run(`http://localhost:5173/g/${guildId}/admin?tab=members`, fetcher)
		).resolves.toEqual({
			guildId,
			memberSnapshot,
			settingsSnapshot: null,
			auditSnapshot: null
		});
		expect(fetcher).toHaveBeenCalledOnce();
		expect(fetcher).toHaveBeenCalledWith(`/api/guilds/${guildId}/members`, {
			headers: { accept: 'application/json' }
		});
	});

	it('loads only the first authoritative audit log page for the log tab', async () => {
		const fetcher = defaultFetcher();
		await expect(
			run(`http://localhost:5173/g/${guildId}/admin?tab=logs`, fetcher)
		).resolves.toEqual({
			guildId,
			memberSnapshot: null,
			settingsSnapshot: null,
			auditSnapshot
		});
		expect(fetcher).toHaveBeenCalledOnce();
		expect(fetcher).toHaveBeenCalledWith(`/api/guilds/${guildId}/audit-logs?limit=50`, {
			headers: { accept: 'application/json' }
		});
	});

	it('redirects the deferred operations tab to members', async () => {
		await expect(
			run(`http://localhost:5173/g/${guildId}/admin?tab=operations`)
		).rejects.toMatchObject({
			status: 307,
			location: `/g/${guildId}/admin?tab=members`
		});
	});
});
