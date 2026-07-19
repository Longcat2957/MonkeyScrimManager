import {
	entryDraftSlotSchema,
	entryDraftSnapshotSchema,
	type EntryDraftSlot,
	type EntryDraftSnapshot
} from '$lib/contracts/entryDraft';
import {
	auctionRecruitmentSnapshotSchema,
	type AuctionRecruitmentSnapshot
} from '$lib/contracts/auctionRecruitmentSnapshot';
import { apiErrorEnvelopeSchema, responseMetaSchema, userIdSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const idempotencyKeySchema = z.uuid();
const revisionSchema = z.number().int().nonnegative();
const entryDraftEnvelopeSchema = z
	.object({ data: entryDraftSnapshotSchema, meta: responseMetaSchema })
	.strict();
const createdSeriesSchema = z
	.object({
		id: z.uuid(),
		href: z.string().startsWith('/g/')
	})
	.strict();
const auctionRecruitmentSnapshotEnvelopeSchema = z
	.object({ data: auctionRecruitmentSnapshotSchema, meta: responseMetaSchema })
	.strict();
const createSeriesEnvelopeSchema = z
	.object({
		data: z.object({ series: createdSeriesSchema }).strict(),
		meta: responseMetaSchema
	})
	.strict();

type EntryDraftAction =
	| 'load'
	| 'move'
	| 'reset'
	| 'create_series'
	| 'create_auction_recruitment'
	| 'reopen';
export type EntryDraftCreatedSeries = z.infer<typeof createdSeriesSchema>;

export class EntryDraftApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null,
		readonly details?: unknown
	) {
		super(message);
		this.name = 'EntryDraftApiError';
	}
}

export function isEntryDraftRevisionConflict(cause: unknown): cause is EntryDraftApiError {
	return (
		cause instanceof EntryDraftApiError &&
		cause.status === 409 &&
		cause.code === 'entry_draft_revision_conflict'
	);
}

export function isEntryDraftAuthoritativeStateError(
	cause: unknown
): cause is EntryDraftApiError {
	if (!(cause instanceof EntryDraftApiError)) return false;
	return (
		(cause.status === 409 &&
			[
				'entry_draft_revision_conflict',
				'recruitment_revision_conflict',
				'series_source_not_ready',
				'entry_draft_auction_source_not_ready',
				'entry_draft_already_converted_to_series'
			].includes(cause.code)) ||
		(cause.status === 422 && cause.code === 'entry_draft_incomplete')
	);
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
		throw new EntryDraftApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing',
			null
		);
	}
	if (!idempotencyKeySchema.safeParse(idempotencyKey).success) {
		throw new EntryDraftApiError(
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

function actionLabel(action: EntryDraftAction): string {
	if (action === 'move') return '엔트리 배치 변경';
	if (action === 'reset') return '엔트리 초기화';
	if (action === 'create_series') return '시리즈 시작';
	if (action === 'create_auction_recruitment') return '경매 모집 전환';
	if (action === 'reopen') return '모집 재개';
	return '엔트리 조회';
}

function fallbackErrorMessage(status: number, action: EntryDraftAction): string {
	const label = actionLabel(action);
	if (status === 400 || status === 422) return `${label} 요청 값이 올바르지 않습니다.`;
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return `${label}에는 길드 운영자 권한이 필요합니다.`;
	if (status === 404) return '내전 모집 또는 엔트리 정보를 찾을 수 없습니다.';
	if (status === 409) return '다른 운영자가 먼저 변경했습니다. 최신 상태를 확인해 주세요.';
	if (status === 503) return '엔트리 편성 서비스를 일시적으로 사용할 수 없습니다.';
	return `${label} 요청을 처리하지 못했습니다.`;
}

async function responsePayload(response: Response): Promise<unknown> {
	return await response.json().catch(() => null);
}

function responseError(
	response: Response,
	payload: unknown,
	action: EntryDraftAction
): EntryDraftApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new EntryDraftApiError(
			parsed.data.error.message,
			response.status,
			parsed.data.error.code,
			parsed.data.error.requestId,
			parsed.data.error.details
		);
	}
	return new EntryDraftApiError(
		fallbackErrorMessage(response.status, action),
		response.status,
		`http_${response.status}`,
		null
	);
}

async function requestSnapshot(
	path: string,
	init: RequestInit,
	action: EntryDraftAction
): Promise<EntryDraftSnapshot> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, {
			...init,
			credentials: 'same-origin'
		});
	} catch {
		throw new EntryDraftApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error', null);
	}

	const payload = await responsePayload(response);
	if (!response.ok) throw responseError(response, payload, action);

	const parsed = entryDraftEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new EntryDraftApiError(
			'엔트리 편성 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

export function fetchEntryDraft(
	guildId: string,
	recruitmentId: string
): Promise<EntryDraftSnapshot> {
	return requestSnapshot(
		apiRoute(['guilds', guildId, 'recruitments', recruitmentId, 'entry-draft']),
		{ headers: { accept: 'application/json' } },
		'load'
	);
}

export async function moveEntryDraftParticipant(
	guildId: string,
	recruitmentId: string,
	command: {
		expectedRevision: number;
		participantId: string;
		slot: EntryDraftSlot | null;
		idempotencyKey: string;
	}
): Promise<EntryDraftSnapshot> {
	if (!revisionSchema.safeParse(command.expectedRevision).success) {
		throw new EntryDraftApiError(
			'엔트리 버전 정보가 올바르지 않습니다.',
			0,
			'invalid_revision',
			null
		);
	}
	if (!userIdSchema.safeParse(command.participantId).success) {
		throw new EntryDraftApiError(
			'참가자 정보가 올바르지 않습니다.',
			0,
			'invalid_participant_id',
			null
		);
	}
	if (command.slot !== null && !entryDraftSlotSchema.safeParse(command.slot).success) {
		throw new EntryDraftApiError('배치할 위치가 올바르지 않습니다.', 0, 'invalid_entry_slot', null);
	}

	return await requestSnapshot(
		apiRoute(['guilds', guildId, 'recruitments', recruitmentId, 'entry-draft']),
		{
			method: 'PATCH',
			headers: mutationHeaders(command.idempotencyKey),
			body: JSON.stringify({
				expectedRevision: command.expectedRevision,
				operation: {
					type: 'MOVE_PARTICIPANT',
					participantId: command.participantId,
					slot: command.slot
				}
			})
		},
		'move'
	);
}

export async function resetEntryDraft(
	guildId: string,
	recruitmentId: string,
	command: { expectedRevision: number; idempotencyKey: string }
): Promise<EntryDraftSnapshot> {
	if (!revisionSchema.safeParse(command.expectedRevision).success) {
		throw new EntryDraftApiError(
			'엔트리 버전 정보가 올바르지 않습니다.',
			0,
			'invalid_revision',
			null
		);
	}

	return await requestSnapshot(
		apiRoute(['guilds', guildId, 'recruitments', recruitmentId, 'entry-draft', 'reset']),
		{
			method: 'POST',
			headers: mutationHeaders(command.idempotencyKey),
			body: JSON.stringify({ expectedRevision: command.expectedRevision })
		},
		'reset'
	);
}

export async function createSeriesFromEntryDraft(
	guildId: string,
	command: {
		sourceRecruitmentId: string;
		expectedRevision: number;
		idempotencyKey: string;
	}
): Promise<EntryDraftCreatedSeries> {
	if (!z.uuid().safeParse(command.sourceRecruitmentId).success) {
		throw new EntryDraftApiError(
			'내전 모집 정보가 올바르지 않습니다.',
			0,
			'invalid_recruitment_id',
			null
		);
	}
	if (!revisionSchema.safeParse(command.expectedRevision).success) {
		throw new EntryDraftApiError(
			'엔트리 버전 정보가 올바르지 않습니다.',
			0,
			'invalid_revision',
			null
		);
	}

	let response: Response;
	try {
		response = await globalThis.fetch(apiRoute(['guilds', guildId, 'series']), {
			method: 'POST',
			credentials: 'same-origin',
			headers: mutationHeaders(command.idempotencyKey),
			body: JSON.stringify({
				sourceRecruitmentId: command.sourceRecruitmentId,
				expectedRevision: command.expectedRevision
			})
		});
	} catch (cause) {
		if (cause instanceof EntryDraftApiError) throw cause;
		throw new EntryDraftApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error', null);
	}

	const payload = await responsePayload(response);
	if (!response.ok) throw responseError(response, payload, 'create_series');

	const parsed = createSeriesEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new EntryDraftApiError(
			'시리즈 시작 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data.series;
}

export async function createAuctionRecruitmentFromEntryDraft(
	guildId: string,
	recruitmentId: string,
	command: { expectedRevision: number; idempotencyKey: string }
): Promise<AuctionRecruitmentSnapshot> {
	if (!z.uuid().safeParse(recruitmentId).success) {
		throw new EntryDraftApiError(
			'내전 모집 정보가 올바르지 않습니다.',
			0,
			'invalid_recruitment_id',
			null
		);
	}
	if (!revisionSchema.safeParse(command.expectedRevision).success) {
		throw new EntryDraftApiError(
			'모집 버전 정보가 올바르지 않습니다.',
			0,
			'invalid_revision',
			null
		);
	}

	let response: Response;
	try {
		response = await globalThis.fetch(
			apiRoute(['guilds', guildId, 'recruitments', recruitmentId, 'auction-recruitment']),
			{
				method: 'POST',
				credentials: 'same-origin',
				headers: mutationHeaders(command.idempotencyKey),
				body: JSON.stringify({ expectedRevision: command.expectedRevision })
			}
		);
	} catch (cause) {
		if (cause instanceof EntryDraftApiError) throw cause;
		throw new EntryDraftApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error', null);
	}

	const payload = await responsePayload(response);
	if (!response.ok) throw responseError(response, payload, 'create_auction_recruitment');

	const parsed = auctionRecruitmentSnapshotEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new EntryDraftApiError(
			'경매 모집 전환 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

export async function reopenRecruitmentForEntry(
	guildId: string,
	recruitmentId: string,
	command: { expectedRevision: number; idempotencyKey: string }
): Promise<EntryDraftSnapshot> {
	if (!revisionSchema.safeParse(command.expectedRevision).success) {
		throw new EntryDraftApiError(
			'모집 버전 정보가 올바르지 않습니다.',
			0,
			'invalid_revision',
			null
		);
	}

	return await requestSnapshot(
		apiRoute(['guilds', guildId, 'recruitments', recruitmentId, 'reopen']),
		{
			method: 'POST',
			headers: mutationHeaders(command.idempotencyKey),
			body: JSON.stringify({ expectedRevision: command.expectedRevision })
		},
		'reopen'
	);
}

export function entryDraftErrorText(cause: unknown): string {
	if (!(cause instanceof EntryDraftApiError)) {
		return '엔트리 편성 요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}
