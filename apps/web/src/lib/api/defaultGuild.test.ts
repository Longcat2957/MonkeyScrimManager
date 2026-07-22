import { afterEach, describe, expect, it, vi } from 'vitest';
import { defaultGuildErrorText, DefaultGuildApiError, updateDefaultGuild } from './defaultGuild';

const guildId = '200000000000000001';
const requestId = '01900000-0000-7000-8000-000000000001';

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('default guild API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('updates through the same-origin API and returns the authoritative guild ID', async () => {
		const fetcher = vi.fn(
			async (...args: Parameters<typeof globalThis.fetch>): Promise<Response> => {
				void args;
				return Response.json({ data: { defaultGuildId: guildId }, meta: { requestId } });
			}
		);
		installBrowser(fetcher);

		await expect(updateDefaultGuild(guildId)).resolves.toBe(guildId);
		expect(fetcher).toHaveBeenCalledWith(
			'/api/me/default-guild',
			expect.objectContaining({ method: 'PUT', credentials: 'same-origin' })
		);
		const init = fetcher.mock.calls[0]?.[1];
		expect(new Headers(init?.headers).get('X-CSRF-Token')).toBe('csrf-token');
		expect(JSON.parse(String(init?.body))).toEqual({ guildId });
	});

	it('preserves the backend code and shows the request ID with its safe message', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'csrf_validation_failed',
						message: '보안 확인에 실패했습니다. 새로고침 후 다시 시도해 주세요.',
						requestId
					}
				},
				{ status: 403 }
			)
		);
		installBrowser(fetcher);

		const cause = await updateDefaultGuild(guildId).catch((error: unknown) => error);
		expect(cause).toBeInstanceOf(DefaultGuildApiError);
		expect(cause).toMatchObject({ code: 'csrf_validation_failed', requestId });
		expect(defaultGuildErrorText(cause)).toBe(
			`보안 확인에 실패했습니다. 새로고침 후 다시 시도해 주세요. (요청 ID: ${requestId})`
		);
	});

	it('reports a safe local reason when the CSRF token is missing', async () => {
		const fetcher = vi.fn();
		installBrowser(fetcher as typeof globalThis.fetch, 'other=value');

		await expect(updateDefaultGuild(guildId)).rejects.toMatchObject({
			code: 'csrf_token_missing',
			message: '보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.'
		});
		expect(fetcher).not.toHaveBeenCalled();
	});
});
