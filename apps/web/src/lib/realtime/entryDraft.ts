import type { EntryDraftSnapshot } from '$lib/contracts/entryDraft';
import type { RealtimeInvalidateEvent } from './browserSocket';

export interface EntryDraftRealtimeTopics {
	recruitment: string;
	draft: string;
	all: readonly [string, string];
}

export function entryDraftRealtimeTopics(
	guildId: string,
	recruitmentId: string
): EntryDraftRealtimeTopics {
	const recruitment = `guild:${guildId}:recruitment:${recruitmentId}`;
	const draft = `guild:${guildId}:recruitment-entry-draft:${recruitmentId}`;
	return { recruitment, draft, all: [recruitment, draft] };
}

export function isNewerEntryDraftInvalidation(
	event: RealtimeInvalidateEvent,
	snapshot: EntryDraftSnapshot,
	topics: EntryDraftRealtimeTopics
): boolean {
	if (event.topic === topics.recruitment) {
		return event.revision > snapshot.recruitment.revision;
	}
	if (event.topic === topics.draft) {
		return event.revision > snapshot.draft.revision;
	}
	return false;
}
