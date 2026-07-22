import {
	apiErrorEnvelopeSchema,
	guildRecordsSearchDataSchema,
	responseMetaSchema,
	type GuildRecordsSearchSnapshot
} from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const guildRecordsEnvelopeSchema = z
	.object({
		data: guildRecordsSearchDataSchema,
		meta: responseMetaSchema
	})
	.strict();

const guildRecordsRequestSchema = z
	.object({
		query: z
			.string()
			.trim()
			.min(1)
			.superRefine((query, context) => {
				if ([...query].length > 100) {
					context.addIssue({ code: 'custom', message: 'Query exceeds 100 Unicode scalars' });
				}
			}),
		seasonId: z.uuid().optional(),
		cursor: z.string().min(1).max(4096).optional(),
		limit: z.number().int().min(1).max(50).default(20)
	})
	.strict();

export interface GuildRecordsRequest {
	query: string;
	seasonId?: string;
	cursor?: string;
	limit?: number;
}

export class GuildRecordsApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null = null
	) {
		super(message);
		this.name = 'GuildRecordsApiError';
	}
}

export async function fetchGuildRecords(
	fetcher: typeof globalThis.fetch,
	guildId: string,
	request: GuildRecordsRequest
): Promise<GuildRecordsSearchSnapshot> {
	const parsedRequest = guildRecordsRequestSchema.safeParse(request);
	if (!parsedRequest.success) {
		throw new GuildRecordsApiError(
			'길드 전적 검색 조건이 올바르지 않습니다.',
			0,
			'invalid_search_query'
		);
	}

	const query: Record<string, string | number> = {
		q: parsedRequest.data.query,
		limit: parsedRequest.data.limit
	};
	if (parsedRequest.data.seasonId) query.seasonId = parsedRequest.data.seasonId;
	if (parsedRequest.data.cursor) query.cursor = parsedRequest.data.cursor;

	let response: Response;
	try {
		response = await fetcher(apiRoute(['guilds', guildId, 'records', 'search'], query), {
			method: 'GET',
			credentials: 'same-origin',
			headers: { accept: 'application/json' }
		});
	} catch {
		throw new GuildRecordsApiError('길드 전적 서버에 연결할 수 없습니다.', 0, 'network_error');
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) throw responseError(response.status, payload);

	const parsed = guildRecordsEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new GuildRecordsApiError(
			'길드 전적 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response'
		);
	}
	return parsed.data.data;
}

function responseError(status: number, payload: unknown): GuildRecordsApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new GuildRecordsApiError(
			parsed.data.error.message,
			status,
			parsed.data.error.code,
			parsed.data.error.requestId
		);
	}
	return new GuildRecordsApiError(fallbackErrorMessage(status), status, `http_${status}`);
}

export function guildRecordsErrorText(cause: unknown): string {
	if (!(cause instanceof GuildRecordsApiError)) return '길드 전적 요청을 처리하지 못했습니다.';
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}

function fallbackErrorMessage(status: number): string {
	if (status === 400) return '길드 전적 검색 조건이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '길드 전적을 확인할 권한이 없습니다.';
	if (status === 404) return '요청한 길드 멤버를 찾을 수 없습니다.';
	if (status === 409) return '같은 이름의 길드 멤버가 여러 명입니다.';
	if (status === 503) return '길드 전적 서비스를 일시적으로 사용할 수 없습니다.';
	return '길드 전적 요청을 처리하지 못했습니다.';
}
