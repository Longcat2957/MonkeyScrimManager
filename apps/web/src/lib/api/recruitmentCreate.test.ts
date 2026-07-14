import { afterEach, describe, expect, it, vi } from 'vitest';
import { createRecruitment, recruitmentCreateErrorText } from './recruitmentCreate';

const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const requestId = '01900000-0000-7000-8000-000000000001';
const idempotencyKey = '01900000-0000-7000-8000-000000000002';

const snapshot = {
	recruitment: {
		id: recruitmentId,
		displayNumber: 214,
		title: '일반 내전 모집 #214',
		status: 'OPEN' as const,
		targetCount: 10 as const,
		revision: 0,
		createdById: '1525847677542207548',
		channelId: null,
		messageId: null,
		updatedAt: '2026-07-14T12:00:00Z'
	},
	participants: [],
	links: { entry: `/g/${guildId}/recruitments/${recruitmentId}/entry` }
};

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('recruitment create API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('creates a session-owned recruitment and returns its authoritative entry link', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: snapshot, meta: { requestId } }, { status: 201 });
		});
		installBrowser(fetcher);

		await expect(createRecruitment(guildId, idempotencyKey)).resolves.toEqual(snapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/recruitments`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin', body: '{}' });
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('preserves a public backend failure and its request ID', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'active_season_required',
						message: '현재 활성 시즌이 없어 내전 모집을 만들 수 없습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await createRecruitment(guildId, idempotencyKey).catch((cause) => cause);
		expect(error).toMatchObject({ status: 409, code: 'active_season_required', requestId });
		expect(recruitmentCreateErrorText(error)).toBe(
			`현재 활성 시즌이 없어 내전 모집을 만들 수 없습니다. (요청 ID: ${requestId})`
		);
	});

	it('fails before a mutation when client security metadata is missing', async () => {
		const fetcher = vi.fn();
		installBrowser(fetcher, 'other=value');

		await expect(createRecruitment(guildId, idempotencyKey)).rejects.toMatchObject({
			status: 0,
			code: 'csrf_token_missing'
		});
		expect(fetcher).not.toHaveBeenCalled();
	});
});
