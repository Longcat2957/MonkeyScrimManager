import { resolve } from '$app/paths';
import { redirect } from '@sveltejs/kit';
import { DEFAULT_GUILD_COOKIE, DEFAULT_GUILD_ID, isMockGuildId } from '$lib/fixtures/guilds';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = ({ cookies }) => {
	const savedGuildId = cookies.get(DEFAULT_GUILD_COOKIE);
	const guildId = savedGuildId && isMockGuildId(savedGuildId) ? savedGuildId : DEFAULT_GUILD_ID;

	redirect(307, resolve('/g/[guildId]', { guildId }));
};
