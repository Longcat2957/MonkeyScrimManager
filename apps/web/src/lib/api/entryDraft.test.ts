import { afterEach, describe, expect, it, vi } from 'vitest';
import type { EntryDraftSnapshot } from '$lib/contracts/entryDraft';
import {
	createSeriesFromEntryDraft,
	entryDraftErrorText,
	fetchEntryDraft,
	isEntryDraftRevisionConflict,
	moveEntryDraftParticipant,
	reopenRecruitmentForEntry,
	resetEntryDraft
} from './entryDraft';

const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const participantId = '1525847677542207548';
const requestId = '01900000-0000-7000-8000-000000000001';
const idempotencyKey = '01900000-0000-7000-8000-000000000002';

const snapshot: EntryDraftSnapshot = {
	recruitment: {
		id: recruitmentId,
		displayNumber: 214,
		title: '금요 일반 내전 모집',
		status: 'CLOSED',
		targetCount: 10,
		bestOf: 3,
		hardFearless: false,
		revision: 3,
		updatedAt: '2026-07-14T12:00:00Z'
	},
	draft: {
		revision: 4,
		updatedAt: '2026-07-14T12:01:00Z',
		updatedBy: { id: participantId, displayName: 'Longcat' },
		participants: [
			{
				id: participantId,
				displayName: 'Longcat',
				preferredLanes: ['MID'],
				mainLane: 'MID',
				mmr: 1623,
				games: 12,
				wins: 7,
				profileIconId: 588,
				riotTier: 'P3',
				championIds: ['Ahri', 'Orianna']
			}
		],
		assignments: [{ participantId, team: 'TEAM_1', lane: 'MID' }]
	},
	capabilities: {
		canEdit: true,
		canReset: true,
		canCreateSeries: false,
		canReopen: true,
		canCancel: true
	}
};

function envelope(data: EntryDraftSnapshot = snapshot): Response {
	return Response.json({ data, meta: { requestId } });
}

function installBrowser(fetcher: typeof globalThis.fetch, cookie = 'monkeybot_csrf=csrf-token') {
	vi.stubGlobal('document', { cookie });
	vi.stubGlobal('fetch', fetcher);
}

describe('entry draft API client', () => {
	afterEach(() => {
		vi.unstubAllGlobals();
		vi.restoreAllMocks();
	});

	it('loads the strict authoritative entry snapshot through the same-origin route', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return envelope();
		});
		installBrowser(fetcher);

		await expect(fetchEntryDraft(guildId, recruitmentId)).resolves.toEqual(snapshot);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/recruitments/${recruitmentId}/entry-draft`);
		expect(init).toMatchObject({ credentials: 'same-origin' });
		expect(new Headers(init?.headers).get('accept')).toBe('application/json');
	});

	it('moves a participant with the current draft revision, CSRF, and idempotency key', async () => {
		const authoritative: EntryDraftSnapshot = {
			...snapshot,
			draft: {
				...snapshot.draft,
				revision: 5,
				assignments: [{ participantId, team: 'TEAM_2', lane: 'TOP' }]
			}
		};
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return envelope(authoritative);
		});
		installBrowser(fetcher, 'theme=dark; monkeybot_csrf=csrf-token');

		await expect(
			moveEntryDraftParticipant(guildId, recruitmentId, {
				expectedRevision: 4,
				participantId,
				slot: 'TEAM_2_TOP',
				idempotencyKey
			})
		).resolves.toEqual(authoritative);

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/recruitments/${recruitmentId}/entry-draft`);
		expect(init).toMatchObject({ method: 'PATCH', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({
			expectedRevision: 4,
			operation: { type: 'MOVE_PARTICIPANT', participantId, slot: 'TEAM_2_TOP' }
		});
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('uses the reset and reopen subroutes with the matching expected revision', async () => {
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return envelope();
		});
		installBrowser(fetcher);

		await resetEntryDraft(guildId, recruitmentId, { expectedRevision: 4, idempotencyKey });
		await reopenRecruitmentForEntry(guildId, recruitmentId, {
			expectedRevision: 3,
			idempotencyKey
		});

		const [resetPath, resetInit] = fetcher.mock.calls[0]!;
		expect(resetPath).toBe(
			`/api/guilds/${guildId}/recruitments/${recruitmentId}/entry-draft/reset`
		);
		expect(resetInit).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(resetInit?.body))).toEqual({ expectedRevision: 4 });

		const [reopenPath, reopenInit] = fetcher.mock.calls[1]!;
		expect(reopenPath).toBe(`/api/guilds/${guildId}/recruitments/${recruitmentId}/reopen`);
		expect(reopenInit).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(reopenInit?.body))).toEqual({ expectedRevision: 3 });
	});

	it('creates a series from the complete entry draft with the recruitment revision', async () => {
		const seriesId = '01900000-0000-7000-8000-000000000303';
		const href = `/g/${guildId}/series/${seriesId}/pick-ban`;
		const fetcher = vi.fn(async (...args: Parameters<typeof globalThis.fetch>) => {
			void args;
			return Response.json({ data: { series: { id: seriesId, href } }, meta: { requestId } });
		});
		installBrowser(fetcher);

		await expect(
			createSeriesFromEntryDraft(guildId, {
				sourceRecruitmentId: recruitmentId,
				expectedRevision: 3,
				idempotencyKey
			})
		).resolves.toEqual({ id: seriesId, href });

		const [path, init] = fetcher.mock.calls[0]!;
		expect(path).toBe(`/api/guilds/${guildId}/series`);
		expect(init).toMatchObject({ method: 'POST', credentials: 'same-origin' });
		expect(JSON.parse(String(init?.body))).toEqual({
			sourceRecruitmentId: recruitmentId,
			expectedRevision: 3
		});
		const headers = new Headers(init?.headers);
		expect(headers.get('X-CSRF-Token')).toBe('csrf-token');
		expect(headers.get('Idempotency-Key')).toBe(idempotencyKey);
	});

	it('keeps the backend reason and request ID on a revision conflict', async () => {
		const fetcher = vi.fn(async () =>
			Response.json(
				{
					error: {
						code: 'entry_draft_revision_conflict',
						message: '다른 운영자가 먼저 엔트리를 변경했습니다.',
						requestId
					}
				},
				{ status: 409 }
			)
		);
		installBrowser(fetcher);

		const error = await resetEntryDraft(guildId, recruitmentId, {
			expectedRevision: 4,
			idempotencyKey
		}).catch((cause) => cause);

		expect(error).toMatchObject({
			status: 409,
			code: 'entry_draft_revision_conflict',
			requestId
		});
		expect(entryDraftErrorText(error)).toBe(
			`다른 운영자가 먼저 엔트리를 변경했습니다. (요청 ID: ${requestId})`
		);
		expect(isEntryDraftRevisionConflict(error)).toBe(true);
		expect(
			isEntryDraftRevisionConflict({
				status: 409,
				code: 'entry_draft_not_editable'
			})
		).toBe(false);
	});
});
