export const seriesBestOfValues = [1, 3, 5] as const;

export type SeriesBestOf = (typeof seriesBestOfValues)[number];

export function formatSeriesRules(bestOf: SeriesBestOf, hardFearless: boolean): string {
	return `BO${bestOf} · ${hardFearless ? '하드 피어리스' : '일반'}`;
}
