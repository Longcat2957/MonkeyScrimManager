import type { ResultChampion } from '$lib/fixtures/resultEntry';

export interface ResultActiveCell {
	kind: 'BAN' | 'PICK';
	key: string;
}

export function championById(
	championId: string | null,
	champions: ResultChampion[]
): ResultChampion | null {
	if (!championId) return null;
	return champions.find((champion) => champion.id === championId) ?? null;
}

export function normalizeChampionName(value: string): string {
	return value.trim().toLowerCase().replace(/\s+/g, '');
}

export function matchChampion(value: string, champions: ResultChampion[]): ResultChampion | null {
	const query = normalizeChampionName(value);
	if (!query) return null;

	return (
		champions.find(
			(champion) =>
				normalizeChampionName(champion.name) === query ||
				normalizeChampionName(champion.id) === query
		) ??
		champions.find((champion) => normalizeChampionName(champion.name).startsWith(query)) ??
		champions.find((champion) => normalizeChampionName(champion.id).startsWith(query)) ??
		champions.find(
			(champion) =>
				normalizeChampionName(champion.name).includes(query) ||
				normalizeChampionName(champion.id).includes(query)
		) ??
		null
	);
}

function championSearchScore(champion: ResultChampion, query: string): number {
	const name = normalizeChampionName(champion.name);
	const id = normalizeChampionName(champion.id);

	if (name === query || id === query) return 0;
	if (name.startsWith(query) || id.startsWith(query)) return 1;
	return 2;
}

export function suggestChampions(
	value: string,
	champions: ResultChampion[],
	limit = 5
): ResultChampion[] {
	const query = normalizeChampionName(value);
	if (!query) return [];

	return champions
		.filter((champion) => {
			const name = normalizeChampionName(champion.name);
			const id = normalizeChampionName(champion.id);
			return name.includes(query) || id.includes(query);
		})
		.sort(
			(a, b) =>
				championSearchScore(a, query) - championSearchScore(b, query) ||
				a.name.localeCompare(b.name, 'ko')
		)
		.slice(0, limit);
}
