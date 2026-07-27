import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	addAuctionRecruitmentMember,
	auctionRecruitmentErrorText,
	closeAuctionRecruitment,
	createAuctionRecruitment,
	removeAuctionRecruitmentMember
} from './auctionRecruitment';

const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000501';
const requestId = '01900000-0000-7000-8000-000000000001';
const idempotencyKey = '01900000-0000-7000-8000-000000000002';
const expectedStartAt = '2026-07-26T11:00:00Z';
const expectedStartLabel = '2026-07-26T20:00:00+09:00 (KST)';

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
	permissions: { canManageMembers: false, canClose: false, canCancel: true }
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
			permissions: { canManageMembers: false, canClose: false, canCancel: true }
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

	it('adds and removes a guild member through the browser operator routes', async () => {
		const operatorSnapshot = {
			...openSnapshot,
			permissions: { canManageMembers: true, canClose: false, canCancel: true }
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
});
