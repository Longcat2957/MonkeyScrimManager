export interface DataDragonImage {
	full: string;
	sprite?: string;
	group?: string;
	x?: number;
	y?: number;
	w?: number;
	h?: number;
}

export interface DataDragonChampionDto {
	version: string;
	id: string;
	key: string;
	name: string;
	title?: string;
	blurb?: string;
	image: DataDragonImage;
	tags?: string[];
	partype?: string;
}

export interface DataDragonChampionResponse {
	type: string;
	format: string;
	version: string;
	data: Record<string, DataDragonChampionDto>;
}

export interface ChampionCatalogEntry {
	id: string;
	key: string;
	name: string;
	idSlug: string;
	imageFull: string;
	title?: string;
	tags: string[];
	searchText: string;
}

export interface ChampionCatalog {
	version: string;
	locale: string;
	champions: ChampionCatalogEntry[];
	byId: Record<string, ChampionCatalogEntry>;
	byKey: Record<string, ChampionCatalogEntry>;
	byName: Record<string, ChampionCatalogEntry>;
	byIdSlug: Record<string, ChampionCatalogEntry>;
}
