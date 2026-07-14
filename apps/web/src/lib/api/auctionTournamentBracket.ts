import {
	auctionTournamentBracketCommandSchema,
	auctionTournamentBracketSnapshotSchema,
	type AuctionTournamentBracketCommand,
	type AuctionTournamentBracketSnapshot
} from '$lib/contracts/auctionTournamentBracket';
import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const idempotencyKeySchema = z.uuid();
const snapshotEnvelopeSchema = z
	.object({ data: auctionTournamentBracketSnapshotSchema, meta: responseMetaSchema })
	.strict();

type AuctionTournamentBracketAction = 'load' | 'command';

export class AuctionTournamentBracketApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'AuctionTournamentBracketApiError';
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
		throw new AuctionTournamentBracketApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing',
			null
		);
	}
	if (!idempotencyKeySchema.safeParse(idempotencyKey).success) {
		throw new AuctionTournamentBracketApiError(
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

function fallbackErrorMessage(status: number, action: AuctionTournamentBracketAction): string {
	if (status === 400 || status === 422) return '브래킷 요청 값이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403)
		return action === 'command'
			? '브래킷을 변경할 길드 운영자 권한이 필요합니다.'
			: '이 브래킷을 확인할 권한이 없습니다.';
	if (status === 404) return '요청한 경매 토너먼트를 찾을 수 없습니다.';
	if (status === 409) return '다른 운영자가 먼저 브래킷을 변경했습니다. 최신 상태를 확인해 주세요.';
	if (status === 503) return '브래킷 서비스를 일시적으로 사용할 수 없습니다.';
	return action === 'command'
		? '브래킷 변경을 처리하지 못했습니다.'
		: '브래킷을 불러오지 못했습니다.';
}

async function responsePayload(response: Response): Promise<unknown> {
	return await response.json().catch(() => null);
}

function responseError(
	response: Response,
	payload: unknown,
	action: AuctionTournamentBracketAction
): AuctionTournamentBracketApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new AuctionTournamentBracketApiError(
			parsed.data.error.message,
			response.status,
			parsed.data.error.code,
			parsed.data.error.requestId
		);
	}
	return new AuctionTournamentBracketApiError(
		fallbackErrorMessage(response.status, action),
		response.status,
		`http_${response.status}`,
		null
	);
}

async function requestSnapshot(
	path: string,
	init: RequestInit,
	action: AuctionTournamentBracketAction
): Promise<AuctionTournamentBracketSnapshot> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, { ...init, credentials: 'same-origin' });
	} catch {
		throw new AuctionTournamentBracketApiError(
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
		throw new AuctionTournamentBracketApiError(
			'브래킷 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

export function fetchAuctionTournamentBracket(
	guildId: string,
	tournamentId: string
): Promise<AuctionTournamentBracketSnapshot> {
	return requestSnapshot(
		apiRoute(['guilds', guildId, 'auction-tournaments', tournamentId, 'bracket']),
		{ headers: { accept: 'application/json' } },
		'load'
	);
}

export async function executeAuctionTournamentBracketCommand(
	guildId: string,
	tournamentId: string,
	command: AuctionTournamentBracketCommand,
	idempotencyKey: string
): Promise<AuctionTournamentBracketSnapshot> {
	const parsedCommand = auctionTournamentBracketCommandSchema.safeParse(command);
	if (!parsedCommand.success) {
		throw new AuctionTournamentBracketApiError(
			'브래킷 명령이 올바르지 않습니다.',
			0,
			'invalid_bracket_command',
			null
		);
	}
	return await requestSnapshot(
		apiRoute(['guilds', guildId, 'auction-tournaments', tournamentId, 'bracket', 'commands']),
		{
			method: 'POST',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify(parsedCommand.data)
		},
		'command'
	);
}

export function isAuctionTournamentBracketRevisionConflict(
	cause: unknown
): cause is AuctionTournamentBracketApiError {
	return (
		cause instanceof AuctionTournamentBracketApiError &&
		cause.code === 'auction_tournament_revision_conflict'
	);
}

export function auctionTournamentBracketErrorText(cause: unknown): string {
	if (!(cause instanceof AuctionTournamentBracketApiError)) {
		return '브래킷 요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}
