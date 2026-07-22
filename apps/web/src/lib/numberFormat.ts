const roundedNumberFormatter = new Intl.NumberFormat('ko-KR', {
	minimumFractionDigits: 0,
	maximumFractionDigits: 0,
	useGrouping: true
});

function roundedFiniteNumber(value: number): number | null {
	if (!Number.isFinite(value)) return null;
	const rounded = Math.round(value);
	return Object.is(rounded, -0) ? 0 : rounded;
}

export function formatRoundedNumber(value: number, fallback = '-'): string {
	const rounded = roundedFiniteNumber(value);
	return rounded === null ? fallback : roundedNumberFormatter.format(rounded);
}

export function formatMmr(value: number, fallback = '-'): string {
	return formatRoundedNumber(value, fallback);
}

export function formatSignedMmr(value: number, fallback = '-'): string {
	const rounded = roundedFiniteNumber(value);
	if (rounded === null) return fallback;
	const rendered = roundedNumberFormatter.format(rounded);
	return rounded > 0 ? `+${rendered}` : rendered;
}
