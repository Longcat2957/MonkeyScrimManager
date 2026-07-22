import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	createRiotAccount,
	deleteAccount,
	fetchRiotAccounts,
	parseRiotIdInput,
	refreshAccount,
	riotErrorText,
	setMainAccount
} from './riot';
import type { RiotAccount } from '$lib/contracts/workflow';

const requestId = '01900000-0000-7000-8000-000000000001';

const mainAccount: RiotAccount = {
	puuid: 'puuid-main',
	platform: 'KR',
	routingRegion: 'ASIA',
	gameName: 'Longcat',
	tagLine: 'KR1',
	profileIconId: 12,
	isMain: true,
	mainPosition: null,
	lastSyncedAt: '2026-07-13T00:00:00Z',
	createdAt: '2026-07-12T00:00:00Z',
	updatedAt: '2026-07-13T00:00:00Z'
};

const secondaryAccount: RiotAccount = {
	...mainAccount,
	puuid: 'puuid-secondary',
	gameName: 'MonkeyBot',
	tagLine: 'TEST',
	profileIconId: null,
	isMain: false
};

function success(accounts: RiotAccount[] = [mainAccount, secondaryAccount]): Response {
	return Response.json({ data: { riotAccounts: accounts }, meta: { requestId } });
}

function installBrowser(fetcher: typeof globalThis.fetch) {
	vi.stubGlobal('document', { cookie: 'other=value; monkeybot_csrf=csrf-token' });
	vi.stubGlobal('fetch', fetcher);
}

function fetchMock(response: () => Response) {
	return vi.fn(async (...args: Parameters<typeof globalThis.fetch>): Promise<Response> => {
		void args;
		return response();
	});
}

describe('Riot account API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('parses exactly one GameName#TagLine separator without rewriting user input', () => {
		expect(parseRiotIdInput('  Long Cat # kr1  ')).toEqual({
			gameName: 'Long Cat',
			tagLine: 'kr1'
		});
		expect(parseRiotIdInput('Longcat')).toBeNull();
		expect(parseRiotIdInput('#KR1')).toBeNull();
		expect(parseRiotIdInput('Longcat#')).toBeNull();
		expect(parseRiotIdInput('Long#cat#KR1')).toBeNull();
	});

	it('lists accounts through the same-origin API route', async () => {
		const fetcher = fetchMock(() => success());
		installBrowser(fetcher);

		await expect(fetchRiotAccounts()).resolves.toEqual([mainAccount, secondaryAccount]);
		expect(fetcher).toHaveBeenCalledWith('/api/me/riot-accounts', {
			credentials: 'same-origin',
			headers: { accept: 'application/json' }
		});
	});

	it('creates from Riot ID fields only and accepts the authoritative account snapshot', async () => {
		const fetcher = fetchMock(() => success([mainAccount]));
		installBrowser(fetcher);

		await expect(
			createRiotAccount({ gameName: 'Longcat', tagLine: 'KR1', platform: 'kr' })
		).resolves.toEqual([mainAccount]);

		expect(fetcher).toHaveBeenCalledOnce();
		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe('/api/me/riot-accounts');
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(new Headers(init?.headers).get('X-CSRF-Token')).toBe('csrf-token');
		expect(new Headers(init?.headers).get('content-type')).toBe('application/json');
		expect(JSON.parse(String(init?.body))).toEqual({
			gameName: 'Longcat',
			tagLine: 'KR1',
			platform: 'kr'
		});
	});

	it('uses encoded mutation routes and returns each server snapshot directly', async () => {
		const fetcher = fetchMock(() => success([secondaryAccount]));
		installBrowser(fetcher);
		const puuid = 'puuid/with space';

		await expect(setMainAccount(puuid)).resolves.toEqual([secondaryAccount]);
		await expect(refreshAccount(puuid)).resolves.toEqual([secondaryAccount]);
		await expect(deleteAccount(puuid)).resolves.toEqual([secondaryAccount]);

		expect(fetcher).toHaveBeenNthCalledWith(
			1,
			'/api/me/riot-accounts/puuid%2Fwith%20space/main',
			expect.objectContaining({ method: 'PUT' })
		);
		expect(fetcher).toHaveBeenNthCalledWith(
			2,
			'/api/me/riot-accounts/puuid%2Fwith%20space/refresh',
			expect.objectContaining({ method: 'POST' })
		);
		expect(fetcher).toHaveBeenNthCalledWith(
			3,
			'/api/me/riot-accounts/puuid%2Fwith%20space',
			expect.objectContaining({ method: 'DELETE' })
		);
		for (const [, init] of fetcher.mock.calls) {
			expect(new Headers(init?.headers).get('X-CSRF-Token')).toBe('csrf-token');
		}
	});

	it('preserves backend error details and normalizes transport failures', async () => {
		const backendFailure = fetchMock(() =>
			Response.json(
				{
					error: {
						code: 'riot_rate_limited',
						message: '잠시 후 다시 시도해 주세요.',
						requestId
					}
				},
				{ status: 429 }
			)
		);
		installBrowser(backendFailure);
		const backendError = await refreshAccount(mainAccount.puuid).catch((cause) => cause);
		expect(backendError).toMatchObject({
			name: 'RiotApiError',
			message: '잠시 후 다시 시도해 주세요.',
			status: 429,
			code: 'riot_rate_limited',
			requestId
		});
		expect(riotErrorText(backendError)).toBe(`잠시 후 다시 시도해 주세요. (요청 ID: ${requestId})`);

		const invalidSuccess = fetchMock(() => Response.json({ data: {}, meta: { requestId } }));
		vi.stubGlobal('fetch', invalidSuccess);
		await expect(fetchRiotAccounts()).rejects.toMatchObject({
			status: 200,
			code: 'invalid_response'
		});

		const networkFailure = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			throw new Error('connect ECONNREFUSED secret');
		});
		vi.stubGlobal('fetch', networkFailure);
		await expect(fetchRiotAccounts()).rejects.toMatchObject({
			message: 'API 서버에 연결할 수 없습니다.',
			status: 0,
			code: 'network_error'
		});
	});
});
