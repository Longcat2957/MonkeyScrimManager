import {
	auctionMatchLineupAssignCommandSchema,
	auctionMatchLineupLockCommandSchema,
	auctionMatchLineupUnlockCommandSchema,
	auctionMatchLineupSnapshotSchema,
	type AuctionMatchLineupAssignCommand,
	type AuctionMatchLineupLockCommand,
	type AuctionMatchLineupUnlockCommand,
	type AuctionMatchLineupSnapshot
} from '$lib/contracts/auctionMatchLineup';
import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const idempotencyKeySchema = z.uuid();
const snapshotEnvelopeSchema = z
	.object({ data: auctionMatchLineupSnapshotSchema, meta: responseMetaSchema })
	.strict();

type AuctionMatchLineupAction = 'load' | 'assign' | 'lock' | 'unlock';

export class AuctionMatchLineupApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'AuctionMatchLineupApiError';
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
		throw new AuctionMatchLineupApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing',
			null
		);
	}
	if (!idempotencyKeySchema.safeParse(idempotencyKey).success) {
		throw new AuctionMatchLineupApiError(
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

function fallbackErrorMessage(status: number, action: AuctionMatchLineupAction): string {
	if (status === 400 || status === 422) return '경매 경기 라인업 요청 값이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403)
		return action === 'load'
			? '이 경매 경기를 확인할 권한이 없습니다.'
			: '라인업을 변경할 길드 운영자 권한이 필요합니다.';
	if (status === 404) return '요청한 경매 경기를 찾을 수 없습니다.';
	if (status === 409) return '다른 운영자가 먼저 라인업을 변경했습니다. 최신 상태를 확인해 주세요.';
	if (status === 503) return '경매 경기 라인업 서비스를 일시적으로 사용할 수 없습니다.';
	return action === 'load'
		? '경매 경기 라인업을 불러오지 못했습니다.'
		: '경매 경기 라인업 변경을 처리하지 못했습니다.';
}

async function responsePayload(response: Response): Promise<unknown> {
	return await response.json().catch(() => null);
}

function responseError(
	response: Response,
	payload: unknown,
	action: AuctionMatchLineupAction
): AuctionMatchLineupApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new AuctionMatchLineupApiError(
			parsed.data.error.message,
			response.status,
			parsed.data.error.code,
			parsed.data.error.requestId
		);
	}
	return new AuctionMatchLineupApiError(
		fallbackErrorMessage(response.status, action),
		response.status,
		`http_${response.status}`,
		null
	);
}

async function requestSnapshot(
	path: string,
	init: RequestInit,
	action: AuctionMatchLineupAction
): Promise<AuctionMatchLineupSnapshot> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, { ...init, credentials: 'same-origin' });
	} catch (cause) {
		if (cause instanceof AuctionMatchLineupApiError) throw cause;
		throw new AuctionMatchLineupApiError(
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
		throw new AuctionMatchLineupApiError(
			'경매 경기 라인업 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

function validateMatchId(matchId: string): void {
	if (!z.uuid().safeParse(matchId).success) {
		throw new AuctionMatchLineupApiError(
			'경매 경기 정보가 올바르지 않습니다.',
			0,
			'invalid_match_id',
			null
		);
	}
}

export function fetchAuctionMatchLineup(
	guildId: string,
	matchId: string
): Promise<AuctionMatchLineupSnapshot> {
	validateMatchId(matchId);
	return requestSnapshot(
		apiRoute(['guilds', guildId, 'auction-matches', matchId]),
		{ headers: { accept: 'application/json' } },
		'load'
	);
}

export async function assignAuctionMatchLineup(
	guildId: string,
	matchId: string,
	command: AuctionMatchLineupAssignCommand,
	idempotencyKey: string
): Promise<AuctionMatchLineupSnapshot> {
	validateMatchId(matchId);
	const parsedCommand = auctionMatchLineupAssignCommandSchema.safeParse(command);
	if (!parsedCommand.success) {
		throw new AuctionMatchLineupApiError(
			'라인업 배치 명령이 올바르지 않습니다.',
			0,
			'invalid_lineup_assignment',
			null
		);
	}
	return await requestSnapshot(
		apiRoute(['guilds', guildId, 'auction-matches', matchId, 'lineup']),
		{
			method: 'PATCH',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify(parsedCommand.data)
		},
		'assign'
	);
}

export async function lockAuctionMatchLineup(
	guildId: string,
	matchId: string,
	command: AuctionMatchLineupLockCommand,
	idempotencyKey: string
): Promise<AuctionMatchLineupSnapshot> {
	validateMatchId(matchId);
	const parsedCommand = auctionMatchLineupLockCommandSchema.safeParse(command);
	if (!parsedCommand.success) {
		throw new AuctionMatchLineupApiError(
			'라인업 확정 명령이 올바르지 않습니다.',
			0,
			'invalid_lineup_lock',
			null
		);
	}
	return await requestSnapshot(
		apiRoute(['guilds', guildId, 'auction-matches', matchId, 'lineup', 'lock']),
		{
			method: 'POST',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify(parsedCommand.data)
		},
		'lock'
	);
}

export async function unlockAuctionMatchLineup(
	guildId: string,
	matchId: string,
	command: AuctionMatchLineupUnlockCommand,
	idempotencyKey: string
): Promise<AuctionMatchLineupSnapshot> {
	validateMatchId(matchId);
	const parsedCommand = auctionMatchLineupUnlockCommandSchema.safeParse(command);
	if (!parsedCommand.success) {
		throw new AuctionMatchLineupApiError(
			'라인업 다시 편성 명령이 올바르지 않습니다.',
			0,
			'invalid_lineup_unlock',
			null
		);
	}
	return await requestSnapshot(
		apiRoute(['guilds', guildId, 'auction-matches', matchId, 'lineup', 'unlock']),
		{
			method: 'POST',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify(parsedCommand.data)
		},
		'unlock'
	);
}

export function isAuctionMatchLineupRevisionConflict(
	cause: unknown
): cause is AuctionMatchLineupApiError {
	return (
		cause instanceof AuctionMatchLineupApiError && cause.code === 'auction_match_revision_conflict'
	);
}

export function auctionMatchLineupErrorText(cause: unknown): string {
	if (!(cause instanceof AuctionMatchLineupApiError)) {
		return '경매 경기 라인업 요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}
