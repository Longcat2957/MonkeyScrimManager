import {
	dummyRecruitmentApplyResultSchema,
	dummyRecruitmentOverviewSchema,
	dummyRecruitmentPreviewSchema,
	type DummyRecruitmentAction
} from '$lib/contracts/dummyRecruitment';
import { apiRoute } from '$lib/routing';
import { requestDevData } from './devTools';

function participantsPath(guildId: string, recruitmentId: string): string {
	return apiRoute(['dev', 'guilds', guildId, 'recruitments', recruitmentId, 'participants']);
}

export function fetchDummyRecruitmentParticipants(guildId: string, recruitmentId: string) {
	return requestDevData(participantsPath(guildId, recruitmentId), dummyRecruitmentOverviewSchema);
}

export function previewDummyRecruitmentParticipants(
	guildId: string,
	recruitmentId: string,
	action: DummyRecruitmentAction,
	userIds: string[]
) {
	return requestDevData(
		`${participantsPath(guildId, recruitmentId)}/preview`,
		dummyRecruitmentPreviewSchema,
		{
			method: 'POST',
			body: JSON.stringify({ action, userIds })
		}
	);
}

export function applyDummyRecruitmentParticipants(
	guildId: string,
	recruitmentId: string,
	action: DummyRecruitmentAction,
	userIds: string[],
	expectedRevision: number,
	idempotencyKey: string
) {
	return requestDevData(
		participantsPath(guildId, recruitmentId),
		dummyRecruitmentApplyResultSchema,
		{
			method: action === 'ADD' ? 'POST' : 'DELETE',
			headers: { 'Idempotency-Key': idempotencyKey },
			body: JSON.stringify({ userIds, expectedRevision })
		}
	);
}
