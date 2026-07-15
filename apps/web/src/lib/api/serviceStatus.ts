import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const timestampSchema = z.iso.datetime({ offset: true });
const serviceStatusDataSchema = z
	.object({
		checkedAt: timestampSchema,
		services: z
			.array(
				z
					.object({
						key: z.enum(['API', 'POSTGRESQL', 'BOT_HEARTBEAT', 'VALKEY']),
						status: z.enum(['normal', 'delayed', 'down']),
						latencyMs: z.number().int().nonnegative().nullable(),
						value: z.string().min(1),
						lastHealthyAt: timestampSchema.nullable(),
						issue: z.string().min(1).nullable()
					})
					.strict()
			)
			.length(4)
	})
	.strict();
const envelopeSchema = z
	.object({
		data: serviceStatusDataSchema,
		meta: responseMetaSchema
	})
	.strict();

export type ServiceStatusSnapshot = z.infer<typeof serviceStatusDataSchema>;

export class ServiceStatusApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'ServiceStatusApiError';
	}
}

export async function fetchServiceStatus(
	fetcher: typeof globalThis.fetch = globalThis.fetch
): Promise<ServiceStatusSnapshot> {
	let response: Response;
	try {
		response = await fetcher(apiRoute(['admin', 'status']), {
			method: 'GET',
			credentials: 'same-origin',
			headers: { accept: 'application/json' }
		});
	} catch {
		throw new ServiceStatusApiError(
			'운영 상태 API 서버에 연결할 수 없습니다.',
			0,
			'network_error',
			null
		);
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new ServiceStatusApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code,
				parsedError.data.error.requestId
			);
		}
		throw new ServiceStatusApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`,
			null
		);
	}

	const parsed = envelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new ServiceStatusApiError(
			'운영 상태 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

export function serviceStatusErrorText(cause: unknown): string {
	if (!(cause instanceof ServiceStatusApiError)) return '운영 상태를 불러오지 못했습니다.';
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}

function fallbackErrorMessage(status: number): string {
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '서비스 관리자 권한이 필요합니다.';
	if (status === 503) return '운영 상태 서비스를 일시적으로 사용할 수 없습니다.';
	return '운영 상태를 불러오지 못했습니다.';
}
