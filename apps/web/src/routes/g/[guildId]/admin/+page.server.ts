import { resolve } from '$app/paths';
import { redirect } from '@sveltejs/kit';
import {
	guildAdminMembersDataSchema,
	guildAdminSettingsDataSchema,
	guildAuditLogsDataSchema
} from '$lib/contracts/workflow';
import { apiGet, apiRoute } from '$lib/server/api';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, url }) => {
	if (url.searchParams.get('tab') === 'operations') {
		const adminPath = resolve('/g/[guildId]/admin', { guildId: params.guildId });
		redirect(307, `${adminPath}?tab=members`);
	}

	const requestedTab = url.searchParams.get('tab');
	const memberSnapshot =
		requestedTab === 'settings' || requestedTab === 'logs'
			? null
			: await apiGet(
					fetch,
					apiRoute(['guilds', params.guildId, 'members']),
					guildAdminMembersDataSchema,
					{ url }
				);
	const settingsSnapshot =
		requestedTab === 'settings'
			? await apiGet(
					fetch,
					apiRoute(['guilds', params.guildId, 'settings']),
					guildAdminSettingsDataSchema,
					{ url }
				)
			: null;
	const auditSnapshot =
		requestedTab === 'logs'
			? await apiGet(
					fetch,
					`${apiRoute(['guilds', params.guildId, 'audit-logs'])}?limit=50`,
					guildAuditLogsDataSchema,
					{ url }
				)
			: null;

	return { guildId: params.guildId, memberSnapshot, settingsSnapshot, auditSnapshot };
};
