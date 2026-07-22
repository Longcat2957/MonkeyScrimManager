import { describe, expect, it } from 'vitest';
import { formatMmr, formatRoundedNumber, formatSignedMmr } from './numberFormat';

describe('user-facing number formatting', () => {
	it('rounds floating-point MMR noise and adds grouping separators', () => {
		expect(formatMmr(1618.7499999999998)).toBe('1,619');
		expect(formatMmr(1499.0000000000002)).toBe('1,499');
	});

	it('formats signed MMR deltas without decimal tails or negative zero', () => {
		expect(formatSignedMmr(18.000000000000004)).toBe('+18');
		expect(formatSignedMmr(-17.999999999999996)).toBe('-18');
		expect(formatSignedMmr(-0.000000000000001)).toBe('0');
	});

	it('uses a safe fallback for invalid display values', () => {
		expect(formatRoundedNumber(Number.NaN)).toBe('-');
		expect(formatMmr(Number.POSITIVE_INFINITY, '미집계')).toBe('미집계');
	});
});
