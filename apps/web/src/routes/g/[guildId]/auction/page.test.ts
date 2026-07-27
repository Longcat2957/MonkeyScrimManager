import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import Page from './+page.svelte';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');
const guildId = '1525851181157580861';
const tournamentId = '01900000-0000-7000-8000-000000000601';
const matchId = '01900000-0000-7000-8000-000000000701';

describe('auction page', () => {
	it('opens the canonical recruitment link returned by the create response', () => {
		expect(source).toContain('let createDialogOpen = $derived(Boolean(data.openCreateDialog));');
		expect(source).toContain('let createHardFearless = $state(true);');
		expect(source).toContain('let createExpectedStartAt = $state');
		expect(source).toContain('expectedStartAt');
		expect(source).toContain('await goto(created.recruitment.href)');
		expect(source).not.toContain('id: created.recruitment.id');
	});

	it('opens the concise tournament bracket before a match workspace for in-game tournaments', () => {
		const { body } = render(Page, {
			props: {
				data: {
					guildId,
					auctionPages: {
						actionableRecruitments: { recruitments: [], nextCursor: null },
						activeTournaments: {
							tournaments: [
								{
									id: tournamentId,
									displayNumber: 118,
									title: '토요 20인 경매',
									format: 'TWENTY_PERSON',
									participantCount: 20,
									teamCount: 4,
									status: 'IN_GAME',
									revision: 7,
									activeMemberCount: 20,
									bidMemberCount: 16,
									incompleteMatchCount: 1,
									nextMatch: {
										id: matchId,
										label: '결승',
										matchKey: 'FINAL',
										status: 'IN_PROGRESS',
										confirmedGameCount: 1
									},
									championTeam: null,
									updatedAt: '2026-07-15T12:00:00Z',
									endedAt: null,
									href: `/g/${guildId}/auction/matches/${matchId}`
								}
							],
							nextCursor: null
						},
						completedTournaments: { tournaments: [], nextCursor: null }
					}
				}
			} as never
		});

		expect(body).toContain('경기 선택');
		expect(body).not.toContain('작업 열기');
		expect(body).not.toContain('토너먼트 현황');
		expect(body).toContain(`href="/g/${guildId}/auction/tournaments/${tournamentId}/result"`);
		expect(body).not.toContain(`href="/g/${guildId}/auction/matches/${matchId}"`);
	});
});
