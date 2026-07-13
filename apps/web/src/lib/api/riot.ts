import {
	apiErrorEnvelopeSchema,
	responseMetaSchema,
	riotAccountsDataSchema,
	type RiotAccount
} from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

export interface RiotAccountCreateInput {
	gameName: string;
	tagLine: string;
	platform: 'kr';
}

export class RiotApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string
	) {
		super(message);
		this.name = 'RiotApiError';
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

function csrfHeaders(): Record<string, string> {
	const token = readCookie('monkeybot_csrf');
	if (!token) {
		throw new RiotApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing'
		);
	}
	return { 'X-CSRF-Token': token };
}

function makeEnvelopeSchema<T>(dataSchema: z.ZodType<T>) {
	return z.object({ data: dataSchema, meta: responseMetaSchema }).strict();
}

async function requestRiotAccounts(path: string, init: RequestInit = {}): Promise<RiotAccount[]> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, {
			...init,
			credentials: 'same-origin',
			headers: {
				accept: 'application/json',
				...init.headers
			}
		});
	} catch {
		throw new RiotApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error');
	}

	const payload = await response.json().catch(() => null);

	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new RiotApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code
			);
		}
		throw new RiotApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`
		);
	}

	const parsed = makeEnvelopeSchema(riotAccountsDataSchema).safeParse(payload);
	if (!parsed.success) {
		throw new RiotApiError(
			'Riot 계정 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response'
		);
	}
	return parsed.data.data.riotAccounts;
}

function fallbackErrorMessage(status: number): string {
	if (status === 400) return 'Riot ID 입력이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '요청을 수행할 권한이 없습니다.';
	if (status === 404) return '요청한 Riot 계정을 찾을 수 없습니다.';
	if (status === 409) return '이미 연결된 Riot 계정입니다.';
	if (status === 429) return 'Riot 요청이 제한되었습니다. 잠시 후 다시 시도해 주세요.';
	if (status === 502) return 'Riot 서비스 응답을 처리하지 못했습니다.';
	if (status === 503) return 'Riot 서비스를 일시적으로 사용할 수 없습니다.';
	return 'Riot 계정 요청을 처리하지 못했습니다.';
}

export function parseRiotIdInput(value: string): { gameName: string; tagLine: string } | null {
	const trimmed = value.trim();
	const parts = trimmed.split('#');
	if (parts.length !== 2) return null;

	const gameName = parts[0]?.trim() ?? '';
	const tagLine = parts[1]?.trim() ?? '';
	if (!gameName || !tagLine) return null;
	return { gameName, tagLine };
}

export async function fetchRiotAccounts(): Promise<RiotAccount[]> {
	return requestRiotAccounts(apiRoute(['me', 'riot-accounts']));
}

export async function createRiotAccount(body: RiotAccountCreateInput): Promise<RiotAccount[]> {
	return requestRiotAccounts(apiRoute(['me', 'riot-accounts']), {
		method: 'POST',
		headers: {
			'content-type': 'application/json',
			...csrfHeaders()
		},
		body: JSON.stringify(body)
	});
}

export async function setMainAccount(puuid: string): Promise<RiotAccount[]> {
	return requestRiotAccounts(apiRoute(['me', 'riot-accounts', puuid, 'main']), {
		method: 'PUT',
		headers: csrfHeaders()
	});
}

export async function refreshAccount(puuid: string): Promise<RiotAccount[]> {
	return requestRiotAccounts(apiRoute(['me', 'riot-accounts', puuid, 'refresh']), {
		method: 'POST',
		headers: csrfHeaders()
	});
}

export async function deleteAccount(puuid: string): Promise<RiotAccount[]> {
	return requestRiotAccounts(apiRoute(['me', 'riot-accounts', puuid]), {
		method: 'DELETE',
		headers: csrfHeaders()
	});
}
