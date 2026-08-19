import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { render } from 'svelte/server';
import Page from './+page.svelte';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');
const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000501';

function renderPage(
	status: 'OPEN' | 'CLOSED' | 'CANCELLED',
	canClose = false,
	canCancel = false,
	canManageMembers = false,
	joinedParticipantCount = 1,
	canRestore = false
) {
	return render(Page, {
		props: {
			data: {
				guildId,
				snapshot: {
					recruitment: {
						id: recruitmentId,
						displayNumber: 42,
						title: '수요 10인 경매',
						format: 'TEN_PERSON',
						rules: { hardFearless: false, semifinalBestOf: null, finalBestOf: 3 },
						participantCount: 10,
						joinedParticipantCount,
						teamCount: 2,
						status,
						revision: 1,
						channelId: '1525900000000000001',
						messageId: '1525900000000000002',
						createdById: '1525847677542207548',
						updatedAt: '2026-07-14T12:00:00Z',
						href: `/g/${guildId}/auction/recruitments/${recruitmentId}`
					},
					participants: Array.from({ length: joinedParticipantCount }, (_, index) => ({
						id: index === 0 ? '1525847677542207548' : `dummy-${index}`,
						displayName: index === 0 ? 'Longcat' : `테스트 플레이어 ${index}`,
						declaredLane: 'MID'
					})),
					links: {
						draft:
							status === 'CLOSED'
								? `/g/${guildId}/auction/recruitments/${recruitmentId}/draft`
								: null
					},
					permissions: {
						canManageMembers,
						canClose,
						canReopen: false,
						canRestore,
						canSendParticipantConfirmationDms: false,
						canCancel
					}
				}
			}
		} as never
	});
}

describe('auction recruitment page', () => {
	it('keeps open participation in Discord without privileged controls', () => {
		const { body } = renderPage('OPEN');

		expect(body).toContain('경매 모집');
		expect(body).toContain('Discord 모집 메시지에서 진행합니다.');
		expect(body).toContain('Longcat');
		expect(body).toContain('신청 라인 · 미드');
		expect(body).not.toContain('모집 마감');
		expect(body).not.toContain('경매 모집 취소');
	});

	it('uses server permissions for close and cancellation actions', () => {
		const { body } = renderPage('OPEN', true, true, false, 10);

		expect(body).toContain('모집 마감');
		expect(body).toContain('경매 모집 취소');
		expect(source).toContain('snapshot.permissions.canClose');
		expect(source).toContain('snapshot.permissions.canCancel');
		expect(source).toContain('cancelConfirmationValid');
	});

	it('keeps cancelled recruitment data reachable and exposes restoration', () => {
		const { body } = renderPage('CANCELLED', false, false, false, 10, true);

		expect(body).toContain('취소 이력으로 보존');
		expect(body).toContain('취소 되돌리기');
		expect(source).toContain('restoreAuctionRecruitment');
	});

	it('waits for a full roster before exposing the close action', () => {
		const { body } = renderPage('OPEN', true, true);

		expect(body).not.toContain('모집 마감');
		expect(source).toContain('canCloseRecruitment');
	});

	it('uses the explicit server capability for operator roster controls', () => {
		const { body } = renderPage('OPEN', false, false, true);

		expect(body).toContain('참가자 추가');
		expect(source).toContain('snapshot.permissions.canManageMembers');
	});

	it('opens the setup route only after the server marks the recruitment closed', () => {
		const { body } = renderPage('CLOSED');

		expect(body).toContain('마감 명단 보기');
		expect(body).toContain(`href="/g/${guildId}/auction/recruitments/${recruitmentId}/draft"`);
	});

	it('shows the selected recruitment rules once in the workspace header', () => {
		const { body } = renderPage('OPEN');

		expect(body).toContain('BO3 · 일반');
	});
});
