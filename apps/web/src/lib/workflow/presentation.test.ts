import { describe, expect, it } from 'vitest';
import {
	buildAuctionView,
	buildSeriesView,
	formatRelativeTime,
	mapOperations
} from './presentation';
import type { AuctionTournamentDto, OperationDto, RecruitmentDto } from '$lib/contracts/workflow';

const now = Date.parse('2026-07-12T12:00:00Z');

describe('workflow presentation', () => {
	it('formats backend UTC timestamps with one stable Korean clock', () => {
		expect(formatRelativeTime('2026-07-12T11:59:40Z', now)).toBe('방금');
		expect(formatRelativeTime('2026-07-12T11:30:00Z', now)).toBe('30분 전');
		expect(formatRelativeTime('2026-07-12T09:00:00Z', now)).toBe('3시간 전');
		expect(formatRelativeTime('2026-07-10T12:00:00Z', now)).toBe('2일 전');
		expect(formatRelativeTime('invalid', now)).toBe('-');
	});

	it('maps backend operation stages to concrete actions and priority tones', () => {
		const operation = (
			kind: OperationDto['kind'],
			status: string,
			priority: OperationDto['priority'] = 'normal'
		): OperationDto => ({
			id: `${kind}:${status}`,
			kind,
			title: status,
			status,
			detail: '다음 작업 대기',
			updatedAt: '2026-07-12T11:30:00Z',
			href: '/g/200000000000000001/work',
			priority
		});
		const items = mapOperations(
			[
				operation('ENTRY', '편성 필요', 'urgent'),
				operation('PICK_BAN', '픽밴/결과 입력', 'urgent'),
				operation('AUCTION', '팀장 설정'),
				operation('AUCTION', '입찰 진행'),
				operation('AUCTION', '대진 설정'),
				operation('AUCTION', '대진 진행'),
				operation('AUCTION', '결과 입력'),
				operation('AUCTION', '새 경매 단계')
			],
			now
		);

		expect(items.map((item) => item.actionLabel)).toEqual([
			'엔트리 편성',
			'픽밴 진행',
			'경매 준비',
			'입찰 진행',
			'대진 설정',
			'대진 확인',
			'결과 입력',
			'경매 진행'
		]);
		expect(items.map((item) => item.tone)).toEqual([
			'warning',
			'warning',
			'info',
			'info',
			'info',
			'info',
			'info',
			'info'
		]);
		expect(items[0].updatedAt).toBe('30분 전');
	});

	it('keeps the canonical direct match workspace href for actionable auction work', () => {
		const tournament: AuctionTournamentDto = {
			id: '01900000-0000-7000-8000-000000000601',
			displayNumber: 118,
			title: '20인 경매',
			format: 'TWENTY_PERSON',
			participantCount: 20,
			teamCount: 4,
			status: 'IN_GAME',
			revision: 7,
			activeMemberCount: 20,
			bidMemberCount: 16,
			incompleteMatchCount: 1,
			nextMatch: {
				id: '01900000-0000-7000-8000-000000000801',
				label: '결승',
				matchKey: 'FINAL',
				status: 'READY',
				confirmedGameCount: 1
			},
			championTeam: null,
			updatedAt: '2026-07-12T11:00:00Z',
			endedAt: null,
			href: '/g/200000000000000001/auction/matches/01900000-0000-7000-8000-000000000801'
		};

		const view = buildAuctionView({
			guildId: '200000000000000001',
			recruitments: [],
			activeTournaments: [tournament],
			completedTournaments: []
		});
		expect(view.activeWork[0]).toMatchObject({
			status: '결과 입력',
			detail: '결승 · 2세트 결과 대기',
			actionLabel: '경기 선택',
			href: '/g/200000000000000001/auction/tournaments/01900000-0000-7000-8000-000000000601/result'
		});
	});

	it('keeps cancelled tournaments as neutral archived history with their record route', () => {
		const tournament: AuctionTournamentDto = {
			id: '01900000-0000-7000-8000-000000000602',
			displayNumber: 117,
			title: '금요 10인 경매',
			format: 'TEN_PERSON',
			participantCount: 10,
			teamCount: 2,
			status: 'CANCELLED',
			revision: 9,
			activeMemberCount: 10,
			bidMemberCount: 8,
			incompleteMatchCount: 0,
			nextMatch: null,
			championTeam: null,
			updatedAt: '2026-07-12T10:00:00Z',
			endedAt: '2026-07-12T10:00:00Z',
			href: '/g/200000000000000001/auction/tournaments/01900000-0000-7000-8000-000000000602/result'
		};

		const view = buildAuctionView({
			guildId: '200000000000000001',
			recruitments: [],
			activeTournaments: [],
			completedTournaments: [tournament]
		});

		expect(view.completedWork[0]).toMatchObject({
			kind: '보관',
			status: '취소됨',
			actionLabel: '기록 보기',
			tone: 'neutral',
			iconKind: 'archived',
			href: tournament.href
		});
	});

	it('keeps open recruitments in the entry recruitment stage', () => {
		const recruitment: RecruitmentDto = {
			id: '01900000-0000-7000-8000-000000000101',
			displayNumber: 214,
			title: '금요 일반 내전 모집',
			status: 'OPEN',
			targetCount: 10,
			bestOf: 3,
			hardFearless: false,
			joinedCount: 3,
			assignedCount: 0,
			revision: 2,
			updatedAt: '2026-07-12T11:00:00Z',
			href: '/g/200000000000000001/recruitments/01900000-0000-7000-8000-000000000101'
		};

		const view = buildSeriesView({
			recruitments: [recruitment],
			activeSeries: [],
			completedSeries: []
		});

		expect(view.activeWork[0]).toMatchObject({
			format: 'BO3 · 일반',
			status: '엔트리 모집',
			detail: '3/10명 · Discord 모집 중',
			actionLabel: '모집 관리',
			href: recruitment.href
		});
	});
});
