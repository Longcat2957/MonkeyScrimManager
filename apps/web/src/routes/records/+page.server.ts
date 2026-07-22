import {
	fetchRiotRecords,
	parseRiotRecordsQuery,
	riotRecordsErrorText
} from '$lib/api/riotRecords';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, url }) => {
	const query = (url.searchParams.get('q') ?? '').trim();
	if (!query) {
		return {
			query,
			lookup: null,
			records: null,
			loadError: null
		};
	}

	const lookup = parseRiotRecordsQuery(query);
	if (!lookup) {
		return {
			query,
			lookup: null,
			records: null,
			loadError: 'Riot ID를 GameName#TagLine 형식으로 입력해 주세요.'
		};
	}

	try {
		return {
			query,
			lookup,
			records: await fetchRiotRecords(fetch, lookup),
			loadError: null
		};
	} catch (error) {
		return {
			query,
			lookup,
			records: null,
			loadError: recordsLoadError(error)
		};
	}
};

function recordsLoadError(error: unknown): string {
	return riotRecordsErrorText(error);
}
