import { resolve } from '$app/paths';
import { redirect } from '@sveltejs/kit';
import { meDataSchema } from '$lib/contracts/workflow';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, url }) => {
	const user = await apiGet(fetch, apiRoute(['me']), meDataSchema, {
		url,
		onApiError: (apiError) => {
			if (apiError.status === 401) redirect(303, resolve('/landing'));
		}
	});

	if (user.defaultGuildId) {
		redirect(307, resolve('/g/[guildId]', { guildId: user.defaultGuildId }));
	}
	redirect(307, `${resolve('/me')}?tab=guild`);
};
