import { apiErrorEnvelopeSchema, responseMetaSchema } from '$lib/contracts/workflow';
import {
	guildDataTransferApplyEnvelopeSchema,
	guildDataTransferExportEnvelopeSchema,
	guildDataTransferPreviewEnvelopeSchema,
	type MemberSeasonMmrSeedArchive,
	type GuildDataTransferApplyResult,
	type GuildDataTransferExportData,
	type GuildDataTransferPreview
} from '$lib/contracts/guildDataTransfer';
import {
	guildFullDataTransferApplyEnvelopeSchema,
	guildFullDataTransferExportEnvelopeSchema,
	guildFullDataTransferPreviewEnvelopeSchema,
	type GuildFullDataTransferApplyResult,
	type GuildFullDataTransferExportData,
	type GuildFullDataTransferPreview,
	type GuildFullSnapshotArchive
} from '$lib/contracts/guildFullDataTransfer';
import { apiRoute } from '$lib/routing';
import { z, type ZodType } from 'zod';

export class GuildDataTransferApiError extends Error {
	constructor(
		message: string,
		readonly status: number,
		readonly code: string,
		readonly requestId: string | null = null
	) {
		super(message);
		this.name = 'GuildDataTransferApiError';
	}
}

export interface ApplyGuildDataTransferCommand {
	importId: string;
	expectedDigest: string;
	expectedTargetRevision: number;
	idempotencyKey: string;
}

export interface ApplyGuildFullDataTransferCommand {
	importId: string;
	expectedDigest: string;
	expectedTargetDigest: string;
	idempotencyKey: string;
}

/** Archive exports are deliberately portable, readable JSON rather than an encrypted container. */
export function serializeGuildMemberSeasonMmrSeedArchive(
	archive: MemberSeasonMmrSeedArchive
): string {
	return `${JSON.stringify(archive, null, 2)}\n`;
}

export function serializeGuildFullSnapshotArchive(archive: GuildFullSnapshotArchive): string {
	return `${JSON.stringify(archive, null, 2)}\n`;
}

function readCookie(name: string): string | null {
	const prefix = `${name}=`;
	const value = (globalThis.document?.cookie ?? '')
		.split(';')
		.map((cookie) => cookie.trim())
		.find((cookie) => cookie.startsWith(prefix));
	return value ? decodeURIComponent(value.slice(prefix.length)) : null;
}

function mutationHeaders(idempotencyKey?: string): Record<string, string> {
	const token = readCookie('monkeybot_csrf');
	if (!token) {
		throw new GuildDataTransferApiError(
			'보안 토큰을 찾을 수 없습니다. 다시 로그인해 주세요.',
			0,
			'csrf_token_missing'
		);
	}
	if (idempotencyKey && !z.uuid().safeParse(idempotencyKey).success) {
		throw new GuildDataTransferApiError(
			'가져오기 요청 식별자가 올바르지 않습니다.',
			0,
			'invalid_idempotency_key'
		);
	}

	return {
		'content-type': 'application/json',
		'X-CSRF-Token': token,
		...(idempotencyKey ? { 'Idempotency-Key': idempotencyKey } : {})
	};
}

async function request<T>(path: string, schema: ZodType<T>, init: RequestInit = {}): Promise<T> {
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
		throw new GuildDataTransferApiError('API 서버에 연결할 수 없습니다.', 0, 'network_error');
	}

	const payload = await response.json().catch(() => null);
	if (!response.ok) {
		const parsedError = apiErrorEnvelopeSchema.safeParse(payload);
		if (parsedError.success) {
			throw new GuildDataTransferApiError(
				parsedError.data.error.message,
				response.status,
				parsedError.data.error.code,
				parsedError.data.error.requestId
			);
		}
		throw new GuildDataTransferApiError(
			fallbackErrorMessage(response.status),
			response.status,
			`http_${response.status}`
		);
	}

	const parsed = schema.safeParse(payload);
	if (!parsed.success) {
		const meta = responseMetaSchema.safeParse(
			typeof payload === 'object' && payload !== null
				? (payload as { meta?: unknown }).meta
				: undefined
		);
		throw new GuildDataTransferApiError(
			'데이터 이동 API 응답 형식이 올바르지 않습니다.',
			response.status,
			'invalid_response',
			meta.success ? meta.data.requestId : null
		);
	}
	return parsed.data;
}

function fallbackErrorMessage(status: number): string {
	if (status === 400 || status === 422) return '가져올 데이터 형식이 올바르지 않습니다.';
	if (status === 401) return '로그인이 만료되었습니다. 다시 로그인해 주세요.';
	if (status === 403) return '길드 관리자 권한이 필요합니다.';
	if (status === 404) return '가져오기 요청을 찾을 수 없습니다.';
	if (status === 409)
		return '길드 상태가 변경되어 데이터를 적용할 수 없습니다. 다시 검증해 주세요.';
	if (status === 413) return '선택한 파일이 허용된 크기를 초과합니다.';
	if (status === 503) return '데이터 이동 서비스를 일시적으로 사용할 수 없습니다.';
	return '데이터 이동 요청을 처리하지 못했습니다.';
}

export async function exportGuildMemberSeasonMmrSeed(
	guildId: string
): Promise<GuildDataTransferExportData> {
	const envelope = await request(
		apiRoute(['guilds', guildId, 'data-export']),
		guildDataTransferExportEnvelopeSchema,
		{
			method: 'POST',
			headers: mutationHeaders()
		}
	);
	return envelope.data;
}

export async function previewGuildMemberSeasonMmrSeedImport(
	guildId: string,
	archive: MemberSeasonMmrSeedArchive
): Promise<GuildDataTransferPreview> {
	const envelope = await request(
		apiRoute(['guilds', guildId, 'data-imports', 'preview']),
		guildDataTransferPreviewEnvelopeSchema,
		{
			method: 'POST',
			headers: mutationHeaders(),
			body: JSON.stringify({ archive })
		}
	);
	return envelope.data;
}

export async function applyGuildMemberSeasonMmrSeedImport(
	guildId: string,
	command: ApplyGuildDataTransferCommand
): Promise<GuildDataTransferApplyResult> {
	const envelope = await request(
		apiRoute(['guilds', guildId, 'data-imports', command.importId, 'apply']),
		guildDataTransferApplyEnvelopeSchema,
		{
			method: 'POST',
			headers: mutationHeaders(command.idempotencyKey),
			body: JSON.stringify({
				expectedDigest: command.expectedDigest,
				expectedTargetRevision: command.expectedTargetRevision
			})
		}
	);
	return envelope.data;
}

export async function exportGuildFullSnapshot(
	guildId: string
): Promise<GuildFullDataTransferExportData> {
	const envelope = await request(
		apiRoute(['guilds', guildId, 'full-data-export']),
		guildFullDataTransferExportEnvelopeSchema,
		{
			method: 'POST',
			headers: mutationHeaders()
		}
	);
	return envelope.data;
}

export async function previewGuildFullSnapshotImport(
	guildId: string,
	archive: GuildFullSnapshotArchive
): Promise<GuildFullDataTransferPreview> {
	const envelope = await request(
		apiRoute(['guilds', guildId, 'full-data-imports', 'preview']),
		guildFullDataTransferPreviewEnvelopeSchema,
		{
			method: 'POST',
			headers: mutationHeaders(),
			body: JSON.stringify({ archive })
		}
	);
	return envelope.data;
}

export async function applyGuildFullSnapshotImport(
	guildId: string,
	command: ApplyGuildFullDataTransferCommand
): Promise<GuildFullDataTransferApplyResult> {
	const envelope = await request(
		apiRoute(['guilds', guildId, 'full-data-imports', command.importId, 'apply']),
		guildFullDataTransferApplyEnvelopeSchema,
		{
			method: 'POST',
			headers: mutationHeaders(command.idempotencyKey),
			body: JSON.stringify({
				expectedDigest: command.expectedDigest,
				expectedTargetDigest: command.expectedTargetDigest
			})
		}
	);
	return envelope.data;
}
