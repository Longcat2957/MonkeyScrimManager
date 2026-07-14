import {
	apiErrorEnvelopeSchema,
	guildAdminSettingsDataSchema,
	responseMetaSchema,
	type GuildAdminSettingsSnapshot
} from '$lib/contracts/workflow';
import type { GuildSettingsUpdateCommand } from '$lib/realtime/admin';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const guildAdminSettingsEnvelopeSchema = z
	.object({
		data: guildAdminSettingsDataSchema,
		meta: responseMetaSchema
	})
	.strict();

export class GuildAdminSettingsApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string
	) {
		super(message);
		this.name = 'GuildAdminSettingsApiError';
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

function mutationHeaders(): Record<string, string> {
	const token = readCookie('monkeybot_csrf');
	if (!token) {
		throw new GuildAdminSettingsApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing'
		);
	}
	return {
		'content-type': 'application/json',
		'X-CSRF-Token': token
	};
}

async function requestSnapshot(
	path: string,
	init: RequestInit = {}
): Promise<GuildAdminSettingsSnapshot> {
	const headers = new Headers(init.headers);
	headers.set('accept', 'application/json');

	let response: Response;
	try {
		response = await globalThis.fetch(path, {
			...init,
			credentials: 'same-origin',
			headers
		});
	} catch {
		throw new GuildAdminSettingsApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error');
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new GuildAdminSettingsApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code
			);
		}
		throw new GuildAdminSettingsApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`
		);
	}

	const parsed = guildAdminSettingsEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new GuildAdminSettingsApiError(
			'길드 설정 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response'
		);
	}
	return parsed.data.data;
}

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return '길드 설정 값이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '길드 관리자 권한이 필요합니다.';
	if (status === 404) return '길드 설정을 찾을 수 없습니다.';
	if (status === 409) return '다른 관리자가 먼저 변경했습니다. 최신 상태를 확인해 주세요.';
	if (status === 503) return '길드 설정 서비스를 일시적으로 사용할 수 없습니다.';
	return '길드 설정 요청을 처리하지 못했습니다.';
}

export function fetchGuildAdminSettings(guildId: string): Promise<GuildAdminSettingsSnapshot> {
	return requestSnapshot(apiRoute(['guilds', guildId, 'settings']));
}

export async function updateGuildAdminSettings(
	guildId: string,
	command: GuildSettingsUpdateCommand
): Promise<GuildAdminSettingsSnapshot> {
	const { settings } = command;
	return await requestSnapshot(apiRoute(['guilds', guildId, 'settings']), {
		method: 'PUT',
		headers: mutationHeaders(),
		body: JSON.stringify({
			displayName: settings.displayName,
			operatorRoleId: settings.operatorRoleId,
			recruitmentChannelId: settings.recruitmentChannelId,
			entryChannelId: settings.entryChannelId,
			resultChannelId: settings.resultChannelId,
			logChannelId: settings.logChannelId,
			isExternallyPublic: settings.isExternallyPublic,
			expectedRevision: command.expectedRevision
		})
	});
}
