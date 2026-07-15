import {
	apiErrorEnvelopeSchema,
	auctionRecruitmentListDataSchema,
	auctionTournamentListDataSchema,
	responseMetaSchema,
	type AuctionRecruitmentDto,
	type AuctionTournamentDto
} from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const auctionRecruitmentListEnvelopeSchema = z
	.object({
		data: auctionRecruitmentListDataSchema,
		meta: responseMetaSchema
	})
	.strict();
const auctionTournamentListEnvelopeSchema = z
	.object({
		data: auctionTournamentListDataSchema,
		meta: responseMetaSchema
	})
	.strict();
const pageLimitSchema = z.number().int().min(1).max(100);

export type AuctionTournamentListState = 'active' | 'completed';
export interface AuctionListPageOptions {
	limit?: number;
	cursor?: string | null;
}
export interface AuctionRecruitmentListPage {
	recruitments: AuctionRecruitmentDto[];
	nextCursor: string | null;
}
export interface AuctionTournamentListPage {
	tournaments: AuctionTournamentDto[];
	nextCursor: string | null;
}

export class AuctionListApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'AuctionListApiError';
	}
}

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return '경매 목록 조회 조건이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '이 길드의 경매 목록을 확인할 권한이 없습니다.';
	if (status === 404) return '요청한 길드를 찾을 수 없습니다.';
	if (status === 503) return '경매 목록 서비스를 일시적으로 사용할 수 없습니다.';
	return '경매 목록을 불러오지 못했습니다.';
}

function validatePageOptions(options: AuctionListPageOptions): number {
	const parsedLimit = pageLimitSchema.safeParse(options.limit ?? 100);
	if (!parsedLimit.success) {
		throw new AuctionListApiError(
			'경매 목록 조회 개수가 올바르지 않습니다.',
			0,
			'invalid_limit',
			null
		);
	}
	if (options.cursor !== undefined && options.cursor !== null && options.cursor.length === 0) {
		throw new AuctionListApiError('경매 목록 커서가 올바르지 않습니다.', 0, 'invalid_cursor', null);
	}
	return parsedLimit.data;
}

async function requestAuctionList<T>(
	fetcher: typeof globalThis.fetch,
	path: string,
	schema: z.ZodType<T>
): Promise<T> {
	let response: Response;
	try {
		response = await fetcher(path, {
			credentials: 'same-origin',
			headers: { accept: 'application/json' }
		});
	} catch {
		throw new AuctionListApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error', null);
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new AuctionListApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code,
				parsedError.data.error.requestId
			);
		}
		throw new AuctionListApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`,
			null
		);
	}

	const parsed = schema.safeParse(payload);
	if (!parsed.success) {
		throw new AuctionListApiError(
			'경매 목록 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data;
}

export async function fetchActionableAuctionRecruitments(
	fetcher: typeof globalThis.fetch,
	guildId: string,
	options: AuctionListPageOptions = {}
): Promise<AuctionRecruitmentListPage> {
	const limit = validatePageOptions(options);
	const query: Record<string, string | number> = { state: 'actionable', limit };
	if (options.cursor) query.cursor = options.cursor;

	return await requestAuctionList(
		fetcher,
		apiRoute(['guilds', guildId, 'auction-recruitments'], query),
		auctionRecruitmentListEnvelopeSchema
	).then((envelope) => envelope.data);
}

export async function fetchAuctionTournaments(
	fetcher: typeof globalThis.fetch,
	guildId: string,
	state: AuctionTournamentListState,
	options: AuctionListPageOptions = {}
): Promise<AuctionTournamentListPage> {
	const limit = validatePageOptions(options);
	const query: Record<string, string | number> = { state, limit };
	if (options.cursor) query.cursor = options.cursor;

	return await requestAuctionList(
		fetcher,
		apiRoute(['guilds', guildId, 'auction-tournaments'], query),
		auctionTournamentListEnvelopeSchema
	).then((envelope) => envelope.data);
}

export function auctionListErrorText(cause: unknown): string {
	if (!(cause instanceof AuctionListApiError)) return '경매 목록을 더 불러오지 못했습니다.';
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}
