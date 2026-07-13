import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import type { ZodType } from 'zod';
import { z } from 'zod';

export class DevToolsApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'DevToolsApiError';
	}
}

export async function requestDevData<T>(
	path: string,
	dataSchema: ZodType<T>,
	init: RequestInit = {}
): Promise<T> {
	const headers = new Headers(init.headers);
	headers.set('accept', 'application/json');
	if (init.method && init.method !== 'GET') {
		headers.set('content-type', 'application/json');
		headers.set('X-CSRF-Token', requireCsrfToken());
	}

	let response: Response;
	try {
		response = await globalThis.fetch(path, {
			...init,
			credentials: 'same-origin',
			headers
		});
	} catch {
		throw new DevToolsApiError(
			'개발 도구 API 서버에 연결할 수 없습니다.',
			0,
			'network_error',
			null
		);
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new DevToolsApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code,
				parsedError.data.error.requestId
			);
		}
		throw new DevToolsApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`,
			null
		);
	}

	const envelopeSchema = z.object({ data: dataSchema, meta: responseMetaSchema }).strict();
	const parsed = envelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new DevToolsApiError(
			'더미 사용자 API 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

export function devToolsErrorText(cause: unknown): string {
	if (!(cause instanceof DevToolsApiError)) {
		return cause instanceof Error ? cause.message : '요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}

function requireCsrfToken(): string {
	const prefix = 'monkeybot_csrf=';
	const value = (globalThis.document?.cookie ?? '')
		.split(';')
		.map((cookie) => cookie.trim())
		.find((cookie) => cookie.startsWith(prefix));
	if (!value) {
		throw new DevToolsApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing',
			null
		);
	}
	return decodeURIComponent(value.slice(prefix.length));
}

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return '더미 사용자 요청 값이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '개발 도구 관리자 권한이 필요합니다.';
	if (status === 404) return '요청한 더미 사용자 또는 그룹을 찾을 수 없습니다.';
	if (status === 409) return '다른 변경과 충돌했습니다. 최신 상태를 확인해 주세요.';
	if (status === 503) return '더미 사용자 서비스를 일시적으로 사용할 수 없습니다.';
	return '더미 사용자 요청을 처리하지 못했습니다.';
}
