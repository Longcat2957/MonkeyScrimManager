import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const guildIdSchema = z.string().regex(/^[1-9][0-9]{0,19}$/);
const defaultGuildEnvelopeSchema = z
	.object({
		data: z.object({ defaultGuildId: guildIdSchema }).strict(),
		meta: responseMetaSchema
	})
	.strict();

export class DefaultGuildApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'DefaultGuildApiError';
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

export async function updateDefaultGuild(guildId: string): Promise<string> {
	if (!guildIdSchema.safeParse(guildId).success) {
		throw new DefaultGuildApiError(
			'대표 길드 선택 값이 올바르지 않습니다.',
			0,
			'invalid_guild_id',
			null
		);
	}

	const csrfToken = readCookie('monkeybot_csrf');
	if (!csrfToken) {
		throw new DefaultGuildApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing',
			null
		);
	}

	let response: Response;
	try {
		response = await globalThis.fetch(apiRoute(['me', 'default-guild']), {
			method: 'PUT',
			credentials: 'same-origin',
			headers: {
				accept: 'application/json',
				'content-type': 'application/json',
				'X-CSRF-Token': csrfToken
			},
			body: JSON.stringify({ guildId })
		});
	} catch {
		throw new DefaultGuildApiError(
			'대표 길드 설정 서버에 연결할 수 없습니다.',
			0,
			'network_error',
			null
		);
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new DefaultGuildApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code,
				parsedError.data.error.requestId
			);
		}
		throw new DefaultGuildApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`,
			null
		);
	}

	const parsed = defaultGuildEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new DefaultGuildApiError(
			'대표 길드 설정 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data.defaultGuildId;
}

export function defaultGuildErrorText(cause: unknown): string {
	if (!(cause instanceof DefaultGuildApiError)) {
		return '대표 길드 저장 중 알 수 없는 오류가 발생했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return '대표 길드 선택 값이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '대표 길드를 변경할 권한이 없습니다.';
	if (status === 404) return '선택한 길드를 찾을 수 없습니다.';
	if (status === 409) return '대표 길드 상태가 변경되었습니다. 새로고침 후 다시 시도해 주세요.';
	if (status === 503) return '대표 길드 설정 서비스를 일시적으로 사용할 수 없습니다.';
	return '대표 길드 설정 요청을 처리하지 못했습니다.';
}
