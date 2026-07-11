import type { Lane } from './workflow';

export type GuildMemberStatus = 'ACTIVE' | 'INACTIVE';

export interface GuildAdminMember {
	id: string;
	discordName: string;
	riotId: string | null;
	role: '운영자' | '참가자';
	mainLane: Lane;
	laneMmr: Record<Lane, number>;
	registered: boolean;
	status: GuildMemberStatus;
	lastActiveAt: string;
}

export interface GuildRevertCheckpoint {
	id: string;
	action: string;
	target: string;
	operator: string;
	createdAt: string;
	status: 'ACTIVE' | 'REVERTED';
}

export interface GuildStaleOperation {
	id: string;
	type: '모집' | '일반 내전' | '경매';
	title: string;
	age: string;
	status: string;
}

export interface GuildAuditEntry {
	id: string;
	action: string;
	target: string;
	operator: string;
	createdAt: string;
	category: '멤버' | 'MMR' | '내전' | '경매' | '설정';
	result: '완료' | '복구됨';
	detail: string;
}

export interface GuildAdminSettings {
	displayName: string;
	operatorRoleId: string;
	recruitmentChannelId: string;
	logChannelId: string;
	seasonId: string;
	isExternallyPublic: boolean;
}

const baseLaneMmr: Record<Lane, number> = {
	TOP: 1500,
	JUNGLE: 1500,
	MID: 1500,
	BOTTOM: 1500,
	SUPPORT: 1500
};

export const mockGuildAdminMembers: GuildAdminMember[] = [
	{
		id: 'user-tamsol',
		discordName: '탐솔',
		riotId: '탐솔#KR1',
		role: '운영자',
		mainLane: 'MID',
		laneMmr: { ...baseLaneMmr, MID: 1642, BOTTOM: 1588 },
		registered: true,
		status: 'ACTIVE',
		lastActiveAt: '방금'
	},
	{
		id: 'user-longcat',
		discordName: 'Longcat',
		riotId: 'Longcat#KR1',
		role: '운영자',
		mainLane: 'JUNGLE',
		laneMmr: { ...baseLaneMmr, JUNGLE: 1618, MID: 1564 },
		registered: true,
		status: 'ACTIVE',
		lastActiveAt: '18분 전'
	},
	{
		id: 'user-maroo',
		discordName: '마루',
		riotId: '마루는탑#KR1',
		role: '참가자',
		mainLane: 'TOP',
		laneMmr: { ...baseLaneMmr, TOP: 1596 },
		registered: true,
		status: 'ACTIVE',
		lastActiveAt: '어제'
	},
	{
		id: 'user-mocha',
		discordName: '모카',
		riotId: '모카원딜#KR2',
		role: '참가자',
		mainLane: 'BOTTOM',
		laneMmr: { ...baseLaneMmr, BOTTOM: 1552 },
		registered: true,
		status: 'ACTIVE',
		lastActiveAt: '2일 전'
	},
	{
		id: 'user-wave',
		discordName: '파도',
		riotId: null,
		role: '참가자',
		mainLane: 'SUPPORT',
		laneMmr: { ...baseLaneMmr, SUPPORT: 1534 },
		registered: true,
		status: 'ACTIVE',
		lastActiveAt: '3일 전'
	},
	{
		id: 'user-dormant',
		discordName: '휴면유저',
		riotId: null,
		role: '참가자',
		mainLane: 'TOP',
		laneMmr: { ...baseLaneMmr },
		registered: false,
		status: 'INACTIVE',
		lastActiveAt: '92일 전'
	}
];

export const mockGuildRevertCheckpoints: GuildRevertCheckpoint[] = [
	{
		id: 'checkpoint-214',
		action: '마지막 게임 결과 삭제',
		target: '금요 정기 내전 · 3세트',
		operator: '탐솔',
		createdAt: '12분 전',
		status: 'ACTIVE'
	},
	{
		id: 'checkpoint-213',
		action: '최근 경매 입찰 복구',
		target: 'A20-118 · Longcat 낙찰',
		operator: 'Longcat',
		createdAt: '27분 전',
		status: 'ACTIVE'
	},
	{
		id: 'checkpoint-209',
		action: '라인업 확정 취소',
		target: 'A10-115 · 결승',
		operator: '탐솔',
		createdAt: '1시간 전',
		status: 'REVERTED'
	}
];

export const mockGuildStaleOperations: GuildStaleOperation[] = [
	{
		id: 'recruitment-201',
		type: '모집',
		title: '평일 저녁 내전 모집',
		age: '4일 경과',
		status: '모집 중'
	},
	{
		id: 'series-188',
		type: '일반 내전',
		title: '주말 자유 내전',
		age: '7일 경과',
		status: '결과 대기'
	},
	{
		id: 'auction-109',
		type: '경매',
		title: 'A20-109 경매 내전',
		age: '11일 경과',
		status: '입찰 중'
	}
];

export const mockGuildAuditEntries: GuildAuditEntry[] = [
	{
		id: 'audit-1046',
		action: 'MMR 수동 조정',
		target: '마루 · TOP +18',
		operator: '탐솔',
		createdAt: '5분 전',
		category: 'MMR',
		result: '완료',
		detail: '이전 경기 누락분 반영 · 1578 → 1596'
	},
	{
		id: 'audit-1045',
		action: '게임 결과 입력',
		target: '금요 정기 내전 · 3세트',
		operator: '탐솔',
		createdAt: '12분 전',
		category: '내전',
		result: '완료',
		detail: '블루팀 승리 · MMR 원장 반영 완료'
	},
	{
		id: 'audit-1044',
		action: '경매 낙찰 확정',
		target: 'A20-118 · Longcat → 2팀',
		operator: 'Longcat',
		createdAt: '27분 전',
		category: '경매',
		result: '완료',
		detail: '낙찰가 220P · revision 17'
	},
	{
		id: 'audit-1043',
		action: '라인업 확정 복구',
		target: 'A10-115 · 결승',
		operator: '탐솔',
		createdAt: '1시간 전',
		category: '경매',
		result: '복구됨',
		detail: 'checkpoint-209 적용 · 결과 입력 전 상태'
	},
	{
		id: 'audit-1042',
		action: '멤버 비활성화',
		target: '휴면유저',
		operator: '탐솔',
		createdAt: '어제',
		category: '멤버',
		result: '완료',
		detail: '90일 이상 활동 없음 · soft delete 적용'
	},
	{
		id: 'audit-1041',
		action: '운영자 역할 변경',
		target: '내전 운영진',
		operator: '탐솔',
		createdAt: '3일 전',
		category: '설정',
		result: '완료',
		detail: 'Discord role 928401530으로 변경'
	}
];

export const mockGuildAdminSettings: GuildAdminSettings = {
	displayName: 'MSM Seoul',
	operatorRoleId: 'role-operator',
	recruitmentChannelId: 'channel-recruitment',
	logChannelId: 'channel-operations',
	seasonId: 'season-3',
	isExternallyPublic: true
};

export const mockDiscordRoles = [
	{ id: 'role-operator', name: '내전 운영진' },
	{ id: 'role-admin', name: '서버 관리자' },
	{ id: 'role-staff', name: 'MonkeyBot Staff' }
];

export const mockDiscordChannels = [
	{ id: 'channel-recruitment', name: '#내전-모집' },
	{ id: 'channel-operations', name: '#내전-운영-로그' },
	{ id: 'channel-general', name: '#일반' }
];

export const mockGuildSeasons = [
	{ id: 'season-3', name: '시즌 3', state: '진행 중' },
	{ id: 'season-2', name: '시즌 2', state: '종료' }
];
