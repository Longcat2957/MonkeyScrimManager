import {
	recruitmentSnapshotSchema,
	type RecruitmentSnapshot
} from '$lib/contracts/recruitmentSnapshot';
import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import type { SeriesBestOf } from '$lib/workflow/seriesRules';
import { z } from 'zod';

const idempotencyKeySchema = z.uuid();

const recruitmentCreateEnvelopeSchema = z
	.object({ data: recruitmentSnapshotSchema, meta: responseMetaSchema })
	.strict();

export type RecruitmentCreateSnapshot = RecruitmentSnapshot;
export type RecruitmentCreateOptions = {
	bestOf: SeriesBestOf;
	hardFearless: boolean;
	expectedStartAt: string;
};

export class RecruitmentCreateApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'RecruitmentCreateApiError';
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
		throw new RecruitmentCreateApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing',
			null
		);
	}
	if (!idempotencyKeySchema.safeParse(idempotencyKey).success) {
		throw new RecruitmentCreateApiError(
			'내전 생성 요청 식별자가 올바르지 않습니다.',
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

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return '내전 모집 생성 요청이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '이 길드에서 내전 모집을 만들 권한이 없습니다.';
	if (status === 404) return '길드 또는 현재 시즌을 찾을 수 없습니다.';
	if (status === 409) return '같은 요청이 이미 처리되었거나 현재 상태가 변경되었습니다.';
	if (status === 503) return '내전 모집 서비스를 일시적으로 사용할 수 없습니다.';
	return '내전 모집 생성 요청을 처리하지 못했습니다.';
}

export function recruitmentCreateErrorText(cause: unknown): string {
	if (!(cause instanceof RecruitmentCreateApiError)) {
		return '내전 모집 생성 요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}

export async function createRecruitment(
	guildId: string,
	options: RecruitmentCreateOptions,
	idempotencyKey: string
): Promise<RecruitmentCreateSnapshot> {
	let response: Response;
	try {
		response = await globalThis.fetch(apiRoute(['guilds', guildId, 'recruitments']), {
			method: 'POST',
			credentials: 'same-origin',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify(options)
		});
	} catch (cause) {
		if (cause instanceof RecruitmentCreateApiError) throw cause;
		throw new RecruitmentCreateApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error', null);
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new RecruitmentCreateApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code,
				parsedError.data.error.requestId
			);
		}
		throw new RecruitmentCreateApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`,
			null
		);
	}

	const parsed = recruitmentCreateEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new RecruitmentCreateApiError(
			'내전 모집 생성 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}
