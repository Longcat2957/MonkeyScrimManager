import { resolve } from '$app/paths';
import { error, redirect } from '@sveltejs/kit';
import { z, type ZodType } from 'zod';
import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';

export { apiRoute };

export interface ApiGetError {
	status: number;
	code: string | null;
	message: string | null;
	requestId: string | null;
}

interface ApiGetContext {
	url: URL;
	onApiError?: (apiError: ApiGetError) => void;
}

export async function apiGet<T>(
	fetcher: typeof globalThis.fetch,
	path: string,
	dataSchema: ZodType<T>,
	context: ApiGetContext
): Promise<T> {
	let response: Response;
	try {
		response = await fetcher(path, { headers: { accept: 'application/json' } });
	} catch {
		error(503, 'API 서버에 연결할 수 없습니다.');
	}

	const payload = await readJson(response);
	if (!response.ok) {
		const apiError = parseApiGetError(response.status, payload);
		context.onApiError?.(apiError);
		handleErrorResponse(apiError, context.url);
	}

	const envelope = dataSchemaEnvelope(dataSchema).safeParse(payload);
	if (!envelope.success) {
		error(502, 'API 응답 형식이 올바르지 않습니다.');
	}
	return envelope.data.data;
}

function dataSchemaEnvelope<T>(dataSchema: ZodType<T>) {
	return z.object({ data: dataSchema, meta: responseMetaSchema }).strict();
}

async function readJson(response: Response): Promise<unknown> {
	try {
		return await response.json();
	} catch {
		if (response.ok) error(502, 'API 응답 형식이 올바르지 않습니다.');
		return null;
	}
}

function parseApiGetError(status: number, payload: unknown): ApiGetError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	return {
		status,
		code: parsed.success ? parsed.data.error.code : null,
		message: parsed.success ? parsed.data.error.message : null,
		requestId: parsed.success ? parsed.data.error.requestId : null
	};
}

function handleErrorResponse(apiError: ApiGetError, url: URL): never {
	const { status, message: backendMessage, requestId } = apiError;

	if (status === 401) {
		const next = `${url.pathname}${url.search}`;
		const login = resolve('/oauth/[...path]', { path: 'discord/start' });
		redirect(303, `${login}?${new URLSearchParams({ next })}`);
	}
	if (status === 400 || status === 403 || status === 404 || status === 409 || status === 503) {
		error(status, withRequestId(backendMessage ?? fallbackMessage(status), requestId));
	}
	if (status >= 500) {
		error(500, withRequestId('API 요청을 처리하지 못했습니다.', requestId));
	}
	error(502, withRequestId('프론트엔드와 API 요청 계약이 일치하지 않습니다.', requestId));
}

function withRequestId(message: string, requestId: string | null): string {
	return requestId ? `${message} (요청 ID: ${requestId})` : message;
}

function fallbackMessage(status: number): string {
	if (status === 400) return '요청 값이 올바르지 않습니다.';
	if (status === 403) return '요청을 수행할 권한이 없습니다.';
	if (status === 404) return '요청한 데이터를 찾을 수 없습니다.';
	if (status === 409) return '요청 대상을 하나로 확정할 수 없습니다.';
	return 'API 서비스를 일시적으로 사용할 수 없습니다.';
}
