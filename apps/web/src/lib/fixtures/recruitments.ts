export interface MockRecruitmentQueueItem {
	id: number;
	targetCount: number;
	status: 'CLOSED';
	createdBy: string;
	createdAt: number;
}

export const mockRecruitmentQueue: MockRecruitmentQueueItem[] = [
	{
		id: 214,
		targetCount: 10,
		status: 'CLOSED',
		createdBy: '홍대 미드',
		createdAt: 1752243120
	},
	{
		id: 213,
		targetCount: 10,
		status: 'CLOSED',
		createdBy: '서초 탑솔',
		createdAt: 1752239640
	}
];
