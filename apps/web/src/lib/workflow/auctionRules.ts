import type { AuctionRecruitmentDto, AuctionRecruitmentRules } from '$lib/contracts/workflow';

export function formatAuctionRules(
	format: AuctionRecruitmentDto['format'],
	rules: AuctionRecruitmentRules
): string {
	const fearlessLabel = rules.hardFearless ? '하드 피어리스' : '일반';
	if (format === 'TEN_PERSON') {
		return `BO${rules.finalBestOf} · ${fearlessLabel}`;
	}

	return `4강 BO${rules.semifinalBestOf ?? 3} · 결승 BO${rules.finalBestOf} · ${fearlessLabel}`;
}
