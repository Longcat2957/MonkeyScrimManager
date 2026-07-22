import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

export type SearchSuggestionMode = 'riot' | 'custom';

const searchSuggestionSchema = z
	.object({
		kind: z.enum(['riotAccount', 'guildMember']),
		value: z.string().min(1),
		label: z.string().min(1),
		description: z.string().min(1).nullable(),
		profileIconId: z.number().int().nonnegative().nullable()
	})
	.strict();

const searchSuggestionsEnvelopeSchema = z
	.object({
		data: z
			.object({
				suggestions: z.array(searchSuggestionSchema).max(8)
			})
			.strict(),
		meta: responseMetaSchema
	})
	.strict();

export type SearchSuggestion = z.infer<typeof searchSuggestionSchema>;

export class SearchSuggestionsApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'SearchSuggestionsApiError';
	}
}

interface SearchSuggestionRequest {
	mode: SearchSuggestionMode;
	query: string;
	guildId: string;
	signal?: AbortSignal;
}

export async function fetchSearchSuggestions(
	fetcher: typeof globalThis.fetch,
	request: SearchSuggestionRequest
): Promise<SearchSuggestion[]> {
	const query = request.query.trim();
	if (query.length < 2 || (request.mode === 'custom' && !request.guildId)) return [];

	const path =
		request.mode === 'riot'
			? apiRoute(['riot', 'records', 'suggestions'], { q: query, platform: 'KR', limit: 8 })
			: apiRoute(['guilds', request.guildId, 'records', 'suggestions'], { q: query, limit: 8 });
	let response: Response;
	try {
		response = await fetcher(path, {
			method: 'GET',
			credentials: 'same-origin',
			headers: { accept: 'application/json' },
			signal: request.signal
		});
	} catch (cause) {
		if (request.signal?.aborted) throw cause;
		throw new SearchSuggestionsApiError(
			'검색 제안 서버에 연결할 수 없습니다.',
			0,
			'network_error',
			null
		);
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new SearchSuggestionsApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code,
				parsedError.data.error.requestId
			);
		}
		throw new SearchSuggestionsApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`,
			null
		);
	}

	const parsed = searchSuggestionsEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new SearchSuggestionsApiError(
			'검색 제안 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data.suggestions;
}

export function searchSuggestionsErrorText(cause: unknown): string {
	if (!(cause instanceof SearchSuggestionsApiError)) {
		return '검색 제안을 불러오지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return '검색어가 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '검색할 권한이 없습니다.';
	if (status === 429) return '검색 요청이 많습니다. 잠시 후 다시 시도해 주세요.';
	if (status === 503) return '검색 서비스를 일시적으로 사용할 수 없습니다.';
	return '검색 제안을 불러오지 못했습니다.';
}
