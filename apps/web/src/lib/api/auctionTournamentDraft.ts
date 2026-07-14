import {
	auctionTournamentDraftCommandSchema,
	auctionTournamentDraftSnapshotSchema,
	type AuctionTournamentDraftCommand,
	type AuctionTournamentDraftSnapshot
} from '$lib/contracts/auctionTournamentDraft';
import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const idempotencyKeySchema = z.uuid();
const snapshotEnvelopeSchema = z
	.object({ data: auctionTournamentDraftSnapshotSchema, meta: responseMetaSchema })
	.strict();

type AuctionTournamentDraftAction = 'load' | 'command';

export class AuctionTournamentDraftApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'AuctionTournamentDraftApiError';
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
		throw new AuctionTournamentDraftApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing',
			null
		);
	}
	if (!idempotencyKeySchema.safeParse(idempotencyKey).success) {
		throw new AuctionTournamentDraftApiError(
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

function fallbackErrorMessage(status: number, action: AuctionTournamentDraftAction): string {
	if (status === 400 || status === 422) return '경매 드래프트 요청 값이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403)
		return action === 'command'
			? '경매 드래프트를 변경할 길드 운영자 권한이 필요합니다.'
			: '이 경매 토너먼트를 확인할 권한이 없습니다.';
	if (status === 404) return '요청한 경매 토너먼트를 찾을 수 없습니다.';
	if (status === 409)
		return '다른 운영자가 먼저 경매 드래프트를 변경했습니다. 최신 상태를 확인해 주세요.';
	if (status === 503) return '경매 드래프트 서비스를 일시적으로 사용할 수 없습니다.';
	return action === 'command'
		? '경매 드래프트 변경을 처리하지 못했습니다.'
		: '경매 드래프트를 불러오지 못했습니다.';
}

async function responsePayload(response: Response): Promise<unknown> {
	return await response.json().catch(() => null);
}

function responseError(
	response: Response,
	payload: unknown,
	action: AuctionTournamentDraftAction
): AuctionTournamentDraftApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new AuctionTournamentDraftApiError(
			parsed.data.error.message,
			response.status,
			parsed.data.error.code,
			parsed.data.error.requestId
		);
	}
	return new AuctionTournamentDraftApiError(
		fallbackErrorMessage(response.status, action),
		response.status,
		`http_${response.status}`,
		null
	);
}

async function requestSnapshot(
	path: string,
	init: RequestInit,
	action: AuctionTournamentDraftAction
): Promise<AuctionTournamentDraftSnapshot> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, { ...init, credentials: 'same-origin' });
	} catch {
		throw new AuctionTournamentDraftApiError(
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
		throw new AuctionTournamentDraftApiError(
			'경매 드래프트 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

export function fetchAuctionTournamentDraft(
	guildId: string,
	tournamentId: string
): Promise<AuctionTournamentDraftSnapshot> {
	return requestSnapshot(
		apiRoute(['guilds', guildId, 'auction-tournaments', tournamentId]),
		{ headers: { accept: 'application/json' } },
		'load'
	);
}

export async function executeAuctionTournamentDraftCommand(
	guildId: string,
	tournamentId: string,
	command: AuctionTournamentDraftCommand,
	idempotencyKey: string
): Promise<AuctionTournamentDraftSnapshot> {
	const parsedCommand = auctionTournamentDraftCommandSchema.safeParse(command);
	if (!parsedCommand.success) {
		throw new AuctionTournamentDraftApiError(
			'경매 드래프트 명령이 올바르지 않습니다.',
			0,
			'invalid_draft_command',
			null
		);
	}
	return await requestSnapshot(
		apiRoute(['guilds', guildId, 'auction-tournaments', tournamentId, 'commands']),
		{
			method: 'POST',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify(parsedCommand.data)
		},
		'command'
	);
}

export function isAuctionTournamentDraftRevisionConflict(
	cause: unknown
): cause is AuctionTournamentDraftApiError {
	return cause instanceof AuctionTournamentDraftApiError && cause.status === 409;
}

export function auctionTournamentDraftErrorText(cause: unknown): string {
	if (!(cause instanceof AuctionTournamentDraftApiError)) {
		return '경매 드래프트 요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}
