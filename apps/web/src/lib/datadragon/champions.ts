import type {
	ChampionCatalog,
	ChampionCatalogEntry,
	DataDragonChampionDto,
	DataDragonChampionResponse
} from './types';

export const DEFAULT_DATA_DRAGON_LOCALE = 'ko_KR';
export const DATA_DRAGON_PROXY_ROOT = '/dd';
export const STATIC_CHAMPION_FIXTURE_URL = '/fixtures/datadragon/champion-ko_KR.fixture.json';

const slugTokenPattern = /[a-z0-9]+/g;

export function normalizeChampionSlug(value: string): string {
	return (value.toLowerCase().match(slugTokenPattern) ?? []).join('');
}

export function dataDragonLocaleToIntlLocale(locale: string): string {
	return locale.replace('_', '-');
}

function searchToken(value: string): string {
	return value.toLowerCase().replace(/\s+/g, '');
}

export function championVersionsUrl(proxyRoot = DATA_DRAGON_PROXY_ROOT): string {
	return `${proxyRoot}/api/versions.json`;
}

export function championCatalogUrl(
	version: string,
	locale = DEFAULT_DATA_DRAGON_LOCALE,
	proxyRoot = DATA_DRAGON_PROXY_ROOT
): string {
	return `${proxyRoot}/cdn/${encodeURIComponent(version)}/data/${encodeURIComponent(locale)}/champion.json`;
}

export function championIconUrl(
	champion: Pick<ChampionCatalogEntry, 'imageFull'> | Pick<DataDragonChampionDto, 'image'> | string,
	version: string,
	proxyRoot = DATA_DRAGON_PROXY_ROOT
): string {
	const imageFull =
		typeof champion === 'string'
			? champion
			: 'imageFull' in champion
				? champion.imageFull
				: champion.image.full;

	return `${proxyRoot}/cdn/${encodeURIComponent(version)}/img/champion/${encodeURIComponent(imageFull)}`;
}

export function championLoadingUrl(
	champion: Pick<ChampionCatalogEntry, 'id'> | string,
	version: string,
	skin = 0,
	proxyRoot = DATA_DRAGON_PROXY_ROOT
): string {
	const id = typeof champion === 'string' ? champion : champion.id;
	return `${proxyRoot}/cdn/img/champion/loading/${encodeURIComponent(id)}_${skin}.jpg?v=${encodeURIComponent(version)}`;
}

export async function loadLatestDataDragonVersion(
	fetcher: typeof fetch,
	proxyRoot = DATA_DRAGON_PROXY_ROOT
): Promise<string> {
	const response = await fetcher(championVersionsUrl(proxyRoot));

	if (!response.ok) {
		throw new Error(`Data Dragon versions lookup failed: ${response.status}`);
	}

	const versions = (await response.json()) as unknown;

	if (!Array.isArray(versions) || typeof versions[0] !== 'string') {
		throw new Error('Data Dragon versions response is invalid');
	}

	return versions[0];
}

export async function loadChampionCatalog(
	fetcher: typeof fetch,
	options: {
		version?: string;
		locale?: string;
		proxyRoot?: string;
		fixtureUrl?: string;
	} = {}
): Promise<ChampionCatalog> {
	const locale = options.locale ?? DEFAULT_DATA_DRAGON_LOCALE;
	const proxyRoot = options.proxyRoot ?? DATA_DRAGON_PROXY_ROOT;

	try {
		const version = options.version ?? (await loadLatestDataDragonVersion(fetcher, proxyRoot));
		return await fetchChampionCatalog(
			fetcher,
			championCatalogUrl(version, locale, proxyRoot),
			locale
		);
	} catch (error) {
		if (!options.fixtureUrl) {
			throw error;
		}

		return fetchChampionCatalog(fetcher, options.fixtureUrl, locale);
	}
}

export function createChampionCatalog(
	response: DataDragonChampionResponse,
	locale = DEFAULT_DATA_DRAGON_LOCALE
): ChampionCatalog {
	const intlLocale = dataDragonLocaleToIntlLocale(locale);
	const champions = Object.values(response.data)
		.map((champion) => toCatalogEntry(champion))
		.sort((a, b) => a.name.localeCompare(b.name, intlLocale));

	return {
		version: response.version,
		locale,
		champions,
		byId: Object.fromEntries(champions.map((champion) => [champion.id, champion])),
		byKey: Object.fromEntries(champions.map((champion) => [champion.key, champion])),
		byName: Object.fromEntries(champions.map((champion) => [champion.name, champion])),
		byIdSlug: Object.fromEntries(champions.map((champion) => [champion.idSlug, champion]))
	};
}

export function findChampion(
	catalog: ChampionCatalog,
	value: string | number
): ChampionCatalogEntry | undefined {
	const token = String(value);
	return (
		catalog.byId[token] ??
		catalog.byKey[token] ??
		catalog.byName[token] ??
		catalog.byIdSlug[normalizeChampionSlug(token)]
	);
}

export function searchChampions(
	catalog: ChampionCatalog,
	query: string,
	options: { limit?: number } = {}
): ChampionCatalogEntry[] {
	const normalizedQuery = searchToken(query);

	if (!normalizedQuery) {
		return catalog.champions.slice(0, options.limit ?? 12);
	}

	const matches = catalog.champions
		.map((champion) => ({
			champion,
			score: scoreChampionMatch(champion, normalizedQuery)
		}))
		.filter((match) => match.score > 0)
		.sort(
			(a, b) =>
				b.score - a.score ||
				a.champion.name.localeCompare(b.champion.name, dataDragonLocaleToIntlLocale(catalog.locale))
		)
		.map((match) => match.champion);

	return matches.slice(0, options.limit ?? 12);
}

async function fetchChampionCatalog(
	fetcher: typeof fetch,
	url: string,
	locale: string
): Promise<ChampionCatalog> {
	const response = await fetcher(url);

	if (!response.ok) {
		throw new Error(`Champion catalog lookup failed: ${response.status}`);
	}

	return createChampionCatalog((await response.json()) as DataDragonChampionResponse, locale);
}

function toCatalogEntry(champion: DataDragonChampionDto): ChampionCatalogEntry {
	const idSlug = normalizeChampionSlug(champion.id);
	const tags = champion.tags ?? [];
	const searchText = [champion.id, idSlug, champion.key, champion.name, champion.title, ...tags]
		.filter(Boolean)
		.map((value) => searchToken(String(value)))
		.join(' ');

	return {
		id: champion.id,
		key: champion.key,
		name: champion.name,
		idSlug,
		imageFull: champion.image.full,
		title: champion.title,
		tags,
		searchText
	};
}

function scoreChampionMatch(champion: ChampionCatalogEntry, query: string): number {
	if (searchToken(champion.name) === query || champion.idSlug === query || champion.key === query) {
		return 100;
	}

	if (searchToken(champion.name).startsWith(query) || champion.idSlug.startsWith(query)) {
		return 75;
	}

	if (champion.searchText.includes(query)) {
		return 40;
	}

	return 0;
}
