import type {
	AuctionRecruitmentDto,
	AuctionTournamentDto,
	OperationDto,
	RecruitmentDto,
	SeriesDto
} from '$lib/contracts/workflow';
import { formatSeriesRules } from './seriesRules';

export function formatRelativeTime(value: string, now = Date.now()): string {
	const timestamp = Date.parse(value);
	if (!Number.isFinite(timestamp)) return '-';
	const elapsedSeconds = Math.max(0, Math.floor((now - timestamp) / 1000));
	if (elapsedSeconds < 60) return '방금';
	if (elapsedSeconds < 3600) return `${Math.floor(elapsedSeconds / 60)}분 전`;
	if (elapsedSeconds < 86_400) return `${Math.floor(elapsedSeconds / 3600)}시간 전`;
	return `${Math.floor(elapsedSeconds / 86_400)}일 전`;
}

export function mapOperations(operations: OperationDto[], now = Date.now()): OperationDto[] {
	return operations.map((operation) => ({
		...operation,
		updatedAt: formatRelativeTime(operation.updatedAt, now)
	}));
}

export interface SeriesActiveWorkItem {
	key: string;
	kind: 'RECRUITMENT' | 'SERIES';
	displayNumber: number;
	title: string;
	format: string;
	status: string;
	detail: string;
	updatedAt: string;
	href: string;
	sortTimestamp: string;
}

export interface CompletedSeriesItem {
	key: string;
	displayNumber: number;
	name: string;
	format: string;
	score: string;
	winner: string;
	outcome: SeriesDto['outcome'];
	playedAt: string;
	href: string;
}

export function buildSeriesView(
	input: {
		recruitments: RecruitmentDto[];
		activeSeries: SeriesDto[];
		completedSeries: SeriesDto[];
	},
	now = Date.now()
) {
	const recruitmentRows: SeriesActiveWorkItem[] = input.recruitments.map((item) => ({
		key: `recruitment:${item.id}`,
		kind: 'RECRUITMENT',
		displayNumber: item.displayNumber,
		title: item.title,
		format: formatSeriesRules(item.bestOf, item.hardFearless),
		status:
			item.status === 'OPEN'
				? '엔트리 모집'
				: item.assignedCount >= item.targetCount
					? '시리즈 생성 대기'
					: '엔트리 편성',
		detail:
			item.status === 'OPEN'
				? `${item.joinedCount}/${item.targetCount}명 · Discord 모집 중`
				: `${item.joinedCount}/${item.targetCount}명 · ${Math.max(item.joinedCount - item.assignedCount, 0)}명 미배정`,
		updatedAt: formatRelativeTime(item.updatedAt, now),
		href: item.href,
		sortTimestamp: item.updatedAt
	}));
	const seriesRows: SeriesActiveWorkItem[] = input.activeSeries.map((item) => ({
		key: `series:${item.id}`,
		kind: 'SERIES',
		displayNumber: item.displayNumber,
		title: item.title,
		format: formatSeriesRules(item.bestOf, item.hardFearless),
		status: '픽밴/결과 입력',
		detail: item.currentSetNumber
			? `${item.currentSetNumber}세트 · 입력 대기`
			: '다음 세트 입력 대기',
		updatedAt: formatRelativeTime(item.updatedAt, now),
		href: item.href,
		sortTimestamp: item.updatedAt
	}));
	const activeWork = [...recruitmentRows, ...seriesRows]
		.sort(compareUpdatedRows)
		.map(({ sortTimestamp, ...item }) => {
			void sortTimestamp;
			return item;
		});
	const completedSeries: CompletedSeriesItem[] = input.completedSeries.map((item) => ({
		key: `series:${item.id}`,
		displayNumber: item.displayNumber,
		name: item.title,
		format: formatSeriesRules(item.bestOf, item.hardFearless),
		score: `${item.team1Score} : ${item.team2Score}`,
		winner:
			item.outcome === 'DRAW'
				? '무승부'
				: item.winningTeam === 'TEAM_1'
					? '1팀'
					: item.winningTeam === 'TEAM_2'
						? '2팀'
						: '결과 없음',
		outcome: item.outcome,
		playedAt: formatRelativeTime(item.endedAt ?? item.updatedAt, now),
		href: item.href
	}));
	return { activeWork, completedSeries };
}

export type AuctionIconKind = 'recruitment' | 'bidding' | 'bracket' | 'completed' | 'archived';
export interface AuctionWorkItem {
	id: string;
	displayNumber: number;
	title: string;
	kind: '모집' | '토너먼트' | '완료' | '보관';
	status: string;
	detail: string;
	href: string;
	iconKind: AuctionIconKind;
	tone: 'neutral' | 'warning' | 'info' | 'success';
	sortTimestamp: string;
}

export function buildAuctionView(input: {
	guildId: string;
	recruitments: AuctionRecruitmentDto[];
	activeTournaments: AuctionTournamentDto[];
	completedTournaments: AuctionTournamentDto[];
}) {
	const recruitmentRows: AuctionWorkItem[] = input.recruitments.map((item) => ({
		id: `recruitment:${item.id}`,
		displayNumber: item.displayNumber,
		title: item.title,
		kind: '모집',
		status: item.status === 'OPEN' ? '참가 모집' : '마감 명단',
		detail:
			item.status === 'OPEN'
				? `${item.joinedParticipantCount}/${item.participantCount}명 · Discord 모집 중`
				: `${item.joinedParticipantCount}/${item.participantCount}명 · ${item.teamCount}팀`,
		href: item.href,
		iconKind: 'recruitment',
		tone: 'warning',
		sortTimestamp: item.updatedAt
	}));
	const activeTournamentRows = input.activeTournaments.map((item) =>
		mapActiveAuctionTournament(item, input.guildId)
	);
	const activeWork = [...recruitmentRows, ...activeTournamentRows]
		.sort(compareUpdatedRows)
		.map(({ sortTimestamp, ...item }) => {
			void sortTimestamp;
			return item;
		});
	const completedWork = input.completedTournaments
		.map((item) =>
			item.status === 'CANCELLED'
				? mapArchivedAuctionTournament(item)
				: mapCompletedAuctionTournament(item)
		)
		.map(({ sortTimestamp, ...item }) => {
			void sortTimestamp;
			return item;
		});
	return { activeWork, completedWork };
}

function mapCompletedAuctionTournament(item: AuctionTournamentDto): AuctionWorkItem {
	return {
		id: `tournament:${item.id}`,
		displayNumber: item.displayNumber,
		title: item.title,
		kind: '완료',
		status: '결과 확정',
		detail: item.championTeam
			? `${item.participantCount}인 · 우승 ${item.championTeam.name}`
			: `${item.participantCount}인 · 우승팀 미정`,
		href: item.href,
		iconKind: 'completed',
		tone: 'success',
		sortTimestamp: item.updatedAt
	};
}

function mapArchivedAuctionTournament(item: AuctionTournamentDto): AuctionWorkItem {
	return {
		id: `tournament:${item.id}`,
		displayNumber: item.displayNumber,
		title: item.title,
		kind: '보관',
		status: '취소됨',
		detail: `${item.participantCount}인 · 운영 취소됨`,
		href: item.href,
		iconKind: 'archived',
		tone: 'neutral',
		sortTimestamp: item.updatedAt
	};
}

function mapActiveAuctionTournament(item: AuctionTournamentDto, guildId: string): AuctionWorkItem {
	if (item.status === 'BIDDING') {
		return {
			id: `tournament:${item.id}`,
			displayNumber: item.displayNumber,
			title: item.title,
			kind: '토너먼트',
			status: '입찰 진행',
			detail: `${item.participantCount}인 · ${item.teamCount}팀 · ${item.bidMemberCount}명 낙찰`,
			href: item.href,
			iconKind: 'bidding',
			tone: 'info',
			sortTimestamp: item.updatedAt
		};
	}
	if (item.status === 'BRACKET_SETUP') {
		return {
			id: `tournament:${item.id}`,
			displayNumber: item.displayNumber,
			title: item.title,
			kind: '토너먼트',
			status: '대진 설정',
			detail: `${item.participantCount}인 · ${item.teamCount}팀 · 대진 확정 대기`,
			href: item.href,
			iconKind: 'bracket',
			tone: 'warning',
			sortTimestamp: item.updatedAt
		};
	}
	const resultInput = item.nextMatch && ['READY', 'IN_PROGRESS'].includes(item.nextMatch.status);
	return {
		id: `tournament:${item.id}`,
		displayNumber: item.displayNumber,
		title: item.title,
		kind: '토너먼트',
		status: resultInput ? '결과 입력' : '대진 진행',
		detail: item.nextMatch
			? resultInput
				? `${item.nextMatch.label} · ${item.nextMatch.confirmedGameCount + 1}세트 결과 대기`
				: `${item.nextMatch.label} · 라인업 대기`
			: `${item.incompleteMatchCount}경기 진행 대기`,
		href: `/g/${guildId}/auction/tournaments/${item.id}/result`,
		iconKind: 'bracket',
		tone: 'warning',
		sortTimestamp: item.updatedAt
	};
}

function compareUpdatedRows(
	left: { sortTimestamp: string; key?: string; id?: string },
	right: { sortTimestamp: string; key?: string; id?: string }
): number {
	const byTime = Date.parse(right.sortTimestamp) - Date.parse(left.sortTimestamp);
	if (byTime !== 0) return byTime;
	return (right.key ?? right.id ?? '').localeCompare(left.key ?? left.id ?? '');
}
