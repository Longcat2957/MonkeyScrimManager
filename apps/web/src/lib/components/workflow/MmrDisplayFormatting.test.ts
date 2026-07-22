import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import type { MockEntryParticipant } from '$lib/fixtures/workflow';
import SeriesCompletedEntryRosterBoard from './SeriesCompletedEntryRosterBoard.svelte';
import { entryParticipantDetail } from './entryDraft';

const decimalParticipant: MockEntryParticipant = {
	id: '390526792496054283',
	name: 'Longcat',
	roles: ['TOP'],
	mainRole: 'TOP',
	mmr: 1618.7499999999998,
	recentRecord: '최근 5승 3패',
	riotTier: 'D4'
};

describe('MMR display formatting', () => {
	it('formats entry summaries and completed rosters without exposing float tails', () => {
		const { body } = render(SeriesCompletedEntryRosterBoard, {
			props: {
				participants: [decimalParticipant],
				assignments: { [decimalParticipant.id]: 'TEAM_1_TOP' },
				champions: []
			}
		});

		expect(entryParticipantDetail(decimalParticipant)).toContain('MMR 1,619');
		expect(body).toContain('평균 1,619');
		expect(body).toContain('1,619');
		expect(body).not.toContain('1618.7499999999998');
	});

	it('uses the shared signed formatter for individual and team result ledgers', () => {
		const source = readFileSync(
			new URL('./SeriesResultLedgerBoard.svelte', import.meta.url),
			'utf8'
		);

		expect(source).toContain('formatSignedMmr(teamMmr');
		expect(source).toContain('formatSignedMmr(change.delta)');
		expect(source).not.toContain('.toFixed(');
	});
});
