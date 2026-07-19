import { afterEach, describe, expect, it, vi } from 'vitest';
import type { SeriesPickBanSnapshot } from '$lib/contracts/seriesPickBan';
import {
	completeSeriesEarly,
	deleteSeries,
	fetchSeriesPickBan,
	isSeriesPickBanRevisionConflict,
	revertLastSeriesPickBanGame,
	revertSeries,
	seriesPickBanErrorText,
	submitSeriesPickBanGame,
	updateSeriesPickBanDraft
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
		hardFearless: false,
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
		sideSelected: false,
		winner: null,
		picks: Object.fromEntries([...teamOneIds, ...teamTwoIds].map((id) => [id, null])),
		bans: { BLUE: [null, null, null, null, null], RED: [null, null, null, null, null] },
		patchVersion: '16.13.1'
	},
	games: [],
	capabilities: {
		canEdit: true,
		canSubmit: true,
		canEarlyComplete: false,
		canRevertLastGame: false,
		canRevert: true,
		canDelete: true
	}
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

	it('accepts the confirmed-game projection alongside the active pick-ban game', async () => {
		const nextSnapshot: SeriesPickBanSnapshot = {
			...snapshot,
			series: { ...snapshot.series, revision: 8 },
			currentGame: { ...snapshot.currentGame, setNumber: 2 },
			games: [
				{
					...snapshot.currentGame,
					sideSelected: true,
					winner: 'BLUE'
				}
			],
			capabilities: { ...snapshot.capabilities, canRevertLastGame: true }
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: nextSnapshot, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(fetchSeriesPickBan(guildId, seriesId)).resolves.toEqual(nextSnapshot);
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

	it('saves one semantic draft cell operation with CSRF but no idempotency key', async () => {
		const nextSnapshot: SeriesPickBanSnapshot = {
			...snapshot,
			series: { ...snapshot.series, revision: 8 },
			currentGame: {
				...snapshot.currentGame,
				sideSelected: true,
				bans: { ...snapshot.currentGame.bans, BLUE: ['Aatrox', null, null, null, null] }
			}
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: nextSnapshot, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(
			updateSeriesPickBanDraft(guildId, seriesId, {
				expectedRevision: 7,
				operation: { type: 'SET_BAN', side: 'BLUE', slot: 1, championId: 'Aatrox' }
			})
		).resolves.toEqual(nextSnapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/series/${seriesId}/pickban`);
		expect(init).toMatchObject({ method: 'PATCH', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({
			expectedRevision: 7,
			operation: { type: 'SET_BAN', side: 'BLUE', slot: 1, championId: 'Aatrox' }
		});
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBeNull();
	});

	it('saves a bulk draft as one APPLY_BULK mutation', async () => {
		const nextSnapshot: SeriesPickBanSnapshot = {
			...snapshot,
			series: { ...snapshot.series, revision: 8 },
			currentGame: {
				...snapshot.currentGame,
				sideSelected: true,
				picks: { ...snapshot.currentGame.picks, [teamOneIds[0]!]: 'Gnar' },
				bans: { ...snapshot.currentGame.bans, RED: ['Yone', null, null, null, null] }
			}
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: nextSnapshot, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(
			updateSeriesPickBanDraft(guildId, seriesId, {
				expectedRevision: 7,
				operation: {
					type: 'APPLY_BULK',
					items: [
						{ type: 'SET_PICK', playerId: teamOneIds[0]!, championId: 'Gnar' },
						{ type: 'SET_BAN', side: 'RED', slot: 1, championId: 'Yone' }
					]
				}
			})
		).resolves.toEqual(nextSnapshot);

		const [, init] = fetcher.mock.calls[0]!;
		expect(JSON.parse(String(init?.body))).toEqual({
			expectedRevision: 7,
			operation: {
				type: 'APPLY_BULK',
				items: [
					{ type: 'SET_PICK', playerId: teamOneIds[0], championId: 'Gnar' },
					{ type: 'SET_BAN', side: 'RED', slot: 1, championId: 'Yone' }
				]
			}
		});
	});

	it('submits and reverses the latest game through their guarded idempotent commands', async () => {
		const submitted = {
			series: { id: seriesId, status: 'IN_PROGRESS' as const, revision: 8 },
			confirmedGameCount: 1,
			nextHref: `/g/${guildId}/series/${seriesId}/pick-ban`
		};
		const reverted = {
			series: { id: seriesId, status: 'IN_PROGRESS' as const, revision: 9 },
			confirmedGameCount: 0,
			nextHref: `/g/${guildId}/series/${seriesId}/pick-ban`
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			const [, init] = args;
			return Response.json({
				data: init?.method === 'DELETE' ? reverted : submitted,
				meta: { requestId }
			});
		});
		installBrowser(fetcher);

		await expect(
			submitSeriesPickBanGame(guildId, seriesId, {
				setNumber: 1,
				expectedRevision: 7,
				sidePreset: 'DEFAULT',
				winner: 'BLUE',
				picks: snapshot.currentGame.picks,
				bans: snapshot.currentGame.bans,
				patchVersion: '16.13.1',
				idempotencyKey
			})
		).resolves.toEqual(submitted);
		await expect(
			revertLastSeriesPickBanGame(guildId, seriesId, {
				expectedRevision: 8,
				idempotencyKey
			})
		).resolves.toEqual(reverted);

		const [submitPath, submitInit] = fetcher.mock.calls[0]!;
		expect(submitPath).toBe(`/api/guilds/${guildId}/series/${seriesId}/games`);
		expect(submitInit).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(submitInit?.body))).toMatchObject({
			setNumber: 1,
			expectedRevision: 7,
			sidePreset: 'DEFAULT',
			winner: 'BLUE'
		});

		const [revertPath, revertInit] = fetcher.mock.calls[1]!;
		expect(revertPath).toBe(`/api/guilds/${guildId}/series/${seriesId}/games/last`);
		expect(revertInit).toMatchObject({ method: 'DELETE', credentials: 'same-origin' });
		expect(JSON.parse(String(revertInit?.body))).toEqual({ expectedRevision: 8 });
		for (const init of [submitInit, revertInit]) {
			const headers = new Headers(init?.headers);
			expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
			expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
		}
	});

	it('ends an undecided series early through the guarded completion route', async () => {
		const completed = {
			series: { id: seriesId, status: 'COMPLETED' as const, revision: 8 },
			confirmedGameCount: 1,
			nextHref: `/g/${guildId}/series/${seriesId}/result`
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: completed, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(
			completeSeriesEarly(guildId, seriesId, {
				expectedRevision: 7,
				outcome: 'TEAM_1',
				idempotencyKey
			})
		).resolves.toEqual(completed);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/series/${seriesId}/complete`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({
			expectedRevision: 7,
			outcome: 'TEAM_1'
		});
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('rejects submit commands without an explicit side preset before fetching', async () => {
		const fetcher = vi.fn();
		installBrowser(fetcher);

		const error = await submitSeriesPickBanGame(guildId, seriesId, {
			setNumber: 1,
			expectedRevision: 7,
			sidePreset: undefined,
			winner: 'BLUE',
			idempotencyKey
		} as never).catch((cause) => cause);

		expect(error).toMatchObject({ code: 'invalid_side_preset' });
		expect(fetcher).not.toHaveBeenCalled();
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

	it('identifies a revision conflict so the workspace can reload instead of retrying the command', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'series_revision_conflict',
						message: '시리즈 상태가 변경되었습니다. 최신 상태를 확인해 주세요.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await updateSeriesPickBanDraft(guildId, seriesId, {
			expectedRevision: 7,
			operation: { type: 'SET_SIDE', sidePreset: 'SWAPPED' }
		}).catch((cause) => cause);

		expect(isSeriesPickBanRevisionConflict(error)).toBe(true);
		expect(seriesPickBanErrorText(error)).toBe(
			`시리즈 상태가 변경되었습니다. 최신 상태를 확인해 주세요. (요청 ID: ${requestId})`
		);
	});

	it('rejects a malformed roster snapshot before exposing it to the pick-ban workspace', async () => {
		const fetcher = vi.fn(async () =>
			Response.json({ data: { ...snapshot, teams: [] }, meta: { requestId } })
		);
		installBrowser(fetcher);

		const error = await fetchSeriesPickBan(guildId, seriesId).catch((cause) => cause);

		expect(error).toMatchObject({ code: 'invalid_response', requestId });
		expect(seriesPickBanErrorText(error)).toBe(
			`시리즈 정보 조회 응답 형식이 올바르지 않습니다. (요청 ID: ${requestId})`
		);
	});

	it('rejects current pick-ban data when the side was not selected by the server', async () => {
		const fetcher = vi.fn(async () =>
			Response.json({
				data: {
					...snapshot,
					currentGame: {
						...snapshot.currentGame,
						sideSelected: false,
						picks: { ...snapshot.currentGame.picks, [teamOneIds[0]!]: 'Aatrox' }
					}
				},
				meta: { requestId }
			})
		);
		installBrowser(fetcher);

		const error = await fetchSeriesPickBan(guildId, seriesId).catch((cause) => cause);

		expect(error).toMatchObject({ code: 'invalid_response', requestId });
	});
});
