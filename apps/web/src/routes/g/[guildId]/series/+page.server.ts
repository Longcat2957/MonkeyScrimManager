import { recruitmentListDataSchema, seriesListDataSchema } from '$lib/contracts/workflow';
import { apiGet, apiRoute } from '$lib/server/api';
import { buildSeriesView } from '$lib/workflow/presentation';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params, parent, url }) => {
	await parent();

	const [recruitmentData, activeSeriesData, completedSeriesData] = await Promise.all([
		apiGet(
			fetch,
			apiRoute(['guilds', params.guildId, 'recruitments'], {
				state: 'actionable',
				limit: 100
			}),
			recruitmentListDataSchema,
			{ url }
		),
		apiGet(
			fetch,
			apiRoute(['guilds', params.guildId, 'series'], { state: 'active', limit: 100 }),
			seriesListDataSchema,
			{ url }
		),
		apiGet(
			fetch,
			apiRoute(['guilds', params.guildId, 'series'], { state: 'completed', limit: 100 }),
			seriesListDataSchema,
			{ url }
		)
	]);

	return {
		guildId: params.guildId,
		view: buildSeriesView({
			recruitments: recruitmentData.recruitments,
			activeSeries: activeSeriesData.series,
			completedSeries: completedSeriesData.series
		})
	};
};
