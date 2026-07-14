import type {
	ScrimPickBanResultInputChampion,
	ScrimPickBanResultInputSide,
	ScrimPickBanResultInputTeam
} from '$lib/fixtures/scrimPickBanResultInputFixtures';

export interface ScrimPickBanResultInputActiveCell {
	kind: 'BAN' | 'PICK';
	key: string;
}

const pickBanSides: ScrimPickBanResultInputSide[] = ['BLUE', 'RED'];

export function scrimPickBanResultInputBanKey(
	side: ScrimPickBanResultInputSide,
	index: number
): string {
	return `${side}-ban-${index}`;
}

export function scrimPickBanResultInputCellOrder(
	teams: Record<ScrimPickBanResultInputSide, ScrimPickBanResultInputTeam>,
	bans: Record<ScrimPickBanResultInputSide, Array<string | null>>
): ScrimPickBanResultInputActiveCell[] {
	return [
		...pickBanSides.flatMap((side) =>
			bans[side].map((_, index) => ({
				kind: 'BAN' as const,
				key: scrimPickBanResultInputBanKey(side, index)
			}))
		),
		...pickBanSides.flatMap((side) =>
			teams[side].players.map((player) => ({ kind: 'PICK' as const, key: player.id }))
		)
	];
}

export function adjacentScrimPickBanResultInputCell(
	cells: ScrimPickBanResultInputActiveCell[],
	current: ScrimPickBanResultInputActiveCell,
	direction: -1 | 1
): ScrimPickBanResultInputActiveCell | null {
	if (cells.length === 0) return null;
	const currentIndex = cells.findIndex(
		(cell) => cell.kind === current.kind && cell.key === current.key
	);
	if (currentIndex < 0) return cells[direction === 1 ? 0 : cells.length - 1] ?? null;
	return cells[(currentIndex + direction + cells.length) % cells.length] ?? null;
}

export function scrimPickBanResultInputCellId(cell: ScrimPickBanResultInputActiveCell): string {
	return `scrim-pickban-${cell.kind.toLowerCase()}-${cell.key}`;
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
