export type OperationKind = 'ENTRY' | 'PICK_BAN' | 'RESULT' | 'AUCTION';

export interface OperationsQueueItem {
	id: string;
	kind: OperationKind;
	title: string;
	status: string;
	detail: string;
	updatedAt: string;
	href: string;
	priority: 'urgent' | 'normal';
}

export const mockOperationsQueue: OperationsQueueItem[] = [
	{
		id: 'entry-r214',
		kind: 'ENTRY',
		title: '모집 #214',
		status: '편성 필요',
		detail: '10명 모집 마감 · 팀/라인 미배정',
		updatedAt: '3분 전',
		href: '/recruitments/214/entry',
		priority: 'urgent'
	},
	{
		id: 'pickban-s104',
		kind: 'PICK_BAN',
		title: '금요 정기 내전',
		status: '픽밴 입력',
		detail: '1세트 완료 · 2세트 픽밴 미입력',
		updatedAt: '8분 전',
		href: '/series/s-104/pick-ban',
		priority: 'urgent'
	},
	{
		id: 'result-s103',
		kind: 'RESULT',
		title: '주말 자유 내전',
		status: '결과 입력',
		detail: 'BO3 종료 · 최종 결과 확인 필요',
		updatedAt: '26분 전',
		href: '/series/s-103/pick-ban',
		priority: 'normal'
	},
	{
		id: 'auction-a118',
		kind: 'AUCTION',
		title: 'A-118 경매',
		status: '입찰 진행',
		detail: '4팀 · 다음 선수 입찰 대기',
		updatedAt: '방금',
		href: '/auction/tournaments/a-118/draft',
		priority: 'normal'
	}
];
