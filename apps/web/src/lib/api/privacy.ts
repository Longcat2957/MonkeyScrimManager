import {
	apiErrorEnvelopeSchema,
	privacySettingsDataSchema,
	responseMetaSchema,
	type PrivacySettings
} from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const privacySettingsEnvelopeSchema = z
	.object({
		data: privacySettingsDataSchema,
		meta: responseMetaSchema
	})
	.strict();

export class PrivacySettingsApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null = null
	) {
		super(message);
		this.name = 'PrivacySettingsApiError';
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
		throw new PrivacySettingsApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing'
		);
	}
	return { 'X-CSRF-Token': token };
}

async function requestPrivacySettings(
	path: string,
	init: RequestInit = {}
): Promise<PrivacySettings> {
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
		throw new PrivacySettingsApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error');
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new PrivacySettingsApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code,
				parsedError.data.error.requestId
			);
		}
		throw new PrivacySettingsApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`
		);
	}

	const parsed = privacySettingsEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new PrivacySettingsApiError(
			'개인정보 설정 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response'
		);
	}
	return parsed.data.data.settings;
}

export function privacySettingsErrorText(cause: unknown): string {
	if (!(cause instanceof PrivacySettingsApiError)) {
		return '개인정보 설정 요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return '개인정보 설정 값이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '요청을 수행할 권한이 없습니다.';
	if (status === 409) return '개인정보 설정이 변경되었습니다. 다시 시도해 주세요.';
	if (status === 503) return '개인정보 설정 서비스를 일시적으로 사용할 수 없습니다.';
	return '개인정보 설정 요청을 처리하지 못했습니다.';
}

export async function fetchPrivacySettings(): Promise<PrivacySettings> {
	return requestPrivacySettings(apiRoute(['me', 'privacy']));
}

export async function updatePrivacySettings(settings: PrivacySettings): Promise<PrivacySettings> {
	return requestPrivacySettings(apiRoute(['me', 'privacy']), {
		method: 'PUT',
		headers: {
			'content-type': 'application/json',
			...csrfHeaders()
		},
		body: JSON.stringify(settings)
	});
}
