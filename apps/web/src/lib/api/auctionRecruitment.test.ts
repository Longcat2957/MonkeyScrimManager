import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	addAuctionRecruitmentMember,
	auctionRecruitmentErrorText,
	cancelAuctionRecruitment,
	closeAuctionRecruitment,
	createAuctionRecruitment,
	dispatchAuctionRecruitmentParticipantConfirmationDms,
	isAuctionRecruitmentRevisionConflict,
	removeAuctionRecruitmentMember,
	reopenAuctionRecruitment,
	restoreAuctionRecruitment
} from './auctionRecruitment';

const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000501';
const requestId = '01900000-0000-7000-8000-000000000001';
const idempotencyKey = '01900000-0000-7000-8000-000000000002';
const expectedStartAt = '2026-07-26T11:00:00Z';
const expectedStartLabel = '2026-07-26 20:00 (KST)';

const openSnapshot = {
	recruitment: {
		id: recruitmentId,
		displayNumber: 42,
		title: `경매 모집 #42 · 예상 시작 ${expectedStartLabel}`,
		format: 'TEN_PERSON' as const,
		rules: {
			hardFearless: false,
			semifinalBestOf: null,
			finalBestOf: 3 as const
		},
		participantCount: 10 as const,
		joinedParticipantCount: 1,
		teamCount: 2 as const,
		status: 'OPEN' as const,
		revision: 0,
		channelId: '1525900000000000001',
		messageId: '1525900000000000002',
		createdById: '1525847677542207548',
		updatedAt: '2026-07-14T12:00:00Z',
		href: `/g/${guildId}/auction/recruitments/${recruitmentId}`
	},
	participants: [
		{ id: '1525847677542207548', displayName: 'Longcat', declaredLane: 'MID' as const }
	],
	links: { draft: null },
	permissions: {
		canManageMembers: false,
		canClose: false,
		canReopen: false,
		canRestore: false,
		canSendParticipantConfirmationDms: false,
		canCancel: true
	}
};

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('auction recruitment API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('creates an auction recruitment with a selected format and browser mutation headers', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: openSnapshot, meta: { requestId } }, { status: 201 });
		});
		installBrowser(fetcher);

		await expect(
			createAuctionRecruitment(
				guildId,
				{
					format: 'TEN_PERSON',
					semifinalBestOf: null,
					finalBestOf: 3,
					hardFearless: false,
					expectedStartAt
				},
				idempotencyKey
			)
		).resolves.toEqual(openSnapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-recruitments`);
		expect(init).toMatchObject({
			method: 'POST',
			credentials: 'same-origin',
			body: JSON.stringify({
				format: 'TEN_PERSON',
				semifinalBestOf: null,
				finalBestOf: 3,
				hardFearless: false,
				expectedStartAt
			})
		});
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('closes a full recruitment through the setup transition endpoint', async () => {
		const closedSnapshot = {
			...openSnapshot,
			recruitment: {
				...openSnapshot.recruitment,
				joinedParticipantCount: 10,
				status: 'CLOSED' as const,
				revision: 1
			},
			links: { draft: `/g/${guildId}/auction/recruitments/${recruitmentId}/draft` },
			permissions: {
				canManageMembers: false,
				canClose: false,
				canReopen: true,
				canRestore: false,
				canSendParticipantConfirmationDms: true,
				canCancel: true
			}
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: closedSnapshot, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(closeAuctionRecruitment(guildId, recruitmentId, idempotencyKey)).resolves.toEqual(
			closedSnapshot
		);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-recruitments/${recruitmentId}/close`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({});
	});

	it('requests participant confirmation DMs through the operator endpoint', async () => {
		const result = { queuedRecipientCount: 10, alreadyQueuedRecipientCount: 0 };
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: result, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(
			dispatchAuctionRecruitmentParticipantConfirmationDms(guildId, recruitmentId, idempotencyKey)
		).resolves.toEqual(result);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(
			`/api/guilds/${guildId}/auction-recruitments/${recruitmentId}/participant-confirmation-dms`
		);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({});
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('reopens a closed recruitment with the authoritative recruitment revision', async () => {
		const reopenedSnapshot = {
			...openSnapshot,
			recruitment: { ...openSnapshot.recruitment, revision: 2 },
			permissions: { ...openSnapshot.permissions, canManageMembers: true }
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: reopenedSnapshot, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(
			reopenAuctionRecruitment(guildId, recruitmentId, 1, idempotencyKey)
		).resolves.toEqual(reopenedSnapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/auction-recruitments/${recruitmentId}/reopen`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({ expectedRevision: 1 });
	});

	it('cancels and restores an auction recruitment with authoritative revisions', async () => {
		const cancelledSnapshot = {
			...openSnapshot,
			recruitment: { ...openSnapshot.recruitment, status: 'CANCELLED' as const, revision: 1 },
			permissions: {
				...openSnapshot.permissions,
				canCancel: false,
				canRestore: true
			}
		};
		const restoredSnapshot = {
			...openSnapshot,
			recruitment: { ...openSnapshot.recruitment, revision: 2 }
		};
		const fetcher = vi
			.fn()
			.mockResolvedValueOnce(Response.json({ data: cancelledSnapshot, meta: { requestId } }))
			.mockResolvedValueOnce(Response.json({ data: restoredSnapshot, meta: { requestId } }));
		installBrowser(fetcher);

		await expect(
			cancelAuctionRecruitment(guildId, recruitmentId, 0, idempotencyKey)
		).resolves.toEqual(cancelledSnapshot);
		await expect(
			restoreAuctionRecruitment(guildId, recruitmentId, 1, idempotencyKey)
		).resolves.toEqual(restoredSnapshot);

		const [cancelPath, cancelInit] = fetcher.mock.calls[0]!;
		expect(cancelPath).toBe(`/api/guilds/${guildId}/auction-recruitments/${recruitmentId}`);
		expect(cancelInit).toMatchObject({ method: 'DELETE', credentials: 'same-origin' });
		expect(JSON.parse(String(cancelInit?.body))).toEqual({ expectedRevision: 0 });

		const [restorePath, restoreInit] = fetcher.mock.calls[1]!;
		expect(restorePath).toBe(
			`/api/guilds/${guildId}/auction-recruitments/${recruitmentId}/restore`
		);
		expect(restoreInit).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(restoreInit?.body))).toEqual({ expectedRevision: 1 });
	});

	it('adds and removes a guild member through the browser operator routes', async () => {
		const operatorSnapshot = {
			...openSnapshot,
			permissions: {
				canManageMembers: true,
				canClose: false,
				canReopen: false,
				canRestore: false,
				canSendParticipantConfirmationDms: false,
				canCancel: true
			}
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: operatorSnapshot, meta: { requestId } });
		});
		installBrowser(fetcher);

		await addAuctionRecruitmentMember(
			guildId,
			recruitmentId,
			{ memberId: '1525847677542207548', declaredLane: 'MID' },
			idempotencyKey
		);
		await removeAuctionRecruitmentMember(
			guildId,
			recruitmentId,
			'1525847677542207548',
			idempotencyKey
		);

		const [addPath, addInit] = fetcher.mock.calls[0]!;
		expect(addPath).toBe(`/api/guilds/${guildId}/auction-recruitments/${recruitmentId}/members`);
		expect(addInit).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(addInit?.body))).toEqual({
			memberId: '1525847677542207548',
			declaredLane: 'MID'
		});

		const [removePath, removeInit] = fetcher.mock.calls[1]!;
		expect(removePath).toBe(
			`/api/guilds/${guildId}/auction-recruitments/${recruitmentId}/members/1525847677542207548`
		);
		expect(removeInit).toMatchObject({ method: 'DELETE', credentials: 'same-origin' });
		expect(JSON.parse(String(removeInit?.body))).toEqual({});
	});

	it('preserves backend failure details and the request ID', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'recruitment_channel_required',
						message: '경매 모집 채널을 먼저 설정해 주세요.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await createAuctionRecruitment(
			guildId,
			{
				format: 'TWENTY_PERSON',
				semifinalBestOf: 3,
				finalBestOf: 5,
				hardFearless: true,
				expectedStartAt
			},
			idempotencyKey
		).catch((cause) => cause);
		expect(error).toMatchObject({ status: 409, code: 'recruitment_channel_required', requestId });
		expect(auctionRecruitmentErrorText(error)).toBe(
			`경매 모집 채널을 먼저 설정해 주세요. (요청 ID: ${requestId})`
		);
	});

	it('identifies optimistic revision conflicts so retries can use a new idempotency key', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'recruitment_revision_conflict',
						message: '경매 모집 상태가 변경되었습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await cancelAuctionRecruitment(guildId, recruitmentId, 0, idempotencyKey).catch(
			(cause) => cause
		);

		expect(isAuctionRecruitmentRevisionConflict(error)).toBe(true);
	});
});
