import {
	apiErrorEnvelopeSchema,
	guildAdminSeasonsDataSchema,
	responseMetaSchema,
	type GuildAdminSeasonsSnapshot
} from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const guildAdminSeasonsEnvelopeSchema = z
	.object({
		data: guildAdminSeasonsDataSchema,
		meta: responseMetaSchema
	})
	.strict();

const idempotencyKeySchema = z.uuid();

export interface StartGuildAdminSeasonCommand {
	name: string;
	expectedCurrentSeasonId: string;
	idempotencyKey: string;
}

export class GuildAdminSeasonsApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string
	) {
		super(message);
		this.name = 'GuildAdminSeasonsApiError';
	}
}

function readCookie(name: string): string | null {
	const prefix = `${name}=`;
	const value = (globalThis.document?.cookie ?? '')
		.split(';')
		.map((cookie) => cookie.trim())
		.find((cookie) => cookie.startsWith(prefix));
	return value ? decodeURIComponent(value.slice(prefix.length)) : null;
}

function mutationHeaders(idempotencyKey: string): Record<string, string> {
	const token = readCookie('monkeybot_csrf');
	if (!token) {
		throw new GuildAdminSeasonsApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing'
		);
	}
	if (!idempotencyKeySchema.safeParse(idempotencyKey).success) {
		throw new GuildAdminSeasonsApiError(
			'새 시즌 요청 식별자가 올바르지 않습니다.',
			0,
			'invalid_idempotency_key'
		);
	}
	return {
		'content-type': 'application/json',
		'X-CSRF-Token': token,
		'Idempotency-Key': idempotencyKey
	};
}

async function requestSnapshot(
	path: string,
	init: RequestInit = {}
): Promise<GuildAdminSeasonsSnapshot> {
	const headers = new Headers(init.headers);
	headers.set('accept', 'application/json');

	let response: Response;
	try {
		response = await globalThis.fetch(path, {
			...init,
			credentials: 'same-origin',
			headers
		});
	} catch {
		throw new GuildAdminSeasonsApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error');
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new GuildAdminSeasonsApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code
			);
		}
		throw new GuildAdminSeasonsApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`
		);
	}

	const parsed = guildAdminSeasonsEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new GuildAdminSeasonsApiError(
			'길드 시즌 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response'
		);
	}
	return parsed.data.data;
}

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return '새 시즌 이름이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '길드 관리자 권한이 필요합니다.';
	if (status === 404) return '길드 시즌을 찾을 수 없습니다.';
	if (status === 409) return '현재 시즌이 변경되었습니다. 최신 상태를 확인해 주세요.';
	if (status === 503) return '길드 시즌 서비스를 일시적으로 사용할 수 없습니다.';
	return '길드 시즌 요청을 처리하지 못했습니다.';
}

export function fetchGuildAdminSeasons(guildId: string): Promise<GuildAdminSeasonsSnapshot> {
	return requestSnapshot(apiRoute(['guilds', guildId, 'seasons']));
}

export async function startGuildAdminSeason(
	guildId: string,
	command: StartGuildAdminSeasonCommand
): Promise<GuildAdminSeasonsSnapshot> {
	return await requestSnapshot(apiRoute(['guilds', guildId, 'seasons']), {
		method: 'POST',
		headers: mutationHeaders(command.idempotencyKey),
		body: JSON.stringify({
			name: command.name,
			expectedCurrentSeasonId: command.expectedCurrentSeasonId
		})
	});
}
