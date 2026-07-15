import {
	apiErrorEnvelopeSchema,
	publicUserProfileDataSchema,
	responseMetaSchema,
	type PublicUserProfile
} from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const discordIdSchema = z.string().regex(/^[1-9][0-9]{0,19}$/);
const userProfileEnvelopeSchema = z
	.object({
		data: publicUserProfileDataSchema,
		meta: responseMetaSchema
	})
	.strict();

export class UserProfileApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'UserProfileApiError';
	}
}

export async function fetchUserProfile(
	fetcher: typeof globalThis.fetch,
	discordId: string
): Promise<PublicUserProfile> {
	if (!discordIdSchema.safeParse(discordId).success) {
		throw new UserProfileApiError(
			'Discord 사용자 ID 형식이 올바르지 않습니다.',
			0,
			'invalid_discord_id',
			null
		);
	}

	let response: Response;
	try {
		response = await fetcher(apiRoute(['users', discordId, 'profile']), {
			method: 'GET',
			credentials: 'same-origin',
			headers: { accept: 'application/json' }
		});
	} catch {
		throw new UserProfileApiError(
			'사용자 프로필 서버에 연결할 수 없습니다.',
			0,
			'network_error',
			null
		);
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new UserProfileApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code,
				parsedError.data.error.requestId
			);
		}
		throw new UserProfileApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`,
			null
		);
	}

	const parsed = userProfileEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new UserProfileApiError(
			'사용자 프로필 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

export function userProfileErrorText(cause: UserProfileApiError): string {
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}

function fallbackErrorMessage(status: number): string {
	if (status === 400) return 'Discord 사용자 ID 형식이 올바르지 않습니다.';
	if (status === 404) return '요청한 사용자 프로필을 찾을 수 없습니다.';
	if (status === 503) return '사용자 프로필 서비스를 일시적으로 사용할 수 없습니다.';
	return '사용자 프로필을 불러오지 못했습니다.';
}
