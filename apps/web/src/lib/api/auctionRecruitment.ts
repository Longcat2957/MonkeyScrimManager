import {
	auctionRecruitmentSnapshotSchema,
	type AuctionRecruitmentSnapshot
} from '$lib/contracts/auctionRecruitmentSnapshot';
import {
	participantConfirmationDmDispatchResultSchema,
	type ParticipantConfirmationDmDispatchResult
} from '$lib/contracts/participantConfirmationDm';
import {
	apiErrorEnvelopeSchema,
	auctionBestOfSchema,
	auctionFormatSchema,
	championPreferenceLaneSchema,
	responseMetaSchema,
	userIdSchema,
	type AuctionBestOf,
	type AuctionRecruitmentDto
} from '$lib/contracts/workflow';
import { apiRoute } from '$lib/routing';
import { z } from 'zod';

const idempotencyKeySchema = z.uuid();
const expectedStartAtSchema = z.iso.datetime({ offset: true });
const auctionRecruitmentSnapshotEnvelopeSchema = z
	.object({ data: auctionRecruitmentSnapshotSchema, meta: responseMetaSchema })
	.strict();
const participantConfirmationDmDispatchEnvelopeSchema = z
	.object({ data: participantConfirmationDmDispatchResultSchema, meta: responseMetaSchema })
	.strict();

type AuctionRecruitmentAction =
	| 'create'
	| 'load'
	| 'add_member'
	| 'remove_member'
	| 'close'
	| 'dispatch_participant_confirmation_dms'
	| 'cancel';

export type AuctionRecruitmentCreateOptions = {
	format: AuctionRecruitmentDto['format'];
	semifinalBestOf: AuctionBestOf | null;
	finalBestOf: AuctionBestOf;
	hardFearless: boolean;
	expectedStartAt: string;
};

export type AuctionRecruitmentMemberOptions = {
	memberId: string;
	declaredLane: z.infer<typeof championPreferenceLaneSchema>;
};

export class AuctionRecruitmentApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null
	) {
		super(message);
		this.name = 'AuctionRecruitmentApiError';
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
		throw new AuctionRecruitmentApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing',
			null
		);
	}
	if (!idempotencyKeySchema.safeParse(idempotencyKey).success) {
		throw new AuctionRecruitmentApiError(
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

function actionLabel(action: AuctionRecruitmentAction): string {
	if (action === 'create') return '경매 모집 생성';
	if (action === 'load') return '경매 모집 조회';
	if (action === 'add_member') return '참가자 추가';
	if (action === 'remove_member') return '참가자 제거';
	if (action === 'close') return '경매 모집 마감';
	if (action === 'dispatch_participant_confirmation_dms') return '참가 확정 DM 발송';
	return '경매 모집 삭제';
}

function fallbackErrorMessage(status: number, action: AuctionRecruitmentAction): string {
	const label = actionLabel(action);
	if (status === 400 || status === 422) return `${label} 요청 값이 올바르지 않습니다.`;
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403)
		return action === 'add_member' || action === 'remove_member'
			? '참가자 관리에는 길드 운영자 권한이 필요합니다.'
			: action === 'dispatch_participant_confirmation_dms'
				? '참가 확정 DM 발송에는 길드 운영자 권한이 필요합니다.'
				: `이 경매 모집을 ${action === 'create' ? '만들' : '변경할'} 권한이 없습니다.`;
	if (status === 404)
		return action === 'create'
			? '길드 또는 현재 시즌을 찾을 수 없습니다.'
			: '요청한 경매 모집을 찾을 수 없습니다.';
	if (status === 409) return '경매 모집 상태가 변경되었습니다. 최신 상태를 확인해 주세요.';
	if (status === 503) return '경매 모집 서비스를 일시적으로 사용할 수 없습니다.';
	return `${label} 요청을 처리하지 못했습니다.`;
}

async function responsePayload(response: Response): Promise<unknown> {
	return await response.json().catch(() => null);
}

function responseError(
	response: Response,
	payload: unknown,
	action: AuctionRecruitmentAction
): AuctionRecruitmentApiError {
	const parsed = apiErrorEnvelopeSchema.safeParse(payload);
	if (parsed.success) {
		return new AuctionRecruitmentApiError(
			parsed.data.error.message,
			response.status,
			parsed.data.error.code,
			parsed.data.error.requestId
		);
	}
	return new AuctionRecruitmentApiError(
		fallbackErrorMessage(response.status, action),
		response.status,
		`http_${response.status}`,
		null
	);
}

async function requestAuctionRecruitment(
	path: string,
	init: RequestInit,
	action: AuctionRecruitmentAction
): Promise<AuctionRecruitmentSnapshot> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, {
			...init,
			credentials: 'same-origin'
		});
	} catch (cause) {
		if (cause instanceof AuctionRecruitmentApiError) throw cause;
		throw new AuctionRecruitmentApiError(
			'API 서버에 연결할 수 없습니다.',
			0,
			'network_error',
			null
		);
	}

	const payload = await responsePayload(response);
	if (!response.ok) throw responseError(response, payload, action);

	const parsed = auctionRecruitmentSnapshotEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new AuctionRecruitmentApiError(
			`${actionLabel(action)} 응답 형식이 올바르지 않습니다.`,
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

async function requestParticipantConfirmationDms(
	path: string,
	init: RequestInit,
	action: AuctionRecruitmentAction
): Promise<ParticipantConfirmationDmDispatchResult> {
	let response: Response;
	try {
		response = await globalThis.fetch(path, {
			...init,
			credentials: 'same-origin'
		});
	} catch (cause) {
		if (cause instanceof AuctionRecruitmentApiError) throw cause;
		throw new AuctionRecruitmentApiError(
			'API 서버에 연결할 수 없습니다.',
			0,
			'network_error',
			null
		);
	}

	const payload = await responsePayload(response);
	if (!response.ok) throw responseError(response, payload, action);

	const parsed = participantConfirmationDmDispatchEnvelopeSchema.safeParse(payload);
	if (!parsed.success) {
		throw new AuctionRecruitmentApiError(
			`${actionLabel(action)} 응답 형식이 올바르지 않습니다.`,
			response.status,
			'invalid_response',
			null
		);
	}
	return parsed.data.data;
}

export async function createAuctionRecruitment(
	guildId: string,
	options: AuctionRecruitmentCreateOptions,
	idempotencyKey: string
): Promise<AuctionRecruitmentSnapshot> {
	if (!auctionFormatSchema.safeParse(options.format).success) {
		throw new AuctionRecruitmentApiError(
			'경매 모집 형식이 올바르지 않습니다.',
			0,
			'invalid_format',
			null
		);
	}
	if (!auctionBestOfSchema.safeParse(options.finalBestOf).success) {
		throw new AuctionRecruitmentApiError(
			'결승 시리즈 형식이 올바르지 않습니다.',
			0,
			'invalid_final_best_of',
			null
		);
	}
	if (typeof options.hardFearless !== 'boolean') {
		throw new AuctionRecruitmentApiError(
			'하드 피어리스 설정이 올바르지 않습니다.',
			0,
			'invalid_hard_fearless',
			null
		);
	}
	if (!expectedStartAtSchema.safeParse(options.expectedStartAt).success) {
		throw new AuctionRecruitmentApiError(
			'예상 시작 시간이 올바르지 않습니다.',
			0,
			'invalid_expected_start_at',
			null
		);
	}
	if (options.format === 'TEN_PERSON' && options.semifinalBestOf !== null) {
		throw new AuctionRecruitmentApiError(
			'10인 경매에는 4강 시리즈 형식을 지정할 수 없습니다.',
			0,
			'invalid_semifinal_best_of',
			null
		);
	}
	if (
		options.format === 'TWENTY_PERSON' &&
		!auctionBestOfSchema.safeParse(options.semifinalBestOf).success
	) {
		throw new AuctionRecruitmentApiError(
			'20인 경매에는 4강 시리즈 형식이 필요합니다.',
			0,
			'invalid_semifinal_best_of',
			null
		);
	}
	return await requestAuctionRecruitment(
		apiRoute(['guilds', guildId, 'auction-recruitments']),
		{
			method: 'POST',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify(options)
		},
		'create'
	);
}

export async function fetchAuctionRecruitment(
	guildId: string,
	recruitmentId: string
): Promise<AuctionRecruitmentSnapshot> {
	return await requestAuctionRecruitment(
		apiRoute(['guilds', guildId, 'auction-recruitments', recruitmentId]),
		{ headers: { accept: 'application/json' } },
		'load'
	);
}

export async function addAuctionRecruitmentMember(
	guildId: string,
	recruitmentId: string,
	options: AuctionRecruitmentMemberOptions,
	idempotencyKey: string
): Promise<AuctionRecruitmentSnapshot> {
	if (!userIdSchema.safeParse(options.memberId).success) {
		throw new AuctionRecruitmentApiError(
			'추가할 길드 멤버 정보가 올바르지 않습니다.',
			0,
			'invalid_member_id',
			null
		);
	}
	if (!championPreferenceLaneSchema.safeParse(options.declaredLane).success) {
		throw new AuctionRecruitmentApiError(
			'참가 라인 정보가 올바르지 않습니다.',
			0,
			'invalid_declared_lane',
			null
		);
	}
	return await requestAuctionRecruitment(
		apiRoute(['guilds', guildId, 'auction-recruitments', recruitmentId, 'members']),
		{
			method: 'POST',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify(options)
		},
		'add_member'
	);
}

export async function removeAuctionRecruitmentMember(
	guildId: string,
	recruitmentId: string,
	memberId: string,
	idempotencyKey: string
): Promise<AuctionRecruitmentSnapshot> {
	if (!userIdSchema.safeParse(memberId).success) {
		throw new AuctionRecruitmentApiError(
			'제거할 참가자 정보가 올바르지 않습니다.',
			0,
			'invalid_member_id',
			null
		);
	}
	return await requestAuctionRecruitment(
		apiRoute(['guilds', guildId, 'auction-recruitments', recruitmentId, 'members', memberId]),
		{
			method: 'DELETE',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify({})
		},
		'remove_member'
	);
}

export async function closeAuctionRecruitment(
	guildId: string,
	recruitmentId: string,
	idempotencyKey: string
): Promise<AuctionRecruitmentSnapshot> {
	return await requestAuctionRecruitment(
		apiRoute(['guilds', guildId, 'auction-recruitments', recruitmentId, 'close']),
		{
			method: 'POST',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify({})
		},
		'close'
	);
}

export async function dispatchAuctionRecruitmentParticipantConfirmationDms(
	guildId: string,
	recruitmentId: string,
	idempotencyKey: string
): Promise<ParticipantConfirmationDmDispatchResult> {
	return await requestParticipantConfirmationDms(
		apiRoute([
			'guilds',
			guildId,
			'auction-recruitments',
			recruitmentId,
			'participant-confirmation-dms'
		]),
		{
			method: 'POST',
			headers: mutationHeaders(idempotencyKey),
			body: JSON.stringify({})
		},
		'dispatch_participant_confirmation_dms'
	);
}

export async function cancelAuctionRecruitment(
	guildId: string,
	recruitmentId: string,
	idempotencyKey: string
): Promise<AuctionRecruitmentSnapshot> {
	return await requestAuctionRecruitment(
		apiRoute(['guilds', guildId, 'auction-recruitments', recruitmentId]),
		{
			method: 'DELETE',
			headers: mutationHeaders(idempotencyKey)
		},
		'cancel'
	);
}

export function auctionRecruitmentErrorText(cause: unknown): string {
	if (!(cause instanceof AuctionRecruitmentApiError)) {
		return '경매 모집 요청을 처리하지 못했습니다.';
	}
	return cause.requestId ? `${cause.message} (요청 ID: ${cause.requestId})` : cause.message;
}
