const UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[1-8][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export type RecruitmentButtonAction = 'join' | 'leave' | 'start-entry';

export interface RecruitmentComponentId {
	action: RecruitmentButtonAction | 'roles';
	recruitmentId: string;
}

export function recruitmentCustomId(
	action: RecruitmentComponentId['action'],
	recruitmentId: string
): string {
	if (!UUID_PATTERN.test(recruitmentId)) {
		throw new Error('recruitment custom ID requires a UUID');
	}
	return `recruit:${action}:${recruitmentId}`;
}

export function parseRecruitmentCustomId(customId: string): RecruitmentComponentId | null {
	const [prefix, action, recruitmentId, extra] = customId.split(':');
	if (prefix !== 'recruit' || extra !== undefined || !recruitmentId) return null;
	if (!['join', 'leave', 'roles', 'start-entry'].includes(action ?? '')) return null;
	if (!UUID_PATTERN.test(recruitmentId)) return null;

	return {
		action: action as RecruitmentComponentId['action'],
		recruitmentId
	};
}
