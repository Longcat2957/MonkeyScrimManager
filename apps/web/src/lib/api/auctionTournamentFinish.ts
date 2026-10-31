import {
	auctionTournamentFinishCommandSchema,
	auctionTournamentFinishPreviewSchema,
	auctionTournamentFinishResultSchema,
	type AuctionTournamentFinishCommand,
	type AuctionTournamentFinishPreview,
	type AuctionTournamentFinishResult
} from '$lib/contracts/auctionTournamentFinish';
import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const idempotencyKeySchema = z.uuid();
const previewEnvelopeSchema = z
	.object({ data: auctionTournamentFinishPreviewSchema, meta: responseMetaSchema })
	.strict();
const resultEnvelopeSchema = z
	.object({ data: auctionTournamentFinishResultSchema, meta: responseMetaSchema })
	.strict();

type AuctionTournamentFinishAction = 'preview' | 'finish';

export class AuctionTournamentFinishApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'AuctionTournamentFinishApiError';
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
		throw new AuctionTournamentFinishApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing',
			null
		);
	}
	if (!idempotencyKeySchema.safeParse(idempotencyKey).success) {
		throw new AuctionTournamentFinishApiError(
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

function fallbackErrorMessage(status: number, action: AuctionTournamentFinishAction): string {
	if (status === 400 || status === 422) return '경매 내전 마무리 요청 값이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '경매 내전을 마무리하려면 길드 운영자 권한이 필요합니다.';
	if (status === 404) return '요청한 경매 내전을 찾을 수 없습니다.';
	if (status === 409) return '경매 내전 상태가 변경되었습니다. 최신 상태를 확인해 주세요.';
	if (status === 503) return '경매 내전 마무리 서비스를 일시적으로 사용할 수 없습니다.';
	return action === 'preview'
		? '경매 내전 마무리 정보를 불러오지 못했습니다.'
		: '경매 내전 마무리를 처리하지 못했습니다.';
}

async function responsePayload(response: Response): Promise<unknown> {
	return await response.json().catch(() => null);
}

function responseError(
	response: Response,
	payload: unknown,
	action: AuctionTournamentFinishAction
): AuctionTournamentFinishApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new AuctionTournamentFinishApiError(
			parsed.data.error.message,
			response.status,
			parsed.data.error.code,
			parsed.data.error.requestId
		);
	}
	return new AuctionTournamentFinishApiError(
		fallbackErrorMessage(response.status, action),
		response.status,
		`http_${response.status}`,
		null
	);
}

async function requestFinishPreview(
	path: string,
	init: RequestInit
): Promise<AuctionTournamentFinishPreview> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, { ...init, credentials: 'same-origin' });
	} catch {
		throw new AuctionTournamentFinishApiError(
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
		throw new AuctionTournamentFinishApiError(
			'경매 내전 마무리 정보 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

async function requestFinishResult(
	path: string,
	init: RequestInit
): Promise<AuctionTournamentFinishResult> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, { ...init, credentials: 'same-origin' });
	} catch {
		throw new AuctionTournamentFinishApiError(
			'API 서버에 연결할 수 없습니다.',
			0,
			'network_error',
			null
		);
	}

	const payload = await responsePayload(response);
	if (!response.ok) throw responseError(response, payload, 'finish');

	const parsed = resultEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new AuctionTournamentFinishApiError(
			'경매 내전 마무리 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

export function fetchAuctionTournamentFinishPreview(
	guildId: string,
	tournamentId: string
): Promise<AuctionTournamentFinishPreview> {
	return requestFinishPreview(
		apiRoute(['guilds', guildId, 'auction-tournaments', tournamentId, 'finish-preview']),
		{ headers: { accept: 'application/json' } }
	);
}

export async function finishAuctionTournament(
	guildId: string,
	tournamentId: string,
	command: AuctionTournamentFinishCommand,
	idempotencyKey: string
): Promise<AuctionTournamentFinishResult> {
	const parsedCommand = auctionTournamentFinishCommandSchema.safeParse(command);
	if (!parsedCommand.success) {
		throw new AuctionTournamentFinishApiError(
			'경매 내전 마무리 입력 값이 올바르지 않습니다.',
			0,
			'invalid_finish_command',
			null
		);
	}
	return await requestFinishResult(
		apiRoute(['guilds', guildId, 'auction-tournaments', tournamentId, 'finish']),
		{
			method: 'POST',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify(parsedCommand.data)
		}
	);
}

export function isAuctionTournamentFinishRevisionConflict(
	cause: unknown
): cause is AuctionTournamentFinishApiError {
	return (
		cause instanceof AuctionTournamentFinishApiError &&
		cause.code === 'auction_tournament_revision_conflict'
	);
}

export function auctionTournamentFinishErrorText(cause: unknown): string {
	if (!(cause instanceof AuctionTournamentFinishApiError)) {
		return '경매 내전 마무리 요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}
