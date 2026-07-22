import {
	apiErrorEnvelopeSchema,
	guildAuditLogsDataSchema,
	responseMetaSchema,
	type GuildAuditLogsSnapshot
} from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const guildAuditLogsEnvelopeSchema = z
	.object({
		data: guildAuditLogsDataSchema,
		meta: responseMetaSchema
	})
	.strict();

const pageLimitSchema = z.number().int().min(1).max(100);

export interface GuildAuditLogsPageOptions {
	limit?: number;
	cursor?: string | null;
}

export class GuildAuditLogsApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null = null
	) {
		super(message);
		this.name = 'GuildAuditLogsApiError';
	}
}

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return '감사 로그 조회 조건이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '길드 관리자 권한이 필요합니다.';
	if (status === 404) return '길드 감사 로그를 찾을 수 없습니다.';
	if (status === 503) return '감사 로그 서비스를 일시적으로 사용할 수 없습니다.';
	return '감사 로그 요청을 처리하지 못했습니다.';
}

export async function fetchGuildAuditLogs(
	guildId: string,
	options: GuildAuditLogsPageOptions = {}
): Promise<GuildAuditLogsSnapshot> {
	const parsedLimit = pageLimitSchema.safeParse(options.limit ?? 50);
	if (!parsedLimit.success) {
		throw new GuildAuditLogsApiError(
			'감사 로그 조회 개수가 올바르지 않습니다.',
			0,
			'invalid_limit'
		);
	}
	if (options.cursor !== undefined && options.cursor !== null && options.cursor.length === 0) {
		throw new GuildAuditLogsApiError('감사 로그 커서가 올바르지 않습니다.', 0, 'invalid_cursor');
	}

	const search = new URLSearchParams({ limit: String(parsedLimit.data) });
	if (options.cursor) search.set('cursor', options.cursor);
	const path = `${apiRoute(['guilds', guildId, 'audit-logs'])}?${search}`;

	let response: Response;
	try {
		response = await globalThis.fetch(path, {
			credentials: 'same-origin',
			headers: { accept: 'application/json' }
		});
	} catch {
		throw new GuildAuditLogsApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error');
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new GuildAuditLogsApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code,
				parsedError.data.error.requestId
			);
		}
		throw new GuildAuditLogsApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`
		);
	}

	const parsed = guildAuditLogsEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new GuildAuditLogsApiError(
			'감사 로그 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response'
		);
	}
	return parsed.data.data;
}

export function guildAuditLogsErrorText(cause: unknown): string {
	if (!(cause instanceof GuildAuditLogsApiError)) return '감사 로그 요청을 처리하지 못했습니다.';
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}
