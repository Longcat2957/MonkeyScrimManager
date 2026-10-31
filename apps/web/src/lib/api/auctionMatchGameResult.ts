import {
	auctionFinalBo1CompleteCommandSchema,
	auctionMatchGameResultRevertCommandSchema,
	auctionMatchGameResultSnapshotSchema,
	auctionMatchGameResultSubmitCommandSchema,
	type AuctionFinalBo1CompleteCommand,
	type AuctionMatchGameResultRevertCommand,
	type AuctionMatchGameResultSnapshot,
	type AuctionMatchGameResultSubmitCommand
} from '$lib/contracts/auctionMatchGameResult';
import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const idempotencyKeySchema = z.uuid();
const matchIdSchema = z.uuid();
const snapshotEnvelopeSchema = z
	.object({ data: auctionMatchGameResultSnapshotSchema, meta: responseMetaSchema })
	.strict();
const responseMetaEnvelopeSchema = z.object({ meta: responseMetaSchema }).passthrough();

type AuctionMatchGameResultAction =
	'load' | 'submit' | 'revert_last_game' | 'complete_final_as_bo1';

export class AuctionMatchGameResultApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'AuctionMatchGameResultApiError';
	}
}

export function isAuctionMatchGameResultRevisionConflict(
	cause: unknown
): cause is AuctionMatchGameResultApiError {
	return (
		cause instanceof AuctionMatchGameResultApiError &&
		cause.code === 'auction_match_revision_conflict'
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

function idempotentMutationHeaders(idempotencyKey: string): Record<string, string> {
	const token = readCsrfToken();
	if (!token) {
		throw new AuctionMatchGameResultApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing',
			null
		);
	}
	if (!idempotencyKeySchema.safeParse(idempotencyKey).success) {
		throw new AuctionMatchGameResultApiError(
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

function actionLabel(action: AuctionMatchGameResultAction): string {
	if (action === 'load') return '경매 경기 정보 조회';
	if (action === 'submit') return '경매 게임 결과 기록';
	if (action === 'complete_final_as_bo1') return '경매 결승 BO1 조기 종료';
	return '마지막 경매 게임 결과 되돌리기';
}

function fallbackErrorMessage(status: number, action: AuctionMatchGameResultAction): string {
	const label = actionLabel(action);
	if (status === 400 || status === 422) return `${label} 요청 값이 올바르지 않습니다.`;
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return `${label}에는 길드 운영자 권한이 필요합니다.`;
	if (status === 404) return '요청한 경매 경기를 찾을 수 없습니다.';
	if (status === 409) return '경기 상태가 변경되었습니다. 최신 상태를 확인해 주세요.';
	if (status === 503) return '경매 경기 결과 서비스를 일시적으로 사용할 수 없습니다.';
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
	action: AuctionMatchGameResultAction
): AuctionMatchGameResultApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new AuctionMatchGameResultApiError(
			parsed.data.error.message,
			response.status,
			parsed.data.error.code,
			parsed.data.error.requestId
		);
	}
	return new AuctionMatchGameResultApiError(
		fallbackErrorMessage(response.status, action),
		response.status,
		`http_${response.status}`,
		responseRequestId(payload)
	);
}

function assertMatchId(matchId: string): void {
	if (!matchIdSchema.safeParse(matchId).success) {
		throw new AuctionMatchGameResultApiError(
			'경매 경기 정보가 올바르지 않습니다.',
			0,
			'invalid_match_id',
			null
		);
	}
}

async function requestSnapshot(
	path: string,
	init: RequestInit,
	action: AuctionMatchGameResultAction
): Promise<AuctionMatchGameResultSnapshot> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, { ...init, credentials: 'same-origin' });
	} catch (cause) {
		if (cause instanceof AuctionMatchGameResultApiError) throw cause;
		throw new AuctionMatchGameResultApiError(
			'API 서버에 연결할 수 없습니다.',
			0,
			'network_error',
			null
		);
	}

	const payload = await responsePayload(response);
	if (!response.ok) throw responseError(response, payload, action);

	const parsed = snapshotEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new AuctionMatchGameResultApiError(
			`${actionLabel(action)} 응답 형식이 올바르지 않습니다.`,
			response.status,
			'invalid_response',
			responseRequestId(payload)
		);
	}
	return parsed.data.data;
}

export function fetchAuctionMatchGameResult(
	guildId: string,
	matchId: string
): Promise<AuctionMatchGameResultSnapshot> {
	assertMatchId(matchId);
	return requestSnapshot(
		apiRoute(['guilds', guildId, 'auction-matches', matchId]),
		{ headers: { accept: 'application/json' } },
		'load'
	);
}

export async function submitAuctionMatchGameResult(
	guildId: string,
	matchId: string,
	command: AuctionMatchGameResultSubmitCommand,
	idempotencyKey: string
): Promise<AuctionMatchGameResultSnapshot> {
	assertMatchId(matchId);
	const parsedCommand = auctionMatchGameResultSubmitCommandSchema.safeParse(command);
	if (!parsedCommand.success) {
		throw new AuctionMatchGameResultApiError(
			'경매 게임 결과 입력 값이 올바르지 않습니다.',
			0,
			'invalid_game_result',
			null
		);
	}
	return await requestSnapshot(
		apiRoute(['guilds', guildId, 'auction-matches', matchId, 'games']),
		{
			method: 'POST',
			headers: idempotentMutationHeaders(idempotencyKey),
			body: JSON.stringify(parsedCommand.data)
		},
		'submit'
	);
}

export async function revertLastAuctionMatchGameResult(
	guildId: string,
	matchId: string,
	command: AuctionMatchGameResultRevertCommand,
	idempotencyKey: string
): Promise<AuctionMatchGameResultSnapshot> {
	assertMatchId(matchId);
	const parsedCommand = auctionMatchGameResultRevertCommandSchema.safeParse(command);
	if (!parsedCommand.success) {
		throw new AuctionMatchGameResultApiError(
			'마지막 경매 게임 결과 되돌리기 값이 올바르지 않습니다.',
			0,
			'invalid_game_revert',
			null
		);
	}
	return await requestSnapshot(
		apiRoute(['guilds', guildId, 'auction-matches', matchId, 'games', 'last']),
		{
			method: 'DELETE',
			headers: idempotentMutationHeaders(idempotencyKey),
			body: JSON.stringify(parsedCommand.data)
		},
		'revert_last_game'
	);
}

export async function completeAuctionFinalAsBo1(
	guildId: string,
	matchId: string,
	command: AuctionFinalBo1CompleteCommand,
	idempotencyKey: string
): Promise<AuctionMatchGameResultSnapshot> {
	assertMatchId(matchId);
	const parsedCommand = auctionFinalBo1CompleteCommandSchema.safeParse(command);
	if (!parsedCommand.success) {
		throw new AuctionMatchGameResultApiError(
			'경매 결승 BO1 조기 종료 값이 올바르지 않습니다.',
			0,
			'invalid_final_bo1_completion',
			null
		);
	}
	return await requestSnapshot(
		apiRoute(['guilds', guildId, 'auction-matches', matchId, 'complete']),
		{
			method: 'POST',
			headers: idempotentMutationHeaders(idempotencyKey),
			body: JSON.stringify(parsedCommand.data)
		},
		'complete_final_as_bo1'
	);
}

export function auctionMatchGameResultErrorText(cause: unknown): string {
	if (!(cause instanceof AuctionMatchGameResultApiError)) {
		return '경매 경기 결과 요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}
