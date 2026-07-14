import { afterEach, describe, expect, it, vi } from 'vitest';
import type { SeriesResultSnapshot } from '$lib/contracts/seriesResult';
import { fetchSeriesResult, seriesResultErrorText } from './seriesResult';

const guildId = '1525851181157580861';
const seriesId = '01900000-0000-7000-8000-000000000201';
const recruitmentId = '01900000-0000-7000-8000-000000000202';
const gameId = '01900000-0000-7000-8000-000000000203';
const requestId = '01900000-0000-7000-8000-000000000001';
const lanes = ['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT'] as const;
const teamOneIds = [
	'1525847677542207501',
	'1525847677542207502',
	'1525847677542207503',
	'1525847677542207504',
	'1525847677542207505'
];
const teamTwoIds = [
	'1525847677542207506',
	'1525847677542207507',
	'1525847677542207508',
	'1525847677542207509',
	'1525847677542207510'
];

function players(ids: string[], prefix: string) {
	return lanes.map((lane, index) => ({
		id: ids[index]!,
		displayName: `${prefix} ${index + 1}`,
		lane,
		mmr: 1500 + index,
		riotTier: index === 0 ? 'P3' : null,
		preferredChampionIds: index === 0 ? ['Aatrox'] : []
	}));
}

const playerIds = [...teamOneIds, ...teamTwoIds];
const snapshot: SeriesResultSnapshot = {
	series: {
		id: seriesId,
		displayNumber: 42,
		title: '금요 일반 내전',
		bestOf: 3,
		hardFearless: true,
		status: 'COMPLETED',
		revision: 12,
		sourceRecruitmentId: recruitmentId,
		winningTeam: 'TEAM_1',
		terminalReason: 'SCORE_REACHED',
		startedAt: '2026-07-14T12:00:00Z',
		endedAt: '2026-07-14T13:32:00Z'
	},
	teams: [
		{ team: 'TEAM_1', name: '1팀', players: players(teamOneIds, '블루') },
		{ team: 'TEAM_2', name: '2팀', players: players(teamTwoIds, '레드') }
	],
	games: [
		{
			id: gameId,
			setNumber: 1,
			sidePreset: 'DEFAULT',
			winner: 'BLUE',
			picks: Object.fromEntries(playerIds.map((id) => [id, 'Aatrox'])),
			bans: {
				BLUE: ['Ahri', 'Aatrox', 'Alistar', 'Ashe', 'Azir'],
				RED: ['Blitzcrank', 'Caitlyn', 'Ezreal', 'Gnar', 'Graves']
			},
			patchVersion: '16.13.1',
			durationSeconds: 1902,
			mmrChanges: playerIds.map((userId, index) => ({
				userId,
				lane: lanes[index % lanes.length]!,
				delta: index < 5 ? 17 : -17,
				mmrBefore: 1500 + index,
				mmrAfter: 1500 + index + (index < 5 ? 17 : -17)
			}))
		}
	],
	score: { team1: 2, team2: 0 },
	capabilities: { canRevertLastGame: true, canDelete: true }
};

describe('series result API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads the completed ledger through the same-origin result route', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: snapshot, meta: { requestId } });
		});
		vi.stubGlobal('fetch', fetcher);

		await expect(fetchSeriesResult(guildId, seriesId)).resolves.toEqual(snapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/series/${seriesId}/result`);
		expect(init).toMatchObject({ credentials: 'same-origin' });
		expect(new Headers(init?.headers).get('accept')).toBe('application/json');
	});

	it('preserves the API reason and request ID for result access failures', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'series_result_not_available',
						message: '완료된 시리즈 결과만 확인할 수 있습니다.',
						requestId
					}
				},
				{ status: 404 }
			)
		);
		vi.stubGlobal('fetch', fetcher);

		const error = await fetchSeriesResult(guildId, seriesId).catch((cause) => cause);

		expect(error).toMatchObject({ status: 404, code: 'series_result_not_available', requestId });
		expect(seriesResultErrorText(error)).toBe(
			`완료된 시리즈 결과만 확인할 수 있습니다. (요청 ID: ${requestId})`
		);
	});

	it('rejects a malformed completed ledger before it reaches the result board', async () => {
		const fetcher = vi.fn(async () =>
			Response.json({ data: { ...snapshot, teams: [snapshot.teams[0]] }, meta: { requestId } })
		);
		vi.stubGlobal('fetch', fetcher);

		const error = await fetchSeriesResult(guildId, seriesId).catch((cause) => cause);

		expect(error).toMatchObject({ code: 'invalid_response', requestId });
		expect(seriesResultErrorText(error)).toBe(
			`시리즈 결과 응답 형식이 올바르지 않습니다. (요청 ID: ${requestId})`
		);
	});
});
