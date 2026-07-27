import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import Page from './+page.svelte';

const guildId = '1525851181157580861';
const recruitmentId = '01900000-0000-7000-8000-000000000501';

describe('auction recruitment closed-roster page', () => {
	it('renders the persisted captain and starting-point setup workflow', () => {
		const { body } = render(Page, {
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
							joinedParticipantCount: 10,
							teamCount: 2,
							status: 'CLOSED',
							revision: 1,
							channelId: '1525900000000000001',
							messageId: '1525900000000000002',
							createdById: '1525847677542207548',
							updatedAt: '2026-07-14T12:00:00Z',
							href: `/g/${guildId}/auction/recruitments/${recruitmentId}`
						},
						participants: [
							{ id: '1525847677542207548', displayName: 'Longcat', declaredLane: 'MID' }
						],
						links: { draft: `/g/${guildId}/auction/recruitments/${recruitmentId}/draft` },
						permissions: {
							canManageMembers: false,
							canClose: false,
							canSendParticipantConfirmationDms: true,
							canCancel: false
						}
					},
					setup: {
						draft: {
							recruitmentId,
							revision: 4,
							teamCount: 2,
							teams: [
								{ teamIndex: 1, teamName: '1팀', captainUserId: null, initialPoints: 1000 },
								{ teamIndex: 2, teamName: '2팀', captainUserId: null, initialPoints: 1000 }
							]
						},
						participants: [
							{ id: '1525847677542207548', displayName: 'Longcat', declaredLane: 'MID' }
						],
						permissions: { canEdit: true, canCreateTournament: true }
					}
				}
			} as never
		});

		expect(body).toContain('경매 준비');
		expect(body).toContain('BO3 · 일반');
		expect(body).toContain('토너먼트 시작 설정');
		expect(body).toContain('Longcat');
		expect(body).toContain('신청 라인 · 미드');
		expect(body).toContain('입찰 시작');
		expect(body).toContain('참가 확정 DM 발송');
		expect(body).not.toContain('실시간 편집');
	});
});
