import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import Page from './+page.svelte';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');

const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const creatorId = '1525900000000000005';
const viewerId = '1525900000000000006';

function snapshot(status: 'OPEN' | 'CLOSED', participantCount = 2) {
	const participants = Array.from({ length: participantCount }, (_, index) => ({
		id: `15259000000000000${index + 3}`,
		displayName: index === 0 ? 'Longcat' : index === 1 ? '탐솔' : `참가자 ${index + 1}`,
		preferredLanes: index === 0 ? ['MID'] : []
	}));

	return {
		recruitment: {
			id: recruitmentId,
			displayNumber: 214,
			title: '금요 일반 내전 모집',
			status,
			targetCount: 10,
			bestOf: 3,
			hardFearless: false,
			revision: 3,
			createdById: creatorId,
			channelId: '1525900000000000001',
			messageId: '1525900000000000002',
			updatedAt: '2026-07-14T12:00:00Z'
		},
		participants,
		links: { entry: `/g/${guildId}/recruitments/${recruitmentId}/entry` }
	};
}

function renderPage(
	status: 'OPEN' | 'CLOSED',
	currentUserRole: 'MEMBER' | 'OPERATOR' = 'MEMBER',
	discordId = viewerId,
	participantCount = 2
) {
	return render(Page, {
		props: {
			data: {
				guildId,
				guild: { currentUserRole },
				user: { discordId },
				snapshot: snapshot(status, participantCount)
			}
		} as never
	});
}

describe('recruitment stage page', () => {
	it('subscribes operators to recruitment invalidations and refreshes the authoritative snapshot', () => {
		expect(source).toContain("from '$lib/realtime/browserSocket'");
		expect(source).toContain("from '$lib/realtime/recruitment'");
		expect(source).toContain('if (!isOperator) return;');
		expect(source).toContain('topics: [topic]');
		expect(source).toContain(
			'onInvalidate: (event) => handleRealtimeInvalidation(event, route, topic)'
		);
		expect(source).toContain('fetchRecruitmentSnapshot(route.guildId, route.recruitmentId)');
	});

	it('shows an open recruitment roster without privileged controls for other members', () => {
		const { body } = renderPage('OPEN');

		expect(body).toContain('엔트리 모집');
		expect(body).toContain('2명');
		expect(body).toContain('Longcat');
		expect(body).toContain('선호 라인 · 미드');
		expect(body).toContain('선호 라인 · 미설정');
		expect(body).not.toContain('엔트리 편성 열기');
		expect(body).not.toContain('Discord 모집 현황');
		expect(body).not.toContain('Discord 모집 중');
		expect(body).not.toContain('참가자 추가');
		expect(body).not.toContain('모집 삭제');
	});

	it('exposes compact operator recruitment actions without a dummy-specific panel', () => {
		const { body } = renderPage('OPEN', 'OPERATOR');

		expect(body).toContain('참가자 추가');
		expect(body).toContain('모집 삭제');
		expect(body).toContain('Longcat 참가자에서 제거');
		expect(body).toContain('탐솔 참가자에서 제거');
		expect(body).not.toContain('더미 참가자');
	});

	it('lets a non-operator creator delete their open recruitment without member controls', () => {
		const { body } = renderPage('OPEN', 'MEMBER', creatorId);

		expect(body).toContain('모집 삭제');
		expect(body).not.toContain('참가자 추가');
		expect(body).not.toContain('Longcat 참가자에서 제거');
	});

	it('only lets an operator start entry editing when the open roster is exactly full', () => {
		const fullOperator = renderPage('OPEN', 'OPERATOR', viewerId, 10).body;
		const partialOperator = renderPage('OPEN', 'OPERATOR', viewerId, 9).body;
		const fullMember = renderPage('OPEN', 'MEMBER', viewerId, 10).body;

		expect(fullOperator).toContain('엔트리 편성 시작');
		expect(partialOperator).not.toContain('엔트리 편성 시작');
		expect(fullMember).not.toContain('엔트리 편성 시작');
	});

	it('only exposes entry editing after the recruitment closes', () => {
		const { body } = renderPage('CLOSED');

		expect(body).toContain('엔트리 편성 열기');
		expect(body).toContain(`href="/g/${guildId}/recruitments/${recruitmentId}/entry"`);
		expect(body).not.toContain('Discord 모집 현황');
	});
});
