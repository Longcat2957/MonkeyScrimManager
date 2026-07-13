import {
	apiErrorEnvelopeSchema,
	guildAdminMembersDataSchema,
	responseMetaSchema,
	type GuildAdminMembersSnapshot
} from '$lib/contracts/workflow';
import type { GuildMemberStatusCommand, GuildMmrAdjustmentCommand } from '$lib/realtime/admin';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const guildAdminMembersEnvelopeSchema = z
	.object({
		data: guildAdminMembersDataSchema,
		meta: responseMetaSchema
	})
	.strict();

export class GuildAdminMembersApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string
	) {
		super(message);
		this.name = 'GuildAdminMembersApiError';
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
		throw new GuildAdminMembersApiError(
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
): Promise<GuildAdminMembersSnapshot> {
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
		throw new GuildAdminMembersApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error');
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new GuildAdminMembersApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code
			);
		}
		throw new GuildAdminMembersApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`
		);
	}

	const parsed = guildAdminMembersEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new GuildAdminMembersApiError(
			'길드 멤버 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response'
		);
	}
	return parsed.data.data;
}

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return '멤버 관리 요청 값이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '길드 관리자 권한이 필요합니다.';
	if (status === 404) return '길드 멤버를 찾을 수 없습니다.';
	if (status === 409) return '다른 관리자가 먼저 변경했습니다. 최신 상태를 확인해 주세요.';
	if (status === 503) return '길드 멤버 관리 서비스를 일시적으로 사용할 수 없습니다.';
	return '길드 멤버 관리 요청을 처리하지 못했습니다.';
}

export function fetchGuildAdminMembers(guildId: string): Promise<GuildAdminMembersSnapshot> {
	return requestSnapshot(apiRoute(['guilds', guildId, 'members']));
}

export async function adjustGuildMemberMmr(
	guildId: string,
	command: GuildMmrAdjustmentCommand
): Promise<GuildAdminMembersSnapshot> {
	return await requestSnapshot(apiRoute(['guilds', guildId, 'members', command.memberId, 'mmr']), {
		method: 'PATCH',
		headers: {
			...mutationHeaders(),
			'Idempotency-Key': command.idempotencyKey
		},
		body: JSON.stringify({
			lane: command.lane,
			delta: command.delta,
			note: command.note,
			expectedRevision: command.expectedRevision
		})
	});
}

export async function updateGuildMemberStatus(
	guildId: string,
	command: GuildMemberStatusCommand
): Promise<GuildAdminMembersSnapshot> {
	return await requestSnapshot(
		apiRoute(['guilds', guildId, 'members', command.memberId, 'status']),
		{
			method: 'PATCH',
			headers: mutationHeaders(),
			body: JSON.stringify({
				status: command.status,
				expectedRevision: command.expectedRevision
			})
		}
	);
}
