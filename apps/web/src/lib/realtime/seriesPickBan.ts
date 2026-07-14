import type { SeriesPickBanSnapshot } from '$lib/contracts/seriesPickBan';
import type { RealtimeInvalidateEvent } from './browserSocket';

export interface SeriesPickBanRealtimeTopics {
	series: string;
	all: readonly [string];
}

export function seriesPickBanRealtimeTopics(
	guildId: string,
	seriesId: string
): SeriesPickBanRealtimeTopics {
	const series = `guild:${guildId}:series:${seriesId}`;
	return { series, all: [series] };
}

export function isNewerSeriesPickBanInvalidation(
	event: RealtimeInvalidateEvent,
	snapshot: SeriesPickBanSnapshot,
	topics: SeriesPickBanRealtimeTopics
): boolean {
	return event.topic === topics.series && event.revision > snapshot.series.revision;
}
