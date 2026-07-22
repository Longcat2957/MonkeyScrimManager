import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	championPreferencesErrorText,
	fetchChampionPreferences,
	updateChampionPreferences
} from './championPreferences';
import type { ChampionPreferences } from '$lib/contracts/workflow';

const requestId = '01900000-0000-7000-8000-000000000001';
const preferences: ChampionPreferences = {
	TOP: ['Aatrox', 'Gnar'],
	JUNGLE: ['LeeSin'],
	MID: ['Ahri', 'Orianna'],
	BOTTOM: ['Ezreal'],
	SUPPORT: ['Thresh']
};

function success(snapshot: ChampionPreferences = preferences): Response {
	return Response.json({ data: { preferences: snapshot }, meta: { requestId } });
}

function fetchMock(response: () => Response) {
	return vi.fn(async (...args: Parameters<typeof globalThis.fetch>): Promise<Response> => {
		void args;
		return response();
	});
}

function installBrowser(fetcher: typeof globalThis.fetch) {
	vi.stubGlobal('document', { cookie: 'monkeybot_csrf=csrf-token; other=value' });
	vi.stubGlobal('fetch', fetcher);
}

describe('champion preference API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads the full preference snapshot from the same-origin API route', async () => {
		const fetcher = fetchMock(() => success());
		installBrowser(fetcher);

		await expect(fetchChampionPreferences()).resolves.toEqual(preferences);
		expect(fetcher).toHaveBeenCalledOnce();
		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe('/api/me/champion-preferences');
		expect(init).toMatchObject({ credentials: 'same-origin' });
		expect(new Headers(init?.headers).get('accept')).toBe('application/json');
	});

	it('updates one canonical lane and returns the authoritative full snapshot', async () => {
		const snapshot: ChampionPreferences = { ...preferences, MID: ['Syndra', 'Ahri'] };
		const fetcher = fetchMock(() => success(snapshot));
		installBrowser(fetcher);

		await expect(updateChampionPreferences('MID', ['Syndra', 'Ahri'])).resolves.toEqual(snapshot);
		expect(fetcher).toHaveBeenCalledOnce();
		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe('/api/me/champion-preferences/MID');
		expect(init).toMatchObject({ method: 'PUT', credentials: 'same-origin' });
		expect(new Headers(init?.headers).get('content-type')).toBe('application/json');
		expect(new Headers(init?.headers).get('X-CSRF-Token')).toBe('csrf-token');
		expect(JSON.parse(String(init?.body))).toEqual({ championIds: ['Syndra', 'Ahri'] });
	});

	it('preserves backend errors and rejects incomplete successful snapshots', async () => {
		const backendFailure = fetchMock(() =>
			Response.json(
				{
					error: {
						code: 'invalid_champion_preferences',
						message: '중복된 챔피언은 등록할 수 없습니다.',
						requestId
					}
				},
				{ status: 422 }
			)
		);
		installBrowser(backendFailure);
		const backendError = await updateChampionPreferences('TOP', ['Gnar', 'Gnar']).catch(
			(cause) => cause
		);
		expect(backendError).toMatchObject({
			name: 'ChampionPreferencesApiError',
			message: '중복된 챔피언은 등록할 수 없습니다.',
			status: 422,
			code: 'invalid_champion_preferences',
			requestId
		});
		expect(championPreferencesErrorText(backendError)).toBe(
			`중복된 챔피언은 등록할 수 없습니다. (요청 ID: ${requestId})`
		);

		const incompleteSuccess = fetchMock(() =>
			Response.json({
				data: { preferences: { TOP: [], JUNGLE: [], MID: [], BOTTOM: [] } },
				meta: { requestId }
			})
		);
		vi.stubGlobal('fetch', incompleteSuccess);
		await expect(fetchChampionPreferences()).rejects.toMatchObject({
			status: 200,
			code: 'invalid_response'
		});
	});

	it('normalizes network failures without leaking transport details', async () => {
		const networkFailure = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			throw new Error('connect ECONNREFUSED password=secret');
		});
		installBrowser(networkFailure);

		await expect(fetchChampionPreferences()).rejects.toMatchObject({
			message: 'API 서버에 연결할 수 없습니다.',
			status: 0,
			code: 'network_error'
		});
	});
});
