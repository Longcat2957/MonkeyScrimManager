import { describe, expect, it } from 'vitest';

import type { RecruitmentSnapshot } from '../api/contracts.js';
import { renderRecruitmentMessage } from './recruitment-message.js';

const guildId = '200000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const webOrigin = new URL('https://dev.monkeybot.lol');

function snapshot(
	status: 'OPEN' | 'CLOSED' | 'CANCELLED',
	participantCount: number
): RecruitmentSnapshot {
	return {
		recruitment: {
			id: recruitmentId,
			displayNumber: 214,
			title: '금요 정기 내전',
			status,
			targetCount: 10,
			revision: 3,
			channelId: null,
			messageId: null,
			createdById: '100000000000000001',
			updatedAt: '2026-07-13T10:00:00Z'
		},
		participants: Array.from({ length: participantCount }, (_, index) => ({
			id: String(100000000000000001n + BigInt(index)),
			displayName: `참가자 ${index + 1}`,
			preferredLanes: index === 0 ? (['MID', 'JUNGLE'] as const) : ([] as const)
		})),
		links: { entry: `/g/${guildId}/recruitments/${recruitmentId}/entry` }
	};
}

function json(value: unknown): Record<string, unknown> {
	return JSON.parse(JSON.stringify(value)) as Record<string, unknown>;
}

describe('renderRecruitmentMessage', () => {
	it('renders join, leave, and lane controls for an open recruitment', () => {
		const rendered = json(renderRecruitmentMessage(snapshot('OPEN', 1), webOrigin));
		const serialized = JSON.stringify(rendered);

		expect(serialized).toContain('5v5 내전 모집 #214');
		expect(serialized).toContain('참가자 1');
		expect(serialized).toContain('미드 / 정글');
		expect(serialized).toContain(`recruit:join:${recruitmentId}`);
		expect(serialized).toContain(`recruit:leave:${recruitmentId}`);
		expect(serialized).toContain(`recruit:roles:${recruitmentId}`);
	});

	it('renders a dummy participant by display name without treating its ID as a Discord mention', () => {
		const withDummy = snapshot('OPEN', 1);
		withDummy.participants[0] = {
			id: 'dmy_01900000-0000-7000-8000-000000000001',
			displayName: 'Dummy Mid',
			preferredLanes: ['MID']
		};
		const serialized = JSON.stringify(json(renderRecruitmentMessage(withDummy, webOrigin)));

		expect(serialized).toContain('Dummy Mid');
		expect(serialized).not.toContain('dmy_01900000-0000-7000-8000-000000000001');
		expect(serialized).not.toContain('<@');
	});

	it('keeps leave and lane controls available when the roster reaches ten', () => {
		const rendered = json(renderRecruitmentMessage(snapshot('OPEN', 10), webOrigin));
		const serialized = JSON.stringify(rendered);

		expect(serialized).toContain('정원 도달');
		expect(serialized).toContain(`recruit:start-entry:${recruitmentId}`);
		expect(serialized).toContain(`recruit:leave:${recruitmentId}`);
		expect(serialized).toContain(`recruit:roles:${recruitmentId}`);
		expect(serialized).not.toContain(`recruit:join:${recruitmentId}`);
	});

	it('renders an API-owned entry link for a closed recruitment', () => {
		const rendered = json(renderRecruitmentMessage(snapshot('CLOSED', 10), webOrigin));
		const serialized = JSON.stringify(rendered);

		expect(serialized).toContain('엔트리 편성 중');
		expect(serialized).toContain(
			`https://dev.monkeybot.lol/g/${guildId}/recruitments/${recruitmentId}/entry`
		);
		expect(serialized).not.toContain('custom_id');
	});

	it('does not render controls for a cancelled recruitment', () => {
		const rendered = json(renderRecruitmentMessage(snapshot('CANCELLED', 2), webOrigin));
		expect(rendered.components).toEqual([]);
	});
});
