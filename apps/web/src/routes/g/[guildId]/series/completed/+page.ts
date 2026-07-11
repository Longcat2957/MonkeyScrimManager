import { resolve } from '$app/paths';
import { redirect } from '@sveltejs/kit';
import type { PageLoad } from './$types';

export const load: PageLoad = ({ params }) => {
	const target = resolve('/g/[guildId]/series', { guildId: params.guildId });
	redirect(307, `${target}#completed`);
};
