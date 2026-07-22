import {
	apiErrorEnvelopeSchema,
	championPreferencesDataSchema,
	responseMetaSchema,
	type ChampionPreferenceLane,
	type ChampionPreferences
} from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

export class ChampionPreferencesApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null = null
	) {
		super(message);
		this.name = 'ChampionPreferencesApiError';
	}
}

const championPreferencesEnvelopeSchema = z
	.object({
		data: championPreferencesDataSchema,
		meta: responseMetaSchema
	})
	.strict();

function readCookie(name: string): string | null {
	const prefix = `${name}=`;
	const value = (globalThis.document?.cookie ?? '')
		.split(';')
		.map((cookie) => cookie.trim())
		.find((cookie) => cookie.startsWith(prefix));
	return value ? decodeURIComponent(value.slice(prefix.length)) : null;
}

function csrfHeaders(): Record<string, string> {
	const token = readCookie('monkeybot_csrf');
	if (!token) {
		throw new ChampionPreferencesApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing'
		);
	}
	return { 'X-CSRF-Token': token };
}

async function requestPreferences(
	path: string,
	init: RequestInit = {}
): Promise<ChampionPreferences> {
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
		throw new ChampionPreferencesApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error');
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new ChampionPreferencesApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code,
				parsedError.data.error.requestId
			);
		}
		throw new ChampionPreferencesApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`
		);
	}

	const parsed = championPreferencesEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new ChampionPreferencesApiError(
			'선호 챔피언 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response'
		);
	}
	return parsed.data.data.preferences;
}

export function championPreferencesErrorText(cause: unknown): string {
	if (!(cause instanceof ChampionPreferencesApiError)) {
		return '선호 챔피언 요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return '선호 챔피언 입력이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '요청을 수행할 권한이 없습니다.';
	if (status === 404) return '선호 챔피언 정보를 찾을 수 없습니다.';
	if (status === 409) return '선호 챔피언 상태가 변경되었습니다. 다시 시도해 주세요.';
	if (status === 503) return '선호 챔피언 서비스를 일시적으로 사용할 수 없습니다.';
	return '선호 챔피언 요청을 처리하지 못했습니다.';
}

export async function fetchChampionPreferences(): Promise<ChampionPreferences> {
	return requestPreferences(apiRoute(['me', 'champion-preferences']));
}

export async function updateChampionPreferences(
	lane: ChampionPreferenceLane,
	championIds: string[]
): Promise<ChampionPreferences> {
	return requestPreferences(apiRoute(['me', 'champion-preferences', lane]), {
		method: 'PUT',
		headers: {
			'content-type': 'application/json',
			...csrfHeaders()
		},
		body: JSON.stringify({ championIds })
	});
}
