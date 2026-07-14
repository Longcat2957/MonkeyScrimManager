import type { RecruitmentSnapshot } from '$lib/contracts/recruitmentSnapshot';
import type { RealtimeInvalidateEvent } from './browserSocket';

export function recruitmentRealtimeTopic(guildId: string, recruitmentId: string): string {
	return `guild:${guildId}:recruitment:${recruitmentId}`;
}

export function isNewerRecruitmentInvalidation(
	event: RealtimeInvalidateEvent,
	snapshot: RecruitmentSnapshot,
	topic: string
): boolean {
	return event.topic === topic && event.revision > snapshot.recruitment.revision;
}
