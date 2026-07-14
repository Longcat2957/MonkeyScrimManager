import { afterEach, describe, expect, it, vi } from 'vitest';
import type { SeriesPickBanSnapshot } from '$lib/contracts/seriesPickBan';
import {
	deleteSeries,
	fetchSeriesPickBan,
	revertSeries,
	seriesPickBanErrorText
} from './seriesPickBan';

const guildId = '1525851181157580861';
const seriesId = '01900000-0000-7000-8000-000000000101';
const recruitmentId = '01900000-0000-7000-8000-000000000102';
const requestId = '01900000-0000-7000-8000-000000000001';
const idempotencyKey = '01900000-0000-7000-8000-000000000002';
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

const snapshot: SeriesPickBanSnapshot = {
	series: {
		id: seriesId,
		displayNumber: 42,
		title: '금요 일반 내전',
		bestOf: 3,
		status: 'IN_PROGRESS',
		revision: 7,
		sourceRecruitmentId: recruitmentId,
		updatedAt: '2026-07-14T12:00:00Z'
	},
	teams: [
		{ team: 'TEAM_1', name: '1팀', players: players(teamOneIds, '블루') },
		{ team: 'TEAM_2', name: '2팀', players: players(teamTwoIds, '레드') }
	],
	currentGame: {
		setNumber: 1,
		sidePreset: 'DEFAULT',
		winner: null,
		picks: Object.fromEntries([...teamOneIds, ...teamTwoIds].map((id) => [id, null])),
		bans: { BLUE: [null, null, null, null, null], RED: [null, null, null, null, null] },
		patchVersion: '16.13.1'
	},
	games: [],
	capabilities: { canRevert: true, canDelete: true }
};

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('series pick-ban API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads the strict series snapshot through the same-origin API route', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: snapshot, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(fetchSeriesPickBan(guildId, seriesId)).resolves.toEqual(snapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/series/${seriesId}`);
		expect(init).toMatchObject({ credentials: 'same-origin' });
		expect(new Headers(init?.headers).get('accept')).toBe('application/json');
	});

	it('uses separate browser lifecycle routes with the series revision, CSRF, and idempotency key', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			const [, init] = args;
			const nextHref =
				init?.method === 'DELETE'
					? `/g/${guildId}/series`
					: `/g/${guildId}/recruitments/${recruitmentId}/entry`;
			return Response.json({ data: { nextHref }, meta: { requestId } });
		});
		installBrowser(fetcher, 'theme=dark; monkeybot_csrf=csrf-token');

		await expect(
			revertSeries(guildId, seriesId, { expectedRevision: 7, idempotencyKey })
		).resolves.toEqual({ nextHref: `/g/${guildId}/recruitments/${recruitmentId}/entry` });
		await expect(
			deleteSeries(guildId, seriesId, { expectedRevision: 7, idempotencyKey })
		).resolves.toEqual({ nextHref: `/g/${guildId}/series` });

		const [revertPath, revertInit] = fetcher.mock.calls[0]!;
		expect(revertPath).toBe(`/api/guilds/${guildId}/series/${seriesId}/revert`);
		expect(revertInit).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(revertInit?.body))).toEqual({ expectedRevision: 7 });

		const [deletePath, deleteInit] = fetcher.mock.calls[1]!;
		expect(deletePath).toBe(`/api/guilds/${guildId}/series/${seriesId}`);
		expect(deleteInit).toMatchObject({ method: 'DELETE', credentials: 'same-origin' });
		expect(JSON.parse(String(deleteInit?.body))).toEqual({ expectedRevision: 7 });
		for (const init of [revertInit, deleteInit]) {
			const headers = new Headers(init?.headers);
			expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
			expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
		}
	});

	it('preserves a lifecycle conflict reason and request ID from the API', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'series_has_confirmed_games',
						message: '확정된 게임이 있어 시리즈를 되돌릴 수 없습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await revertSeries(guildId, seriesId, {
			expectedRevision: 7,
			idempotencyKey
		}).catch((cause) => cause);

		expect(error).toMatchObject({ status: 409, code: 'series_has_confirmed_games', requestId });
		expect(seriesPickBanErrorText(error)).toBe(
			`확정된 게임이 있어 시리즈를 되돌릴 수 없습니다. (요청 ID: ${requestId})`
		);
	});

	it('rejects a malformed roster snapshot before exposing it to the pick-ban workspace', async () => {
		const fetcher = vi.fn(async () =>
			Response.json({ data: { ...snapshot, teams: [] }, meta: { requestId } })
		);
		installBrowser(fetcher);

		await expect(fetchSeriesPickBan(guildId, seriesId)).rejects.toMatchObject({
			code: 'invalid_response'
		});
	});
});
