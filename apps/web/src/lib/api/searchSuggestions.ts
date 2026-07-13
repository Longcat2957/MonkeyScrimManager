import { responseMetaSchema } from '$lib/contracts/workflow';
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
	const response = await fetcher(path, {
		method: 'GET',
		credentials: 'same-origin',
		headers: { accept: 'application/json' },
		signal: request.signal
	});
	if (!response.ok) throw new Error('search_suggestions_unavailable');

	const payload = await response.json().catch(() => null);
	const parsed = searchSuggestionsEnvelopeSchema.safeParse(payload);
	if (!parsed.success) throw new Error('invalid_search_suggestions');
	return parsed.data.data.suggestions;
}
