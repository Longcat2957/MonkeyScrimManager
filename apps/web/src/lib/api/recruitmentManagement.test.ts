import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	addRecruitmentMember,
	cancelRecruitment,
	closeRecruitmentForEntry,
	dispatchRecruitmentParticipantConfirmationDms,
	isRecruitmentManagementRevisionConflict,
	removeRecruitmentMember,
	recruitmentManagementErrorText,
	restoreRecruitment
} from './recruitmentManagement';

const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const memberId = '1525847677542207548';
const requestId = '01900000-0000-7000-8000-000000000001';
const idempotencyKey = '01900000-0000-7000-8000-000000000002';

const snapshot = {
	recruitment: {
		id: recruitmentId,
		displayNumber: 214,
		title: '일반 내전 모집 #214',
		status: 'OPEN' as const,
		targetCount: 10 as const,
		bestOf: 3 as const,
		hardFearless: false,
		revision: 1,
		createdById: memberId,
		channelId: '1525900000000000001',
		messageId: '1525900000000000002',
		updatedAt: '2026-07-14T12:00:00Z'
	},
	participants: [{ id: memberId, displayName: 'Longcat', preferredLanes: ['MID'] }],
	links: { entry: `/g/${guildId}/recruitments/${recruitmentId}/entry` }
};

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('recruitment management API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('adds an existing guild member with browser CSRF and an idempotency key', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: snapshot, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(
			addRecruitmentMember(guildId, recruitmentId, memberId, idempotencyKey)
		).resolves.toEqual(snapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/recruitments/${recruitmentId}/members`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({ memberId });
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('cancels a recruitment through the same-origin DELETE endpoint', async () => {
		const cancelledSnapshot = {
			...snapshot,
			recruitment: { ...snapshot.recruitment, status: 'CANCELLED' as const, revision: 2 }
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: cancelledSnapshot, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(cancelRecruitment(guildId, recruitmentId, 1, idempotencyKey)).resolves.toEqual(
			cancelledSnapshot
		);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/recruitments/${recruitmentId}`);
		expect(init).toMatchObject({ method: 'DELETE', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({ expectedRevision: 1 });
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('restores a cancelled recruitment with optimistic revision protection', async () => {
		const restoredSnapshot = {
			...snapshot,
			recruitment: { ...snapshot.recruitment, revision: 3 }
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: restoredSnapshot, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(restoreRecruitment(guildId, recruitmentId, 2, idempotencyKey)).resolves.toEqual(
			restoredSnapshot
		);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/recruitments/${recruitmentId}/restore`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({ expectedRevision: 2 });
	});

	it('removes a participant with browser CSRF and returns the fresh recruitment snapshot', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({
				data: { ...snapshot, participants: [] },
				meta: { requestId }
			});
		});
		installBrowser(fetcher);

		await expect(
			removeRecruitmentMember(guildId, recruitmentId, memberId, idempotencyKey)
		).resolves.toEqual({ ...snapshot, participants: [] });

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/recruitments/${recruitmentId}/members/${memberId}`);
		expect(init).toMatchObject({ method: 'DELETE', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({});
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('starts entry editing from a full recruitment with browser mutation headers', async () => {
		const closedSnapshot = {
			...snapshot,
			recruitment: { ...snapshot.recruitment, status: 'CLOSED' as const }
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: closedSnapshot, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(closeRecruitmentForEntry(guildId, recruitmentId, idempotencyKey)).resolves.toEqual(
			closedSnapshot
		);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/recruitments/${recruitmentId}/close`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({});
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('requests participant confirmation DMs through the browser operator endpoint', async () => {
		const result = { queuedRecipientCount: 10, alreadyQueuedRecipientCount: 0 };
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: result, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(
			dispatchRecruitmentParticipantConfirmationDms(guildId, recruitmentId, idempotencyKey)
		).resolves.toEqual(result);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(
			`/api/guilds/${guildId}/recruitments/${recruitmentId}/participant-confirmation-dms`
		);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({});
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('preserves backend failure details and the request ID', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'recruitment_full',
						message: '모집 정원이 이미 찼습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await addRecruitmentMember(
			guildId,
			recruitmentId,
			memberId,
			idempotencyKey
		).catch((cause) => cause);
		expect(error).toMatchObject({ status: 409, code: 'recruitment_full', requestId });
		expect(recruitmentManagementErrorText(error)).toBe(
			`모집 정원이 이미 찼습니다. (요청 ID: ${requestId})`
		);
	});

	it('identifies optimistic revision conflicts so retries can use a new idempotency key', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'recruitment_revision_conflict',
						message: '모집 상태가 변경되었습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await cancelRecruitment(guildId, recruitmentId, 1, idempotencyKey).catch(
			(cause) => cause
		);

		expect(isRecruitmentManagementRevisionConflict(error)).toBe(true);
	});

	it('rejects before mutation when the CSRF cookie is missing', async () => {
		const fetcher = vi.fn();
		installBrowser(fetcher, 'other=value');

		await expect(
			cancelRecruitment(guildId, recruitmentId, 1, idempotencyKey)
		).rejects.toMatchObject({
			status: 0,
			code: 'csrf_token_missing'
		});
		expect(fetcher).not.toHaveBeenCalled();
	});
});
