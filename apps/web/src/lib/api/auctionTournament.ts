import {
	auctionSetupDraftSnapshotSchema,
	auctionSetupConfiguredTeamSchema,
	auctionTournamentNavigationSchema,
	type AuctionSetupConfiguredTeam,
	type AuctionSetupDraftSnapshot,
	type AuctionTournamentNavigation
} from '$lib/contracts/auctionTournamentSetup';
import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const idempotencyKeySchema = z.uuid();
const revisionSchema = z.number().int().nonnegative();
const auctionSetupDraftEnvelopeSchema = z
	.object({ data: auctionSetupDraftSnapshotSchema, meta: responseMetaSchema })
	.strict();
const auctionTournamentCreateEnvelopeSchema = z
	.object({
		data: z.object({ tournament: auctionTournamentNavigationSchema }).passthrough(),
		meta: responseMetaSchema
	})
	.strict();

type AuctionTournamentAction = 'load_setup' | 'save_setup' | 'create_tournament';

export type AuctionSetupDraftSaveCommand = {
	expectedRevision: number;
	teams: AuctionSetupConfiguredTeam[];
	idempotencyKey: string;
};

export type AuctionTournamentCreateCommand = {
	sourceRecruitmentId: string;
	expectedRevision: number;
	idempotencyKey: string;
};

export class AuctionTournamentApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'AuctionTournamentApiError';
	}
}

function readCsrfToken(): string | null {
	const prefix = 'monkeybot_csrf=';
	const value = (globalThis.document?.cookie ?? '')
		.split(';')
		.map((cookie) => cookie.trim())
		.find((cookie) => cookie.startsWith(prefix));
	return value ? decodeURIComponent(value.slice(prefix.length)) : null;
}

function mutationHeaders(idempotencyKey: string): Record<string, string> {
	const token = readCsrfToken();
	if (!token) {
		throw new AuctionTournamentApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing',
			null
		);
	}
	if (!idempotencyKeySchema.safeParse(idempotencyKey).success) {
		throw new AuctionTournamentApiError(
			'요청 식별자가 올바르지 않습니다.',
			0,
			'invalid_idempotency_key',
			null
		);
	}
	return {
		accept: 'application/json',
		'content-type': 'application/json',
		'X-CSRF-Token': token,
		'Idempotency-Key': idempotencyKey
	};
}

function actionLabel(action: AuctionTournamentAction): string {
	if (action === 'load_setup') return '경매 준비 설정 조회';
	if (action === 'save_setup') return '경매 준비 설정 저장';
	return '경매 입찰 시작';
}

function fallbackErrorMessage(status: number, action: AuctionTournamentAction): string {
	const label = actionLabel(action);
	if (status === 400 || status === 422) return `${label} 요청 값이 올바르지 않습니다.`;
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return `${label}에는 길드 운영자 권한이 필요합니다.`;
	if (status === 404) return '경매 모집 또는 준비 설정을 찾을 수 없습니다.';
	if (status === 409) return '다른 운영자가 먼저 변경했습니다. 최신 설정을 확인해 주세요.';
	if (status === 503) return '경매 준비 서비스를 일시적으로 사용할 수 없습니다.';
	return `${label} 요청을 처리하지 못했습니다.`;
}

async function responsePayload(response: Response): Promise<unknown> {
	return await response.json().catch(() => null);
}

function responseError(
	response: Response,
	payload: unknown,
	action: AuctionTournamentAction
): AuctionTournamentApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new AuctionTournamentApiError(
			parsed.data.error.message,
			response.status,
			parsed.data.error.code,
			parsed.data.error.requestId
		);
	}
	return new AuctionTournamentApiError(
		fallbackErrorMessage(response.status, action),
		response.status,
		`http_${response.status}`,
		null
	);
}

async function requestAuctionSetupDraft(
	path: string,
	init: RequestInit,
	action: AuctionTournamentAction
): Promise<AuctionSetupDraftSnapshot> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, { ...init, credentials: 'same-origin' });
	} catch (cause) {
		if (cause instanceof AuctionTournamentApiError) throw cause;
		throw new AuctionTournamentApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error', null);
	}

	const payload = await responsePayload(response);
	if (!response.ok) throw responseError(response, payload, action);

	const parsed = auctionSetupDraftEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new AuctionTournamentApiError(
			`${actionLabel(action)} 응답 형식이 올바르지 않습니다.`,
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

function validateSetupTeams(teams: AuctionSetupConfiguredTeam[]): void {
	if (
		teams.length < 2 ||
		teams.length > 4 ||
		!teams.every((team) => auctionSetupConfiguredTeamSchema.safeParse(team).success)
	) {
		throw new AuctionTournamentApiError(
			'경매 팀 설정이 올바르지 않습니다.',
			0,
			'invalid_setup_teams',
			null
		);
	}

	const teamIndexes = teams.map((team) => team.teamIndex).sort((left, right) => left - right);
	if (!teamIndexes.every((teamIndex, index) => teamIndex === index + 1)) {
		throw new AuctionTournamentApiError(
			'경매 팀 순서가 올바르지 않습니다.',
			0,
			'invalid_team_indexes',
			null
		);
	}

	const captainIds = teams.map((team) => team.captainUserId);
	if (new Set(captainIds).size !== captainIds.length) {
		throw new AuctionTournamentApiError(
			'같은 참가자를 여러 팀장으로 지정할 수 없습니다.',
			0,
			'duplicate_captain',
			null
		);
	}
}

export function fetchAuctionSetupDraft(
	guildId: string,
	recruitmentId: string
): Promise<AuctionSetupDraftSnapshot> {
	return requestAuctionSetupDraft(
		apiRoute(['guilds', guildId, 'auction-recruitments', recruitmentId, 'setup-draft']),
		{ headers: { accept: 'application/json' } },
		'load_setup'
	);
}

export async function saveAuctionSetupDraft(
	guildId: string,
	recruitmentId: string,
	command: AuctionSetupDraftSaveCommand
): Promise<AuctionSetupDraftSnapshot> {
	if (!z.uuid().safeParse(recruitmentId).success) {
		throw new AuctionTournamentApiError(
			'경매 모집 정보가 올바르지 않습니다.',
			0,
			'invalid_recruitment_id',
			null
		);
	}
	if (!revisionSchema.safeParse(command.expectedRevision).success) {
		throw new AuctionTournamentApiError(
			'경매 준비 버전 정보가 올바르지 않습니다.',
			0,
			'invalid_revision',
			null
		);
	}
	validateSetupTeams(command.teams);

	return await requestAuctionSetupDraft(
		apiRoute(['guilds', guildId, 'auction-recruitments', recruitmentId, 'setup-draft']),
		{
			method: 'PATCH',
			headers: mutationHeaders(command.idempotencyKey),
			body: JSON.stringify({ expectedRevision: command.expectedRevision, teams: command.teams })
		},
		'save_setup'
	);
}

export async function createAuctionTournament(
	guildId: string,
	command: AuctionTournamentCreateCommand
): Promise<AuctionTournamentNavigation> {
	if (!z.uuid().safeParse(command.sourceRecruitmentId).success) {
		throw new AuctionTournamentApiError(
			'경매 모집 정보가 올바르지 않습니다.',
			0,
			'invalid_recruitment_id',
			null
		);
	}
	if (!revisionSchema.safeParse(command.expectedRevision).success) {
		throw new AuctionTournamentApiError(
			'경매 준비 버전 정보가 올바르지 않습니다.',
			0,
			'invalid_revision',
			null
		);
	}

	let response: Response;
	try {
		response = await globalThis.fetch(apiRoute(['guilds', guildId, 'auction-tournaments']), {
			method: 'POST',
			credentials: 'same-origin',
			headers: mutationHeaders(command.idempotencyKey),
			body: JSON.stringify({
				sourceRecruitmentId: command.sourceRecruitmentId,
				expectedRevision: command.expectedRevision
			})
		});
	} catch (cause) {
		if (cause instanceof AuctionTournamentApiError) throw cause;
		throw new AuctionTournamentApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error', null);
	}

	const payload = await responsePayload(response);
	if (!response.ok) throw responseError(response, payload, 'create_tournament');

	const parsed = auctionTournamentCreateEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new AuctionTournamentApiError(
			'경매 입찰 시작 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data.tournament;
}

export function isAuctionSetupDraftRevisionConflict(
	cause: unknown
): cause is AuctionTournamentApiError {
	return cause instanceof AuctionTournamentApiError && cause.status === 409;
}

export function auctionTournamentErrorText(cause: unknown): string {
	if (!(cause instanceof AuctionTournamentApiError)) {
		return '경매 준비 요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}
