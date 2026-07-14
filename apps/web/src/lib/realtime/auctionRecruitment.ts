import type { AuctionRecruitmentSnapshot } from '$lib/contracts/auctionRecruitmentSnapshot';
import type { RealtimeInvalidateEvent } from './browserSocket';

export function auctionRecruitmentRealtimeTopic(guildId: string, recruitmentId: string): string {
	return `guild:${guildId}:auction-recruitment:${recruitmentId}`;
}

export function isNewerAuctionRecruitmentInvalidation(
	event: RealtimeInvalidateEvent,
	snapshot: AuctionRecruitmentSnapshot,
	topic: string
): boolean {
	return event.topic === topic && event.revision > snapshot.recruitment.revision;
}
