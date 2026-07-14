import { seriesResultSnapshotSchema, type SeriesResultSnapshot } from '$lib/contracts/seriesResult';
import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const seriesIdSchema = z.uuid();
const seriesResultEnvelopeSchema = z
	.object({ data: seriesResultSnapshotSchema, meta: responseMetaSchema })
	.strict();
const responseMetaEnvelopeSchema = z.object({ meta: responseMetaSchema }).passthrough();

export class SeriesResultApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'SeriesResultApiError';
	}
}

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return '시리즈 결과 조회 요청 값이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '시리즈 결과를 볼 수 있는 길드 소속이 필요합니다.';
	if (status === 404) return '요청한 시리즈 결과를 찾을 수 없습니다.';
	if (status === 503) return '시리즈 결과 서비스를 일시적으로 사용할 수 없습니다.';
	return '시리즈 결과를 불러오지 못했습니다.';
}

async function responsePayload(response: Response): Promise<unknown> {
	return await response.json().catch(() => null);
}

function responseRequestId(payload: unknown): string | null {
	const parsed = responseMetaEnvelopeSchema.safeParse(payload);
	return parsed.success ? parsed.data.meta.requestId : null;
}

function responseError(response: Response, payload: unknown): SeriesResultApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new SeriesResultApiError(
			parsed.data.error.message,
			response.status,
			parsed.data.error.code,
			parsed.data.error.requestId
		);
	}
	return new SeriesResultApiError(
		fallbackErrorMessage(response.status),
		response.status,
		`http_${response.status}`,
		null
	);
}

export async function fetchSeriesResult(
	guildId: string,
	seriesId: string
): Promise<SeriesResultSnapshot> {
	if (!seriesIdSchema.safeParse(seriesId).success) {
		throw new SeriesResultApiError(
			'시리즈 정보가 올바르지 않습니다.',
			0,
			'invalid_series_id',
			null
		);
	}

	let response: Response;
	try {
		response = await globalThis.fetch(apiRoute(['guilds', guildId, 'series', seriesId, 'result']), {
			headers: { accept: 'application/json' },
			credentials: 'same-origin'
		});
	} catch {
		throw new SeriesResultApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error', null);
	}

	const payload = await responsePayload(response);
	if (!response.ok) throw responseError(response, payload);

	const parsed = seriesResultEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new SeriesResultApiError(
			'시리즈 결과 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			responseRequestId(payload)
		);
	}
	return parsed.data.data;
}

export function seriesResultErrorText(cause: unknown): string {
	if (!(cause instanceof SeriesResultApiError)) {
		return '시리즈 결과를 불러오지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}
