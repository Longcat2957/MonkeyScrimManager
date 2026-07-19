import {
	auctionTournamentCancelCommandSchema,
	auctionTournamentCancelPreviewSchema,
	auctionTournamentCancelResultSchema,
	type AuctionTournamentCancelCommand,
	type AuctionTournamentCancelPreview,
	type AuctionTournamentCancelResult
} from '$lib/contracts/auctionTournamentCancel';
import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const idempotencyKeySchema = z.uuid();
const previewEnvelopeSchema = z
	.object({ data: auctionTournamentCancelPreviewSchema, meta: responseMetaSchema })
	.strict();
const resultEnvelopeSchema = z
	.object({ data: auctionTournamentCancelResultSchema, meta: responseMetaSchema })
	.strict();

type AuctionTournamentCancelAction = 'preview' | 'cancel';

export class AuctionTournamentCancelApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'AuctionTournamentCancelApiError';
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
		throw new AuctionTournamentCancelApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing',
			null
		);
	}
	if (!idempotencyKeySchema.safeParse(idempotencyKey).success) {
		throw new AuctionTournamentCancelApiError(
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

function fallbackErrorMessage(status: number, action: AuctionTournamentCancelAction): string {
	if (status === 400 || status === 422) return '경매 내전 조기 종료 요청 값이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '경매 내전을 조기 종료하려면 길드 운영자 권한이 필요합니다.';
	if (status === 404) return '요청한 경매 내전을 찾을 수 없습니다.';
	if (status === 409) return '경매 내전 상태가 변경되었습니다. 최신 상태를 확인해 주세요.';
	if (status === 503) return '경매 내전 조기 종료 서비스를 일시적으로 사용할 수 없습니다.';
	return action === 'preview'
		? '경매 내전 조기 종료 정보를 불러오지 못했습니다.'
		: '경매 내전 조기 종료를 처리하지 못했습니다.';
}

async function responsePayload(response: Response): Promise<unknown> {
	return await response.json().catch(() => null);
}

function responseError(
	response: Response,
	payload: unknown,
	action: AuctionTournamentCancelAction
): AuctionTournamentCancelApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new AuctionTournamentCancelApiError(
			parsed.data.error.message,
			response.status,
			parsed.data.error.code,
			parsed.data.error.requestId
		);
	}
	return new AuctionTournamentCancelApiError(
		fallbackErrorMessage(response.status, action),
		response.status,
		`http_${response.status}`,
		null
	);
}

async function requestCancelPreview(
	path: string,
	init: RequestInit
): Promise<AuctionTournamentCancelPreview> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, { ...init, credentials: 'same-origin' });
	} catch {
		throw new AuctionTournamentCancelApiError(
			'API 서버에 연결할 수 없습니다.',
			0,
			'network_error',
			null
		);
	}

	const payload = await responsePayload(response);
	if (!response.ok) throw responseError(response, payload, 'preview');

	const parsed = previewEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new AuctionTournamentCancelApiError(
			'경매 내전 조기 종료 정보 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

async function requestCancelResult(
	path: string,
	init: RequestInit
): Promise<AuctionTournamentCancelResult> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, { ...init, credentials: 'same-origin' });
	} catch {
		throw new AuctionTournamentCancelApiError(
			'API 서버에 연결할 수 없습니다.',
			0,
			'network_error',
			null
		);
	}

	const payload = await responsePayload(response);
	if (!response.ok) throw responseError(response, payload, 'cancel');

	const parsed = resultEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new AuctionTournamentCancelApiError(
			'경매 내전 조기 종료 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

export function fetchAuctionTournamentCancelPreview(
	guildId: string,
	tournamentId: string
): Promise<AuctionTournamentCancelPreview> {
	return requestCancelPreview(
		apiRoute(['guilds', guildId, 'auction-tournaments', tournamentId, 'cancel-preview']),
		{ headers: { accept: 'application/json' } }
	);
}

export async function cancelAuctionTournament(
	guildId: string,
	tournamentId: string,
	command: AuctionTournamentCancelCommand,
	idempotencyKey: string
): Promise<AuctionTournamentCancelResult> {
	const parsedCommand = auctionTournamentCancelCommandSchema.safeParse(command);
	if (!parsedCommand.success) {
		throw new AuctionTournamentCancelApiError(
			'경매 내전 조기 종료 입력 값이 올바르지 않습니다.',
			0,
			'invalid_cancel_command',
			null
		);
	}
	return await requestCancelResult(
		apiRoute(['guilds', guildId, 'auction-tournaments', tournamentId, 'cancel']),
		{
			method: 'POST',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify(parsedCommand.data)
		}
	);
}

export function isAuctionTournamentCancelRevisionConflict(
	cause: unknown
): cause is AuctionTournamentCancelApiError {
	return (
		cause instanceof AuctionTournamentCancelApiError &&
		cause.code === 'auction_tournament_revision_conflict'
	);
}

export function auctionTournamentCancelErrorText(cause: unknown): string {
	if (!(cause instanceof AuctionTournamentCancelApiError)) {
		return '경매 내전 조기 종료 요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}
