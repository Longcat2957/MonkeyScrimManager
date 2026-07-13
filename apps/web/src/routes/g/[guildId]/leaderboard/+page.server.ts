import { resolve } from '$app/paths';
import { redirect } from '@sveltejs/kit';
import { fetchGuildLeaderboard, GuildLeaderboardApiError } from '$lib/api/guildLeaderboard';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, parent, url }) => {
	await parent();

	try {
		const snapshot = await fetchGuildLeaderboard(fetch, params.guildId);
		return { snapshot, loadError: null };
	} catch (cause) {
		if (!(cause instanceof GuildLeaderboardApiError)) throw cause;
		if (cause.status === 401) {
			const next = `${url.pathname}${url.search}`;
			const login = resolve('/oauth/[...path]', { path: 'discord/start' });
			redirect(303, `${login}?${new URLSearchParams({ next })}`);
		}
		return { snapshot: null, loadError: cause.message };
	}
};
