import {
	apiErrorEnvelopeSchema,
	guildLeaderboardDataSchema,
	responseMetaSchema,
	type GuildLeaderboardSnapshot
} from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const guildLeaderboardEnvelopeSchema = z
	.object({
		data: guildLeaderboardDataSchema,
		meta: responseMetaSchema
	})
	.strict();

export class GuildLeaderboardApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string
	) {
		super(message);
		this.name = 'GuildLeaderboardApiError';
	}
}

export async function fetchGuildLeaderboard(
	fetcher: typeof globalThis.fetch,
	guildId: string
): Promise<GuildLeaderboardSnapshot> {
	let response: Response;
	try {
		response = await fetcher(apiRoute(['guilds', guildId, 'leaderboard']), {
			method: 'GET',
			credentials: 'same-origin',
			headers: { accept: 'application/json' }
		});
	} catch {
		throw new GuildLeaderboardApiError('리더보드 서버에 연결할 수 없습니다.', 0, 'network_error');
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) throw responseError(response.status, payload);

	const parsed = guildLeaderboardEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new GuildLeaderboardApiError(
			'리더보드 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response'
		);
	}
	return parsed.data.data;
}

function responseError(status: number, payload: unknown): GuildLeaderboardApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new GuildLeaderboardApiError(parsed.data.error.message, status, parsed.data.error.code);
	}
	return new GuildLeaderboardApiError(fallbackErrorMessage(status), status, `http_${status}`);
}

function fallbackErrorMessage(status: number): string {
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '리더보드를 확인할 권한이 없습니다.';
	if (status === 404) return '요청한 길드 리더보드를 찾을 수 없습니다.';
	if (status === 503) return '리더보드 서비스를 일시적으로 사용할 수 없습니다.';
	return '리더보드 요청을 처리하지 못했습니다.';
}
