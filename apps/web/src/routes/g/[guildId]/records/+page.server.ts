import { resolve } from '$app/paths';
import { redirect } from '@sveltejs/kit';
import { fetchGuildRecords, GuildRecordsApiError } from '$lib/api/guildRecords';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, parent, url }) => {
	await parent();

	const query = url.searchParams.get('q')?.trim() ?? '';
	const seasonId = url.searchParams.get('seasonId')?.trim() || null;
	if (!query) {
		return {
			query,
			seasonId,
			snapshot: null,
			loadError: null
		};
	}

	try {
		const snapshot = await fetchGuildRecords(fetch, params.guildId, {
			query,
			seasonId: seasonId ?? undefined,
			limit: 20
		});
		return { query, seasonId, snapshot, loadError: null };
	} catch (cause) {
		if (!(cause instanceof GuildRecordsApiError)) throw cause;
		if (cause.status === 401) {
			const next = `${url.pathname}${url.search}`;
			const login = resolve('/oauth/[...path]', { path: 'discord/start' });
			redirect(303, `${login}?${new URLSearchParams({ next })}`);
		}
		return {
			query,
			seasonId,
			snapshot: null,
			loadError: cause.message
		};
	}
};
