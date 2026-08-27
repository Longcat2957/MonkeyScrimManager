import { MessageFlags } from 'discord.js';
import { describe, expect, it } from 'vitest';

import type { AuctionRecruitmentSnapshot } from '../api/contracts.js';
import { renderAuctionRecruitmentMessage } from './auction-recruitment-message.js';

const guildId = '200000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const tournamentId = '01900000-0000-7000-8000-000000000201';
const webOrigin = new URL('https://dev.monkeybot.lol');
const expectedStartLabel = '2026-07-26 20:00 (KST)';

function snapshot(
	status: AuctionRecruitmentSnapshot['recruitment']['status'],
	participantCount: number,
	format: AuctionRecruitmentSnapshot['recruitment']['format'] = 'TEN_PERSON'
): AuctionRecruitmentSnapshot {
	return {
		recruitment: {
			id: recruitmentId,
			displayNumber: 214,
			title: `금요 경매 내전 · 예상 시작 ${expectedStartLabel}`,
			format,
			rules:
				format === 'TEN_PERSON'
					? { hardFearless: false, semifinalBestOf: null, finalBestOf: 3 }
					: { hardFearless: true, semifinalBestOf: 1, finalBestOf: 5 },
			participantCount: format === 'TEN_PERSON' ? 10 : 20,
			joinedParticipantCount: participantCount,
			teamCount: format === 'TEN_PERSON' ? 2 : 4,
			status,
			revision: 3,
			channelId: null,
			messageId: null,
			createdById: '100000000000000001',
			updatedAt: '2026-07-14T10:00:00Z',
			href:
				status === 'CONVERTED'
					? `/g/${guildId}/auction/tournaments/${tournamentId}/draft`
					: `/g/${guildId}/auction/recruitments/${recruitmentId}`
		},
		participants: Array.from({ length: participantCount }, (_, index) => ({
			id: String(100000000000000001n + BigInt(index)),
			displayName: `참가자 ${index + 1}`,
			declaredLane: index % 2 === 0 ? ('MID' as const) : null
		})),
		links: {
			draft:
				status === 'CLOSED' ? `/g/${guildId}/auction/recruitments/${recruitmentId}/draft` : null
		},
		permissions: { canManageMembers: false, canClose: false, canCancel: true }
	};
}

function json(value: unknown): Record<string, unknown> {
	return JSON.parse(JSON.stringify(value)) as Record<string, unknown>;
}

describe('renderAuctionRecruitmentMessage', () => {
	it('renders join, leave, and one-lane join controls for an open recruitment', () => {
		const rendered = json(renderAuctionRecruitmentMessage(snapshot('OPEN', 1), webOrigin));
		const serialized = JSON.stringify(rendered);

		expect(rendered.flags).toBe(MessageFlags.IsComponentsV2);
		expect(rendered).not.toHaveProperty('embeds');
		expect(serialized).toContain('경매 내전 모집 #214');
		expect(serialized).toContain(`auction-recruit:join:${recruitmentId}`);
		expect(serialized).toContain(`auction-recruit:leave:${recruitmentId}`);
		expect(serialized).toContain(`auction-recruit:lane:${recruitmentId}`);
		expect(serialized).toContain('참가 라인 선택 후 참가');
		expect(serialized).toContain('"max_values":1');
		expect(serialized).toContain('BO3 · 일반');
		expect(serialized).toContain(expectedStartLabel);
	});

	it('keeps leave and lane controls but hides join after a full 20-person roster', () => {
		const rendered = json(
			renderAuctionRecruitmentMessage(snapshot('OPEN', 20, 'TWENTY_PERSON'), webOrigin)
		);
		const serialized = JSON.stringify(rendered);

		expect(serialized).toContain('정원 도달');
		expect(serialized).toContain(`auction-recruit:leave:${recruitmentId}`);
		expect(serialized).toContain(`auction-recruit:lane:${recruitmentId}`);
		expect(serialized).not.toContain(`auction-recruit:join:${recruitmentId}`);
		expect(serialized).toContain('4강 BO1 · 결승 BO5 · 하드 피어리스');
	});

	it('renders the server-owned setup link without participant controls after closing', () => {
		const rendered = json(renderAuctionRecruitmentMessage(snapshot('CLOSED', 10), webOrigin));
		const serialized = JSON.stringify(rendered);

		expect(serialized).toContain('마감 명단 열기');
		expect(serialized).toContain(
			`https://dev.monkeybot.lol/g/${guildId}/auction/recruitments/${recruitmentId}/draft`
		);
		expect(serialized).not.toContain('auction-recruit:');
	});

	it('hands a converted recruitment off to the actual auction tournament', () => {
		const rendered = json(renderAuctionRecruitmentMessage(snapshot('CONVERTED', 10), webOrigin));
		const serialized = JSON.stringify(rendered);

		expect(serialized).toContain('경매 진행 중');
		expect(serialized).toContain('경매 내전 열기');
		expect(serialized).toContain(
			`https://dev.monkeybot.lol/g/${guildId}/auction/tournaments/${tournamentId}/draft`
		);
		expect(serialized).not.toContain(`/auction/recruitments/${recruitmentId}/draft`);
		expect(serialized).not.toContain('auction-recruit:');
	});

	it('splits a legal 20-person, 100-character-name roster into readable text displays', () => {
		const current = snapshot('OPEN', 20, 'TWENTY_PERSON');
		const names = current.participants.map((participant, index) => {
			const name = `P${String(index + 1).padStart(2, '0')}${'x'.repeat(97)}`;
			participant.displayName = name;
			return name;
		});
		const rendered = renderAuctionRecruitmentMessage(current, webOrigin);
		const firstComponent = rendered.components[0]?.toJSON() as {
			components?: Array<{ content?: string }>;
		};
		const rosterDisplays = (firstComponent.components ?? []).filter((component) =>
			component.content?.startsWith('`')
		);

		expect(rosterDisplays).toHaveLength(3);
		for (const display of rosterDisplays) {
			expect(display.content?.length).toBeLessThanOrEqual(1_024);
		}
		const rosterText = rosterDisplays.map((display) => display.content).join('\n');
		for (const name of names) {
			expect(rosterText).toContain(name);
		}
	});
});
