import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import {
	riotRecordsLookupSchema,
	riotRecentMatchesPageSchema,
	riotRecordsSchema,
	type RiotRecentMatchesPage,
	type RiotRecords,
	type RiotRecordsLookup
} from '$lib/contracts/riotRecords';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const riotRecordsEnvelopeSchema = z
	.object({
		data: riotRecordsSchema,
		meta: responseMetaSchema
	})
	.strict();

const riotRecentMatchesPageEnvelopeSchema = z
	.object({
		data: riotRecentMatchesPageSchema,
		meta: responseMetaSchema
	})
	.strict();

export class RiotRecordsApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string
	) {
		super(message);
		this.name = 'RiotRecordsApiError';
	}
}

interface RiotRecordsLookupInput {
	gameName: string;
	tagLine: string;
	platform: string;
}

export function parseRiotRecordsQuery(value: string): RiotRecordsLookup | null {
	const query = value.trim();
	const separator = query.indexOf('#');
	if (separator <= 0 || separator !== query.lastIndexOf('#') || separator === query.length - 1) {
		return null;
	}

	const lookup = {
		gameName: query.slice(0, separator).trim(),
		tagLine: query.slice(separator + 1).trim(),
		platform: 'KR' as const
	};
	const parsed = riotRecordsLookupSchema.safeParse(lookup);
	return parsed.success ? parsed.data : null;
}

export async function fetchRiotRecords(
	fetcher: typeof globalThis.fetch,
	lookup: RiotRecordsLookup
): Promise<RiotRecords> {
	return requestRiotRecords(
		fetcher,
		apiRoute(['riot', 'records'], {
			gameName: lookup.gameName,
			tagLine: lookup.tagLine,
			platform: lookup.platform
		}),
		{ method: 'GET' }
	);
}

export async function refreshRiotRecords(lookup: RiotRecordsLookupInput): Promise<RiotRecords> {
	const parsedLookup = riotRecordsLookupSchema.safeParse(lookup);
	if (!parsedLookup.success) {
		throw new RiotRecordsApiError('Riot ID 입력이 올바르지 않습니다.', 0, 'invalid_query');
	}
	return requestRiotRecords(globalThis.fetch, apiRoute(['riot', 'records', 'refresh']), {
		method: 'POST',
		headers: {
			'content-type': 'application/json',
			...csrfHeaders()
		},
		body: JSON.stringify(parsedLookup.data)
	});
}

export async function fetchMoreRiotMatches(
	lookup: RiotRecordsLookupInput,
	offset: number,
	count = 10
): Promise<RiotRecentMatchesPage> {
	const parsedLookup = riotRecordsLookupSchema.safeParse(lookup);
	if (!parsedLookup.success || !Number.isInteger(offset) || offset < 0 || count < 1 || count > 20) {
		throw new RiotRecordsApiError('전적 더 보기 요청이 올바르지 않습니다.', 0, 'invalid_query');
	}

	return requestRiotRecentMatchesPage(
		apiRoute(['riot', 'records', 'matches'], {
			gameName: parsedLookup.data.gameName,
			tagLine: parsedLookup.data.tagLine,
			platform: parsedLookup.data.platform,
			offset: String(offset),
			count: String(count)
		})
	);
}

async function requestRiotRecentMatchesPage(path: string): Promise<RiotRecentMatchesPage> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, {
			method: 'GET',
			credentials: 'same-origin',
			headers: { accept: 'application/json' }
		});
	} catch {
		throw new RiotRecordsApiError('Riot 전적 서버에 연결할 수 없습니다.', 0, 'network_error');
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) throw apiResponseError(response.status, payload);
	const parsed = riotRecentMatchesPageEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new RiotRecordsApiError(
			'Riot 전적 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response'
		);
	}
	return parsed.data.data;
}

async function requestRiotRecords(
	fetcher: typeof globalThis.fetch,
	path: string,
	init: RequestInit
): Promise<RiotRecords> {
	let response: Response;
	try {
		response = await fetcher(path, {
			...init,
			credentials: 'same-origin',
			headers: {
				accept: 'application/json',
				...init.headers
			}
		});
	} catch {
		throw new RiotRecordsApiError('Riot 전적 서버에 연결할 수 없습니다.', 0, 'network_error');
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		throw apiResponseError(response.status, payload);
	}

	const parsed = riotRecordsEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new RiotRecordsApiError(
			'Riot 전적 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response'
		);
	}
	return parsed.data.data;
}

function apiResponseError(status: number, payload: unknown): RiotRecordsApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new RiotRecordsApiError(parsed.data.error.message, status, parsed.data.error.code);
	}
	return new RiotRecordsApiError(fallbackErrorMessage(status), status, `http_${status}`);
}

function csrfHeaders(): Record<string, string> {
	const token = readCookie('monkeybot_csrf');
	if (!token) {
		throw new RiotRecordsApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing'
		);
	}
	return { 'X-CSRF-Token': token };
}

function readCookie(name: string): string | null {
	const prefix = `${name}=`;
	const value = (globalThis.document?.cookie ?? '')
		.split(';')
		.map((cookie) => cookie.trim())
		.find((cookie) => cookie.startsWith(prefix));
	return value ? decodeURIComponent(value.slice(prefix.length)) : null;
}

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return 'Riot ID 입력이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '요청을 수행할 권한이 없습니다.';
	if (status === 404) return '요청한 Riot 계정을 찾을 수 없습니다.';
	if (status === 429) return 'Riot 요청이 제한되었습니다. 잠시 후 다시 시도해 주세요.';
	if (status === 502 || status === 503) {
		return 'Riot 전적 서비스를 일시적으로 사용할 수 없습니다.';
	}
	return 'Riot 전적 요청을 처리하지 못했습니다.';
}
