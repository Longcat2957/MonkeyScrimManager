import { afterEach, describe, expect, it, vi } from 'vitest';
import type { GuildLeaderboardSnapshot } from '$lib/contracts/workflow';
import { fetchGuildLeaderboard, guildLeaderboardErrorText } from './guildLeaderboard';

const guildId = '1525851181157580861';
const requestId = '01900000-0000-7000-8000-000000000001';
const snapshot: GuildLeaderboardSnapshot = {
	season: { id: '01900000-0000-7000-8000-000000000010', name: '시즌 1' },
	players: [
		{
			id: '390526792496054283',
			name: 'Longcat',
			tag: null,
			profileIconId: null,
			compositeMmr: 1600,
			mainRole: 'JUNGLE',
			isMe: true,
			stats: {
				JUNGLE: { mmr: 1600, games: 10, wins: 6, championId: null }
			}
		}
	]
};

function envelope(data: unknown = snapshot): Response {
	return Response.json({ data, meta: { requestId } });
}

describe('guild leaderboard API client', () => {
	afterEach(() => vi.restoreAllMocks());

	it('loads the strict active-season leaderboard', async () => {
		const fetcher = vi.fn(async () => envelope());

		await expect(fetchGuildLeaderboard(fetcher, guildId)).resolves.toEqual(snapshot);
		expect(fetcher).toHaveBeenCalledWith(`/api/guilds/${guildId}/leaderboard`, {
			method: 'GET',
			credentials: 'same-origin',
			headers: { accept: 'application/json' }
		});
	});

	it('preserves backend errors and rejects contract drift', async () => {
		const forbidden = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'guild_member_not_found',
						message: '길드 멤버를 찾을 수 없습니다.',
						requestId
					}
				},
				{ status: 404 }
			)
		);
		const forbiddenError = await fetchGuildLeaderboard(forbidden, guildId).catch((cause) => cause);
		expect(forbiddenError).toMatchObject({
			status: 404,
			code: 'guild_member_not_found',
			message: '길드 멤버를 찾을 수 없습니다.',
			requestId
		});
		expect(guildLeaderboardErrorText(forbiddenError)).toBe(
			`길드 멤버를 찾을 수 없습니다. (요청 ID: ${requestId})`
		);

		const malformed = vi.fn(async () => envelope({ ...snapshot, internalGuildId: guildId }));
		await expect(fetchGuildLeaderboard(malformed, guildId)).rejects.toMatchObject({
			status: 200,
			code: 'invalid_response'
		});
	});

	it('normalizes transport failures', async () => {
		const fetcher = vi.fn(async () => {
			throw new Error('connect ECONNREFUSED password=secret');
		});

		await expect(fetchGuildLeaderboard(fetcher, guildId)).rejects.toMatchObject({
			message: '리더보드 서버에 연결할 수 없습니다.',
			status: 0,
			code: 'network_error'
		});
	});
});
