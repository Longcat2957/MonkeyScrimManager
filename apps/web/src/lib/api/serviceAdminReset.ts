import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z, type ZodType } from 'zod';

const guildIdSchema = z.string().regex(/^[1-9][0-9]{9,19}$/);
const timestampSchema = z.iso.datetime({ offset: true });
const countSchema = z.number().int().nonnegative();
const resetCountsSchema = z
	.object({
		totalRows: countSchema,
		seasons: countSchema,
		recruitments: countSchema,
		series: countSchema,
		games: countSchema,
		auctionRecruitments: countSchema,
		auctionTournaments: countSchema,
		mmrRows: countSchema,
		importJobs: countSchema
	})
	.strict();
const resetPreviewSchema = z
	.object({
		guildId: guildIdSchema,
		displayName: z.string().min(1),
		memberCount: countSchema,
		activeSeasonName: z.string().min(1),
		confirmation: z.string().min(1),
		counts: resetCountsSchema
	})
	.strict();
const resetResultSchema = z
	.object({
		guildId: guildIdSchema,
		displayName: z.string().min(1),
		resetAt: timestampSchema,
		deleted: resetCountsSchema,
		freshSeason: z
			.object({
				id: z.uuid(),
				name: z.string().min(1)
			})
			.strict()
	})
	.strict();

export type GuildRecordResetPreview = z.infer<typeof resetPreviewSchema>;
export type GuildRecordResetResult = z.infer<typeof resetResultSchema>;

export class ServiceAdminResetApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null = null
	) {
		super(message);
		this.name = 'ServiceAdminResetApiError';
	}
}

export async function fetchGuildRecordResetPreview(
	guildId: string,
	fetcher: typeof globalThis.fetch = globalThis.fetch
): Promise<GuildRecordResetPreview> {
	validateGuildId(guildId);
	return request(
		apiRoute(['admin', 'guilds', guildId, 'record-reset']),
		resetPreviewSchema,
		{},
		fetcher
	);
}

export async function resetGuildRecords(
	guildId: string,
	confirmation: string,
	idempotencyKey: string = globalThis.crypto.randomUUID(),
	fetcher: typeof globalThis.fetch = globalThis.fetch
): Promise<GuildRecordResetResult> {
	validateGuildId(guildId);
	if (!z.uuid().safeParse(idempotencyKey).success) {
		throw new ServiceAdminResetApiError(
			'초기화 요청 식별자가 올바르지 않습니다.',
			0,
			'invalid_idempotency_key'
		);
	}
	const csrfToken = readCookie('monkeybot_csrf');
	if (!csrfToken) {
		throw new ServiceAdminResetApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing'
		);
	}
	return request(
		apiRoute(['admin', 'guilds', guildId, 'record-reset']),
		resetResultSchema,
		{
			method: 'POST',
			headers: {
				'content-type': 'application/json',
				'X-CSRF-Token': csrfToken,
				'Idempotency-Key': idempotencyKey
			},
			body: JSON.stringify({ confirmation })
		},
		fetcher
	);
}

export function serviceAdminResetErrorText(cause: unknown): string {
	if (!(cause instanceof ServiceAdminResetApiError))
		return '테스트 기록 초기화를 처리하지 못했습니다.';
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}

function validateGuildId(guildId: string): void {
	if (!guildIdSchema.safeParse(guildId).success) {
		throw new ServiceAdminResetApiError(
			'Discord 서버 ID 형식이 올바르지 않습니다.',
			0,
			'invalid_guild_id'
		);
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

async function request<T>(
	path: string,
	schema: ZodType<T>,
	init: RequestInit,
	fetcher: typeof globalThis.fetch
): Promise<T> {
	const headers = new Headers(init.headers);
	headers.set('accept', 'application/json');
	let response: Response;
	try {
		response = await fetcher(path, { ...init, credentials: 'same-origin', headers });
	} catch {
		throw new ServiceAdminResetApiError(
			'초기화 API 서버에 연결할 수 없습니다.',
			0,
			'network_error'
		);
	}
	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new ServiceAdminResetApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code,
				parsedError.data.error.requestId
			);
		}
		throw new ServiceAdminResetApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`
		);
	}
	const parsed = z.object({ data: schema, meta: responseMetaSchema }).strict().safeParse(payload);
	if (!parsed.success) {
		const meta = responseMetaSchema.safeParse(
			typeof payload === 'object' && payload !== null
				? (payload as { meta?: unknown }).meta
				: undefined
		);
		throw new ServiceAdminResetApiError(
			'초기화 API 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			meta.success ? meta.data.requestId : null
		);
	}
	return parsed.data.data;
}

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return '초기화 확인 값이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '서비스 관리자 권한 또는 개발 환경 설정이 필요합니다.';
	if (status === 404) return '초기화할 서버를 찾을 수 없습니다.';
	if (status === 409) return '서버 상태가 변경되어 초기화하지 못했습니다. 다시 확인해 주세요.';
	if (status === 503) return '초기화 서비스를 일시적으로 사용할 수 없습니다.';
	return '테스트 기록 초기화를 처리하지 못했습니다.';
}
