import { afterEach, describe, expect, it, vi } from 'vitest';
import type { PrivacySettings } from '$lib/contracts/workflow';
import { fetchPrivacySettings, privacySettingsErrorText, updatePrivacySettings } from './privacy';

const requestId = '01900000-0000-7000-8000-000000000001';
const settings: PrivacySettings = {
	publicProfile: true,
	showRiotIds: true,
	allowExternalRecordSearch: false
};

function success(snapshot: PrivacySettings = settings): Response {
	return Response.json({ data: { settings: snapshot }, meta: { requestId } });
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

describe('privacy settings API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads the complete settings snapshot from the same-origin API route', async () => {
		const fetcher = fetchMock(() => success());
		installBrowser(fetcher);

		await expect(fetchPrivacySettings()).resolves.toEqual(settings);
		expect(fetcher).toHaveBeenCalledOnce();
		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe('/api/me/privacy');
		expect(init).toMatchObject({ credentials: 'same-origin' });
		expect(new Headers(init?.headers).get('accept')).toBe('application/json');
	});

	it('updates all settings with CSRF and returns the authoritative server snapshot', async () => {
		const submitted: PrivacySettings = {
			publicProfile: false,
			showRiotIds: false,
			allowExternalRecordSearch: true
		};
		const authoritative: PrivacySettings = { ...submitted, publicProfile: true };
		const fetcher = fetchMock(() => success(authoritative));
		installBrowser(fetcher, 'other=value; monkeybot_csrf=csrf-token');

		await expect(updatePrivacySettings(submitted)).resolves.toEqual(authoritative);
		expect(fetcher).toHaveBeenCalledOnce();
		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe('/api/me/privacy');
		expect(init).toMatchObject({ method: 'PUT', credentials: 'same-origin' });
		expect(new Headers(init?.headers).get('content-type')).toBe('application/json');
		expect(new Headers(init?.headers).get('X-CSRF-Token')).toBe('csrf-token');
		expect(JSON.parse(String(init?.body))).toEqual(submitted);
	});

	it('preserves backend errors and rejects non-strict successful snapshots', async () => {
		const backendFailure = fetchMock(() =>
			Response.json(
				{
					error: {
						code: 'privacy_update_rejected',
						message: '개인정보 설정을 저장할 수 없습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(backendFailure);
		const backendError = await updatePrivacySettings(settings).catch((cause) => cause);
		expect(backendError).toMatchObject({
			name: 'PrivacySettingsApiError',
			message: '개인정보 설정을 저장할 수 없습니다.',
			status: 409,
			code: 'privacy_update_rejected',
			requestId
		});
		expect(privacySettingsErrorText(backendError)).toBe(
			`개인정보 설정을 저장할 수 없습니다. (요청 ID: ${requestId})`
		);

		const invalidSuccess = fetchMock(() =>
			Response.json({
				data: { settings: { ...settings, unexpected: true } },
				meta: { requestId }
			})
		);
		vi.stubGlobal('fetch', invalidSuccess);
		await expect(fetchPrivacySettings()).rejects.toMatchObject({
			status: 200,
			code: 'invalid_response'
		});
	});

	it('fails before a mutation when the CSRF cookie is unavailable', async () => {
		const fetcher = fetchMock(() => success());
		installBrowser(fetcher, 'other=value');

		await expect(updatePrivacySettings(settings)).rejects.toMatchObject({
			message: '보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			status: 0,
			code: 'csrf_token_missing'
		});
		expect(fetcher).not.toHaveBeenCalled();
	});

	it('normalizes network failures without leaking transport details', async () => {
		const networkFailure = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			throw new Error('connect ECONNREFUSED password=secret');
		});
		installBrowser(networkFailure);

		await expect(fetchPrivacySettings()).rejects.toMatchObject({
			message: 'API 서버에 연결할 수 없습니다.',
			status: 0,
			code: 'network_error'
		});
	});
});
