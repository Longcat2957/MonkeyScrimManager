import type { ScrimPickBanResultInputChampion } from '$lib/fixtures/scrimPickBanResultInputFixtures';

export interface ScrimPickBanResultInputActiveCell {
	kind: 'BAN' | 'PICK';
	key: string;
}

export function findScrimPickBanResultInputChampionById(
	championId: string | null,
	champions: ScrimPickBanResultInputChampion[]
): ScrimPickBanResultInputChampion | null {
	if (!championId) return null;
	return champions.find((champion) => champion.id === championId) ?? null;
}

export function normalizeScrimPickBanResultInputChampionName(value: string): string {
	return value.trim().toLowerCase().replace(/\s+/g, '');
}

export function matchScrimPickBanResultInputChampion(
	value: string,
	champions: ScrimPickBanResultInputChampion[]
): ScrimPickBanResultInputChampion | null {
	const query = normalizeScrimPickBanResultInputChampionName(value);
	if (!query) return null;

	return (
		champions.find(
			(champion) =>
				normalizeScrimPickBanResultInputChampionName(champion.name) === query ||
				normalizeScrimPickBanResultInputChampionName(champion.id) === query
		) ??
		champions.find((champion) =>
			normalizeScrimPickBanResultInputChampionName(champion.name).startsWith(query)
		) ??
		champions.find((champion) =>
			normalizeScrimPickBanResultInputChampionName(champion.id).startsWith(query)
		) ??
		champions.find(
			(champion) =>
				normalizeScrimPickBanResultInputChampionName(champion.name).includes(query) ||
				normalizeScrimPickBanResultInputChampionName(champion.id).includes(query)
		) ??
		null
	);
}

function scrimPickBanResultInputChampionSearchScore(
	champion: ScrimPickBanResultInputChampion,
	query: string
): number {
	const name = normalizeScrimPickBanResultInputChampionName(champion.name);
	const id = normalizeScrimPickBanResultInputChampionName(champion.id);

	if (name === query || id === query) return 0;
	if (name.startsWith(query) || id.startsWith(query)) return 1;
	return 2;
}

export function suggestScrimPickBanResultInputChampions(
	value: string,
	champions: ScrimPickBanResultInputChampion[],
	limit = 5
): ScrimPickBanResultInputChampion[] {
	const query = normalizeScrimPickBanResultInputChampionName(value);
	if (!query) return [];

	return champions
		.filter((champion) => {
			const name = normalizeScrimPickBanResultInputChampionName(champion.name);
			const id = normalizeScrimPickBanResultInputChampionName(champion.id);
			return name.includes(query) || id.includes(query);
		})
		.sort(
			(a, b) =>
				scrimPickBanResultInputChampionSearchScore(a, query) -
					scrimPickBanResultInputChampionSearchScore(b, query) || a.name.localeCompare(b.name, 'ko')
		)
		.slice(0, limit);
}
