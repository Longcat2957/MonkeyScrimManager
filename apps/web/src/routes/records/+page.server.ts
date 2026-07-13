import { fetchRiotRecords, parseRiotRecordsQuery, RiotRecordsApiError } from '$lib/api/riotRecords';
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
	if (!(error instanceof RiotRecordsApiError)) {
		return 'Riot 전적을 불러오지 못했습니다.';
	}
	if (error.status === 400 || error.status === 422) {
		return 'Riot ID 입력이 올바르지 않습니다.';
	}
	if (error.status === 404) {
		return '요청한 Riot 계정을 찾을 수 없습니다.';
	}
	if (error.status === 429) {
		return 'Riot 요청이 제한되었습니다. 잠시 후 다시 시도해 주세요.';
	}
	if (
		error.status === 0 ||
		error.status === 502 ||
		error.status === 503 ||
		error.code === 'invalid_response'
	) {
		return 'Riot 전적 서비스를 일시적으로 사용할 수 없습니다.';
	}
	return 'Riot 전적을 불러오지 못했습니다.';
}
