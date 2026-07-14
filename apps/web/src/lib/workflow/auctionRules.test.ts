import { describe, expect, it } from 'vitest';
import { formatAuctionRules } from './auctionRules';

describe('auction recruitment rules', () => {
	it('formats a 10-person final series without a semifinal label', () => {
		expect(
			formatAuctionRules('TEN_PERSON', {
				hardFearless: false,
				semifinalBestOf: null,
				finalBestOf: 3
			})
		).toBe('BO3 · 일반');
	});

	it('formats 20-person semifinal and final rules independently', () => {
		expect(
			formatAuctionRules('TWENTY_PERSON', {
				hardFearless: true,
				semifinalBestOf: 1,
				finalBestOf: 5
			})
		).toBe('4강 BO1 · 결승 BO5 · 하드 피어리스');
	});
});
