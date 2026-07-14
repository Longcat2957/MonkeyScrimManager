import {
	recruitmentSnapshotSchema,
	type RecruitmentSnapshot
} from '$lib/contracts/recruitmentSnapshot';
import { apiErrorEnvelopeSchema, responseMetaSchema, userIdSchema } from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const idempotencyKeySchema = z.uuid();
const recruitmentSnapshotEnvelopeSchema = z
	.object({ data: recruitmentSnapshotSchema, meta: responseMetaSchema })
	.strict();

type RecruitmentManagementAction = 'load' | 'add_member' | 'remove_member' | 'start_entry' | 'cancel';

export class RecruitmentManagementApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'RecruitmentManagementApiError';
	}
}

function readCsrfToken(): string | null {
	const prefix = 'monkeybot_csrf=';
	const value = (globalThis.document?.cookie ?? '')
		.split(';')
		.map((cookie) => cookie.trim())
		.find((cookie) => cookie.startsWith(prefix));
	return value ? decodeURIComponent(value.slice(prefix.length)) : null;
}

function mutationHeaders(idempotencyKey: string): Record<string, string> {
	const token = readCsrfToken();
	if (!token) {
		throw new RecruitmentManagementApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing',
			null
		);
	}
	if (!idempotencyKeySchema.safeParse(idempotencyKey).success) {
		throw new RecruitmentManagementApiError(
			'요청 식별자가 올바르지 않습니다.',
			0,
			'invalid_idempotency_key',
			null
		);
	}
	return {
		accept: 'application/json',
		'content-type': 'application/json',
		'X-CSRF-Token': token,
		'Idempotency-Key': idempotencyKey
	};
}

function actionLabel(action: RecruitmentManagementAction): string {
	if (action === 'load') return '내전 모집 조회';
	if (action === 'add_member') return '참가자 추가';
	if (action === 'remove_member') return '참가자 제거';
	if (action === 'start_entry') return '엔트리 편성 시작';
	return '모집 삭제';
}

function fallbackErrorMessage(status: number, action: RecruitmentManagementAction): string {
	const label = actionLabel(action);
	if (status === 400 || status === 422) return `${label} 요청 값이 올바르지 않습니다.`;
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) {
		if (action === 'load') return '이 내전 모집을 볼 권한이 없습니다.';
		if (action === 'add_member') return '참가자 추가에는 길드 운영자 권한이 필요합니다.';
		if (action === 'remove_member') return '참가자 제거에는 길드 운영자 권한이 필요합니다.';
		if (action === 'start_entry') return '엔트리 편성 시작에는 길드 운영자 권한이 필요합니다.';
		return '이 모집을 삭제할 권한이 없습니다.';
	}
	if (status === 404) return '요청한 내전 모집을 찾을 수 없습니다.';
	if (status === 409) return '모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.';
	if (status === 503) return '내전 모집 서비스를 일시적으로 사용할 수 없습니다.';
	return `${label} 요청을 처리하지 못했습니다.`;
}

async function responsePayload(response: Response): Promise<unknown> {
	return await response.json().catch(() => null);
}

function responseError(
	response: Response,
	payload: unknown,
	action: RecruitmentManagementAction
): RecruitmentManagementApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new RecruitmentManagementApiError(
			parsed.data.error.message,
			response.status,
			parsed.data.error.code,
			parsed.data.error.requestId
		);
	}
	return new RecruitmentManagementApiError(
		fallbackErrorMessage(response.status, action),
		response.status,
		`http_${response.status}`,
		null
	);
}

async function requestMutation(
	path: string,
	init: RequestInit,
	action: RecruitmentManagementAction
): Promise<Response> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, {
			...init,
			credentials: 'same-origin'
		});
	} catch (cause) {
		if (cause instanceof RecruitmentManagementApiError) throw cause;
		throw new RecruitmentManagementApiError(
			'API 서버에 연결할 수 없습니다.',
			0,
			'network_error',
			null
		);
	}

	if (!response.ok) {
		throw responseError(response, await responsePayload(response), action);
	}
	return response;
}

export async function fetchRecruitmentSnapshot(
	guildId: string,
	recruitmentId: string
): Promise<RecruitmentSnapshot> {
	const response = await requestMutation(
		apiRoute(['guilds', guildId, 'recruitments', recruitmentId]),
		{ headers: { accept: 'application/json' } },
		'load'
	);
	const payload = await responsePayload(response);
	const parsed = recruitmentSnapshotEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new RecruitmentManagementApiError(
			'내전 모집 조회 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

export async function addRecruitmentMember(
	guildId: string,
	recruitmentId: string,
	memberId: string,
	idempotencyKey: string
): Promise<RecruitmentSnapshot> {
	if (!userIdSchema.safeParse(memberId).success) {
		throw new RecruitmentManagementApiError(
			'추가할 길드 멤버 정보가 올바르지 않습니다.',
			0,
			'invalid_member_id',
			null
		);
	}

	const response = await requestMutation(
		apiRoute(['guilds', guildId, 'recruitments', recruitmentId, 'members']),
		{
			method: 'POST',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify({ memberId })
		},
		'add_member'
	);
	const payload = await responsePayload(response);
	const parsed = recruitmentSnapshotEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new RecruitmentManagementApiError(
			'참가자 추가 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

export async function removeRecruitmentMember(
	guildId: string,
	recruitmentId: string,
	memberId: string,
	idempotencyKey: string
): Promise<RecruitmentSnapshot> {
	if (!userIdSchema.safeParse(memberId).success) {
		throw new RecruitmentManagementApiError(
			'제거할 참가자 정보가 올바르지 않습니다.',
			0,
			'invalid_member_id',
			null
		);
	}

	const response = await requestMutation(
		apiRoute(['guilds', guildId, 'recruitments', recruitmentId, 'members', memberId]),
		{
			method: 'DELETE',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify({})
		},
		'remove_member'
	);
	const payload = await responsePayload(response);
	const parsed = recruitmentSnapshotEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new RecruitmentManagementApiError(
			'참가자 제거 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

export async function closeRecruitmentForEntry(
	guildId: string,
	recruitmentId: string,
	idempotencyKey: string
): Promise<RecruitmentSnapshot> {
	const response = await requestMutation(
		apiRoute(['guilds', guildId, 'recruitments', recruitmentId, 'close']),
		{
			method: 'POST',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify({})
		},
		'start_entry'
	);
	const payload = await responsePayload(response);
	const parsed = recruitmentSnapshotEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new RecruitmentManagementApiError(
			'엔트리 편성 시작 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

export async function cancelRecruitment(
	guildId: string,
	recruitmentId: string,
	idempotencyKey: string
): Promise<void> {
	await requestMutation(
		apiRoute(['guilds', guildId, 'recruitments', recruitmentId]),
		{
			method: 'DELETE',
			headers: mutationHeaders(idempotencyKey)
		},
		'cancel'
	);
}

export function recruitmentManagementErrorText(cause: unknown): string {
	if (!(cause instanceof RecruitmentManagementApiError)) {
		return '내전 모집 요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}
