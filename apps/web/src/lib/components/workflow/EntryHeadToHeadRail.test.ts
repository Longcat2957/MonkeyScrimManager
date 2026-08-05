import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import type { EntryDraftHeadToHeadRecord } from '$lib/contracts/entryDraft';
import type { EntrySlot, MockEntryParticipant } from '$lib/fixtures/workflow';
import EntryHeadToHeadRail from './EntryHeadToHeadRail.svelte';

const participants: MockEntryParticipant[] = [
	{
		id: '100000000000000001',
		name: '팀 1 탑',
		roles: ['TOP'],
		mainRole: 'TOP',
		mmr: 1500,
		recentRecord: '내전 4승 1패'
	},
	{
		id: '100000000000000002',
		name: '팀 2 탑',
		roles: ['TOP'],
		mainRole: 'TOP',
		mmr: 1500,
		recentRecord: '내전 1승 4패'
	}
];

const headToHead: EntryDraftHeadToHeadRecord[] = [
	{
		lane: 'TOP',
		userAId: participants[0].id,
		userBId: participants[1].id,
		plays: 5,
		userAWins: 4,
		userBWins: 1
	}
];

function renderRail(assignments: Record<string, EntrySlot>) {
	return render(EntryHeadToHeadRail, {
		props: {
			activeLanes: ['TOP'],
			assignments,
			participants,
			headToHead
		}
	}).body;
}

describe('entry head-to-head rail', () => {
	it('shows the record from TEAM_1 player perspective', () => {
		const body = renderRail({
			[participants[0].id]: 'TEAM_1_TOP',
			[participants[1].id]: 'TEAM_2_TOP'
		});

		expect(body).toContain('탑 상대전적: 5게임 4승 1패, 3승 우위');
	});

	it('reverses wins and losses when the same pair swaps sides', () => {
		const body = renderRail({
			[participants[0].id]: 'TEAM_2_TOP',
			[participants[1].id]: 'TEAM_1_TOP'
		});

		expect(body).toContain('탑 상대전적: 5게임 1승 4패, 3승 우위');
	});
});
