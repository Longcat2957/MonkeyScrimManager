import { describe, expect, it, vi } from 'vitest';
import { fetchSearchSuggestions } from './searchSuggestions';

const requestId = '01900000-0000-7000-8000-000000000001';

function success(suggestions: unknown[]): Response {
	return Response.json({ data: { suggestions }, meta: { requestId } });
}

describe('navbar search suggestions API client', () => {
	it('loads privacy-safe Riot account suggestions', async () => {
		const fetcher = vi.fn(async () =>
			success([
				{
					kind: 'riotAccount',
					value: 'Longcat#KR1',
					label: 'Longcat#KR1',
					description: 'Riot 계정',
					profileIconId: 29
				}
			])
		);

		await expect(
			fetchSearchSuggestions(fetcher, { mode: 'riot', query: 'Long', guildId: '' })
		).resolves.toHaveLength(1);
		expect(fetcher).toHaveBeenCalledWith(
			'/api/riot/records/suggestions?q=Long&platform=KR&limit=8',
			expect.objectContaining({ method: 'GET', credentials: 'same-origin' })
		);
	});

	it('scopes custom suggestions to the active guild', async () => {
		const fetcher = vi.fn(async () => success([]));

		await fetchSearchSuggestions(fetcher, {
			mode: 'custom',
			query: '탐솔',
			guildId: '123456789'
		});

		expect(fetcher).toHaveBeenCalledWith(
			'/api/guilds/123456789/records/suggestions?q=%ED%83%90%EC%86%94&limit=8',
			expect.any(Object)
		);
	});

	it('does not request incomplete queries and rejects malformed responses', async () => {
		const fetcher = vi.fn(async () => success([]));
		await expect(
			fetchSearchSuggestions(fetcher, { mode: 'riot', query: 'L', guildId: '' })
		).resolves.toEqual([]);
		expect(fetcher).not.toHaveBeenCalled();

		const malformed = vi.fn(async () => success([{ kind: 'riotAccount', value: 'Longcat#KR1' }]));
		await expect(
			fetchSearchSuggestions(malformed, { mode: 'riot', query: 'Long', guildId: '' })
		).rejects.toThrow('invalid_search_suggestions');
	});
});
