import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	applyDummyRecruitmentParticipants,
	fetchDummyRecruitmentParticipants,
	previewDummyRecruitmentParticipants
} from './dummyRecruitment';

const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000020';
const userId = 'dmy_01900000-0000-7000-8000-000000000001';
const requestId = '01900000-0000-7000-8000-000000000099';
const idempotencyKey = '01900000-0000-7000-8000-000000000301';
const overview = {
	recruitment: {
		id: recruitmentId,
		title: '일반 내전 모집',
		status: 'OPEN',
		targetCount: 10,
		joinedCount: 9,
		remainingSlots: 1,
		revision: 4
	},
	candidates: [
		{
			userId,
			displayName: '테스트 미드 01',
			mainLane: 'MID',
			profileState: 'ACTIVE',
			membershipStatus: 'ACTIVE',
			participationState: 'AVAILABLE'
		}
	]
};

describe('dummy recruitment participant API', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('supports read, data-level preview issues and atomic apply with idempotency', async () => {
		const preview = {
			action: 'ADD',
			canApply: true,
			currentRevision: 4,
			projectedJoinedCount: 10,
			remainingSlots: 0,
			selected: overview.candidates,
			issues: []
		};
		const fetcher = vi
			.fn<typeof globalThis.fetch>()
			.mockResolvedValueOnce(Response.json({ data: overview, meta: { requestId } }))
			.mockResolvedValueOnce(Response.json({ data: preview, meta: { requestId } }))
			.mockResolvedValueOnce(
				Response.json({
					data: {
						action: 'ADD',
						changedUserIds: [userId],
						overview: {
							...overview,
							recruitment: { ...overview.recruitment, joinedCount: 10, remainingSlots: 0 }
						}
					},
					meta: { requestId }
				})
			);
		vi.stubGlobal('document', { cookie: 'monkeybot_csrf=csrf-token' });
		vi.stubGlobal('fetch', fetcher);

		await expect(fetchDummyRecruitmentParticipants(guildId, recruitmentId)).resolves.toEqual(
			overview
		);
		await expect(
			previewDummyRecruitmentParticipants(guildId, recruitmentId, 'ADD', [userId])
		).resolves.toEqual(preview);
		await applyDummyRecruitmentParticipants(
			guildId,
			recruitmentId,
			'ADD',
			[userId],
			4,
			idempotencyKey
		);

		const base = `/api/dev/guilds/${guildId}/recruitments/${recruitmentId}/participants`;
		expect(fetcher.mock.calls[0]![0]).toBe(base);
		expect(fetcher.mock.calls[1]![0]).toBe(`${base}/preview`);
		expect(JSON.parse(String(fetcher.mock.calls[1]![1]?.body))).toEqual({
			action: 'ADD',
			userIds: [userId]
		});
		const applyHeaders = new Headers(fetcher.mock.calls[2]![1]?.headers);
		expect(applyHeaders.get('Idempotency-Key')).toBe(idempotencyKey);
		expect(applyHeaders.get('X-CSRF-Token')).toBe('csrf-token');
		expect(JSON.parse(String(fetcher.mock.calls[2]![1]?.body))).toEqual({
			userIds: [userId],
			expectedRevision: 4
		});
	});
});
