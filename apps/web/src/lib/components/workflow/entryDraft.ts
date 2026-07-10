import {
	TEAMS,
	type EntrySlot,
	type Lane,
	type MockEntryParticipant,
	type Team
} from '$lib/fixtures/workflow';

export function entrySlot(team: Team, lane: Lane): EntrySlot {
	return `${team}_${lane}`;
}

export function entrySlots(lanes: Lane[]): EntrySlot[] {
	const slots: EntrySlot[] = [];
	for (const team of TEAMS) {
		for (const lane of lanes) slots.push(entrySlot(team, lane));
	}
	return slots;
}

export function participantForSlot(
	participants: MockEntryParticipant[],
	assignments: Record<string, EntrySlot>,
	slot: EntrySlot
): MockEntryParticipant | null {
	return participants.find((participant) => assignments[participant.id] === slot) ?? null;
}

export function moveEntryParticipant(
	assignments: Record<string, EntrySlot>,
	participants: MockEntryParticipant[],
	userId: string,
	slot: EntrySlot | null
): Record<string, EntrySlot> {
	const next = { ...assignments };
	if (!slot) {
		delete next[userId];
		return next;
	}

	const currentUserId = participantForSlot(participants, next, slot)?.id ?? null;
	const previousSlot = next[userId];

	if (currentUserId && currentUserId !== userId) {
		if (previousSlot) next[currentUserId] = previousSlot;
		else delete next[currentUserId];
	}

	next[userId] = slot;
	return next;
}

export function entryTeamAverage(
	team: Team,
	lanes: Lane[],
	participants: MockEntryParticipant[],
	assignments: Record<string, EntrySlot>
): number {
	const teamMembers = lanes
		.map((lane) => participantForSlot(participants, assignments, entrySlot(team, lane)))
		.filter((participant): participant is MockEntryParticipant => Boolean(participant));

	if (teamMembers.length === 0) return 0;
	return Math.round(
		teamMembers.reduce((sum, participant) => sum + participant.mmr, 0) / teamMembers.length
	);
}

export function entryParticipantDetail(participant: MockEntryParticipant): string {
	return [participant.recentRecord, `MMR ${participant.mmr}`, participant.riotTier]
		.filter(Boolean)
		.join(' · ');
}
