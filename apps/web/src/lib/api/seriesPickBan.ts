import {
	seriesGameMutationResultSchema,
	seriesEarlyCompletionOutcomeSchema,
	seriesPickBanDraftOperationSchema,
	seriesPickBanSidePresetSchema,
	seriesPickBanSideSchema,
	seriesPickBanSnapshotSchema,
	type SeriesGameMutationResult,
	type SeriesEarlyCompletionOutcome,
	type SeriesPickBanDraftOperation,
	type SeriesPickBanSnapshot
} from '$lib/contracts/seriesPickBan';
import { apiErrorEnvelopeSchema, responseMetaSchema, userIdSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const idempotencyKeySchema = z.uuid();
const seriesIdSchema = z.uuid();
const revisionSchema = z.number().int().nonnegative();
const setNumberSchema = z.number().int().min(1).max(5);
const championIdSchema = z
	.string()
	.min(1)
	.max(64)
	.regex(/^[A-Za-z0-9]+$/);
const patchVersionSchema = z.string().min(1).max(32).nullable();
const picksSchema = z.record(userIdSchema, championIdSchema.nullable());
const bansSchema = z
	.object({
		BLUE: z.array(championIdSchema.nullable()).length(5),
		RED: z.array(championIdSchema.nullable()).length(5)
	})
	.strict();
const seriesPickBanEnvelopeSchema = z
	.object({ data: seriesPickBanSnapshotSchema, meta: responseMetaSchema })
	.strict();
const gameMutationEnvelopeSchema = z
	.object({ data: seriesGameMutationResultSchema, meta: responseMetaSchema })
	.strict();
const responseMetaEnvelopeSchema = z.object({ meta: responseMetaSchema }).passthrough();
const lifecycleResponseEnvelopeSchema = z
	.object({
		data: z.object({ nextHref: z.string().startsWith('/g/') }).strict(),
		meta: responseMetaSchema
	})
	.strict();

type SeriesPickBanAction =
	'load' | 'draft' | 'submit_game' | 'early_complete' | 'revert_last_game' | 'revert' | 'delete';

export class SeriesPickBanApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'SeriesPickBanApiError';
	}
}

export function isSeriesPickBanRevisionConflict(cause: unknown): cause is SeriesPickBanApiError {
	return (
		cause instanceof SeriesPickBanApiError &&
		cause.status === 409 &&
		cause.code === 'series_revision_conflict'
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

function csrfMutationHeaders(): Record<string, string> {
	const token = readCsrfToken();
	if (!token) {
		throw new SeriesPickBanApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing',
			null
		);
	}
	return {
		accept: 'application/json',
		'content-type': 'application/json',
		'X-CSRF-Token': token
	};
}

function idempotentMutationHeaders(idempotencyKey: string): Record<string, string> {
	if (!idempotencyKeySchema.safeParse(idempotencyKey).success) {
		throw new SeriesPickBanApiError(
			'요청 식별자가 올바르지 않습니다.',
			0,
			'invalid_idempotency_key',
			null
		);
	}
	return { ...csrfMutationHeaders(), 'Idempotency-Key': idempotencyKey };
}

function actionLabel(action: SeriesPickBanAction): string {
	if (action === 'load') return '시리즈 정보 조회';
	if (action === 'draft') return '픽밴 변경';
	if (action === 'submit_game') return '게임 결과 기록';
	if (action === 'early_complete') return '시리즈 조기 종료';
	if (action === 'revert_last_game') return '마지막 게임 결과 되돌리기';
	if (action === 'revert') return '시리즈 되돌리기';
	return '시리즈 취소';
}

function fallbackErrorMessage(status: number, action: SeriesPickBanAction): string {
	const label = actionLabel(action);
	if (status === 400 || status === 422) return `${label} 요청 값이 올바르지 않습니다.`;
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return `${label}에는 길드 운영자 권한이 필요합니다.`;
	if (status === 404) return '요청한 시리즈를 찾을 수 없습니다.';
	if (status === 409) return '시리즈 상태가 변경되었습니다. 최신 상태를 확인해 주세요.';
	if (status === 503) return '시리즈 서비스를 일시적으로 사용할 수 없습니다.';
	return `${label} 요청을 처리하지 못했습니다.`;
}

async function responsePayload(response: Response): Promise<unknown> {
	return await response.json().catch(() => null);
}

function responseRequestId(payload: unknown): string | null {
	const parsed = responseMetaEnvelopeSchema.safeParse(payload);
	return parsed.success ? parsed.data.meta.requestId : null;
}

function responseError(
	response: Response,
	payload: unknown,
	action: SeriesPickBanAction
): SeriesPickBanApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new SeriesPickBanApiError(
			parsed.data.error.message,
			response.status,
			parsed.data.error.code,
			parsed.data.error.requestId
		);
	}
	return new SeriesPickBanApiError(
		fallbackErrorMessage(response.status, action),
		response.status,
		`http_${response.status}`,
		null
	);
}

async function request(
	path: string,
	init: RequestInit,
	action: SeriesPickBanAction
): Promise<{ response: Response; payload: unknown }> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, { ...init, credentials: 'same-origin' });
	} catch (cause) {
		if (cause instanceof SeriesPickBanApiError) throw cause;
		throw new SeriesPickBanApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error', null);
	}

	const payload = await responsePayload(response);
	if (!response.ok) throw responseError(response, payload, action);
	return { response, payload };
}

function assertSeriesId(seriesId: string): void {
	if (!seriesIdSchema.safeParse(seriesId).success) {
		throw new SeriesPickBanApiError(
			'시리즈 정보가 올바르지 않습니다.',
			0,
			'invalid_series_id',
			null
		);
	}
}

function assertRevision(expectedRevision: number): void {
	if (!revisionSchema.safeParse(expectedRevision).success) {
		throw new SeriesPickBanApiError(
			'시리즈 버전 정보가 올바르지 않습니다.',
			0,
			'invalid_revision',
			null
		);
	}
}

function parseSnapshot(
	response: Response,
	payload: unknown,
	action: SeriesPickBanAction
): SeriesPickBanSnapshot {
	const parsed = seriesPickBanEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new SeriesPickBanApiError(
			`${actionLabel(action)} 응답 형식이 올바르지 않습니다.`,
			response.status,
			'invalid_response',
			responseRequestId(payload)
		);
	}
	return parsed.data.data;
}

function parseGameMutation(
	response: Response,
	payload: unknown,
	action: Extract<SeriesPickBanAction, 'submit_game' | 'early_complete' | 'revert_last_game'>
): SeriesGameMutationResult {
	const parsed = gameMutationEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new SeriesPickBanApiError(
			`${actionLabel(action)} 응답 형식이 올바르지 않습니다.`,
			response.status,
			'invalid_response',
			responseRequestId(payload)
		);
	}
	return parsed.data.data;
}

export async function fetchSeriesPickBan(
	guildId: string,
	seriesId: string
): Promise<SeriesPickBanSnapshot> {
	assertSeriesId(seriesId);
	const { response, payload } = await request(
		apiRoute(['guilds', guildId, 'series', seriesId]),
		{ headers: { accept: 'application/json' } },
		'load'
	);
	return parseSnapshot(response, payload, 'load');
}

export async function updateSeriesPickBanDraft(
	guildId: string,
	seriesId: string,
	command: { expectedRevision: number; operation: SeriesPickBanDraftOperation }
): Promise<SeriesPickBanSnapshot> {
	assertSeriesId(seriesId);
	assertRevision(command.expectedRevision);
	if (!seriesPickBanDraftOperationSchema.safeParse(command.operation).success) {
		throw new SeriesPickBanApiError(
			'픽밴 변경 값이 올바르지 않습니다.',
			0,
			'invalid_pickban',
			null
		);
	}

	const { response, payload } = await request(
		apiRoute(['guilds', guildId, 'series', seriesId, 'pickban']),
		{
			method: 'PATCH',
			headers: csrfMutationHeaders(),
			body: JSON.stringify({
				expectedRevision: command.expectedRevision,
				operation: command.operation
			})
		},
		'draft'
	);
	return parseSnapshot(response, payload, 'draft');
}

export async function submitSeriesPickBanGame(
	guildId: string,
	seriesId: string,
	command: {
		setNumber: number;
		expectedRevision: number;
		sidePreset: 'DEFAULT' | 'SWAPPED';
		winner: 'BLUE' | 'RED';
		picks?: Record<string, string | null>;
		bans?: { BLUE: Array<string | null>; RED: Array<string | null> };
		patchVersion?: string | null;
		idempotencyKey: string;
	}
): Promise<SeriesGameMutationResult> {
	assertSeriesId(seriesId);
	assertRevision(command.expectedRevision);
	if (!setNumberSchema.safeParse(command.setNumber).success) {
		throw new SeriesPickBanApiError(
			'세트 번호가 올바르지 않습니다.',
			0,
			'invalid_set_number',
			null
		);
	}
	if (!seriesPickBanSidePresetSchema.safeParse(command.sidePreset).success) {
		throw new SeriesPickBanApiError(
			'사이드 정보가 올바르지 않습니다.',
			0,
			'invalid_side_preset',
			null
		);
	}
	if (!seriesPickBanSideSchema.safeParse(command.winner).success) {
		throw new SeriesPickBanApiError('승리 팀 정보가 올바르지 않습니다.', 0, 'invalid_winner', null);
	}
	if (command.picks !== undefined && !picksSchema.safeParse(command.picks).success) {
		throw new SeriesPickBanApiError('픽 정보가 올바르지 않습니다.', 0, 'invalid_picks', null);
	}
	if (command.bans !== undefined && !bansSchema.safeParse(command.bans).success) {
		throw new SeriesPickBanApiError('밴 정보가 올바르지 않습니다.', 0, 'invalid_bans', null);
	}
	if (
		command.patchVersion !== undefined &&
		!patchVersionSchema.safeParse(command.patchVersion).success
	) {
		throw new SeriesPickBanApiError(
			'패치 버전 정보가 올바르지 않습니다.',
			0,
			'invalid_patch_version',
			null
		);
	}

	const { response, payload } = await request(
		apiRoute(['guilds', guildId, 'series', seriesId, 'games']),
		{
			method: 'POST',
			headers: idempotentMutationHeaders(command.idempotencyKey),
			body: JSON.stringify({
				setNumber: command.setNumber,
				expectedRevision: command.expectedRevision,
				sidePreset: command.sidePreset,
				winner: command.winner,
				picks: command.picks,
				bans: command.bans,
				patchVersion: command.patchVersion
			})
		},
		'submit_game'
	);
	return parseGameMutation(response, payload, 'submit_game');
}

export async function completeSeriesEarly(
	guildId: string,
	seriesId: string,
	command: {
		expectedRevision: number;
		outcome: SeriesEarlyCompletionOutcome;
		idempotencyKey: string;
	}
): Promise<SeriesGameMutationResult> {
	assertSeriesId(seriesId);
	assertRevision(command.expectedRevision);
	if (!seriesEarlyCompletionOutcomeSchema.safeParse(command.outcome).success) {
		throw new SeriesPickBanApiError(
			'조기 종료 결과가 올바르지 않습니다.',
			0,
			'invalid_early_completion_outcome',
			null
		);
	}

	const { response, payload } = await request(
		apiRoute(['guilds', guildId, 'series', seriesId, 'complete']),
		{
			method: 'POST',
			headers: idempotentMutationHeaders(command.idempotencyKey),
			body: JSON.stringify({
				expectedRevision: command.expectedRevision,
				outcome: command.outcome
			})
		},
		'early_complete'
	);
	return parseGameMutation(response, payload, 'early_complete');
}

export async function revertLastSeriesPickBanGame(
	guildId: string,
	seriesId: string,
	command: { expectedRevision: number; idempotencyKey: string }
): Promise<SeriesGameMutationResult> {
	assertSeriesId(seriesId);
	assertRevision(command.expectedRevision);
	const { response, payload } = await request(
		apiRoute(['guilds', guildId, 'series', seriesId, 'games', 'last']),
		{
			method: 'DELETE',
			headers: idempotentMutationHeaders(command.idempotencyKey),
			body: JSON.stringify({ expectedRevision: command.expectedRevision })
		},
		'revert_last_game'
	);
	return parseGameMutation(response, payload, 'revert_last_game');
}

async function lifecycleMutation(
	guildId: string,
	seriesId: string,
	command: { expectedRevision: number; idempotencyKey: string },
	method: 'POST' | 'DELETE',
	action: Extract<SeriesPickBanAction, 'revert' | 'delete'>
): Promise<{ nextHref: string }> {
	assertSeriesId(seriesId);
	assertRevision(command.expectedRevision);
	const segments =
		action === 'revert'
			? ['guilds', guildId, 'series', seriesId, 'revert']
			: ['guilds', guildId, 'series', seriesId];
	const { response, payload } = await request(
		apiRoute(segments),
		{
			method,
			headers: idempotentMutationHeaders(command.idempotencyKey),
			body: JSON.stringify({ expectedRevision: command.expectedRevision })
		},
		action
	);
	const parsed = lifecycleResponseEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new SeriesPickBanApiError(
			`${actionLabel(action)} 응답 형식이 올바르지 않습니다.`,
			response.status,
			'invalid_response',
			responseRequestId(payload)
		);
	}
	return parsed.data.data;
}

export async function revertSeries(
	guildId: string,
	seriesId: string,
	command: { expectedRevision: number; idempotencyKey: string }
): Promise<{ nextHref: string }> {
	return await lifecycleMutation(guildId, seriesId, command, 'POST', 'revert');
}

export async function deleteSeries(
	guildId: string,
	seriesId: string,
	command: { expectedRevision: number; idempotencyKey: string }
): Promise<{ nextHref: string }> {
	return await lifecycleMutation(guildId, seriesId, command, 'DELETE', 'delete');
}

export function seriesPickBanErrorText(cause: unknown): string {
	if (!(cause instanceof SeriesPickBanApiError)) {
		return '시리즈 요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}
