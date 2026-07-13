import { escapeMarkdown } from 'discord.js';

import {
	MonkeyBotApiContractError,
	MonkeyBotApiError,
	MonkeyBotApiTransportError
} from '../api/client.js';

export interface BotErrorReference {
	reason: string;
	code: string;
	requestId: string;
}

export function botErrorReference(error: unknown, interactionId: string): BotErrorReference {
	const discordRequestId = `Discord:${safeIdentifier(interactionId, 'unknown')}`;

	if (error instanceof MonkeyBotApiError) {
		return {
			reason: safePublicMessage(error.message, 'MonkeyBot API가 요청을 거부했습니다.'),
			code: safeIdentifier(error.code, 'api_error'),
			requestId: error.requestId
				? `API:${safeIdentifier(error.requestId, 'unknown')}`
				: discordRequestId
		};
	}
	if (error instanceof MonkeyBotApiTransportError) {
		return {
			reason: 'MonkeyBot API에 연결할 수 없거나 응답 시간이 초과되었습니다.',
			code: 'bot_api_transport_error',
			requestId: discordRequestId
		};
	}
	if (error instanceof MonkeyBotApiContractError) {
		return {
			reason: 'MonkeyBot API 응답 형식을 확인하지 못했습니다.',
			code: 'bot_api_contract_error',
			requestId: discordRequestId
		};
	}
	return {
		reason: '예기치 않은 내부 오류가 발생했습니다.',
		code: 'interaction_failed',
		requestId: discordRequestId
	};
}

export function reasonWithErrorReference(reason: string, reference: BotErrorReference): string {
	return [reason, `오류 코드: \`${reference.code}\``, `요청 ID: \`${reference.requestId}\``].join(
		'\n'
	);
}

function safePublicMessage(value: string, fallback: string): string {
	const withoutControls = [...value]
		.map((character) => {
			const codePoint = character.codePointAt(0);
			return codePoint !== undefined && (codePoint <= 31 || codePoint === 127) ? ' ' : character;
		})
		.join('');
	const normalized = withoutControls.replace(/\s+/g, ' ').trim();
	if (!normalized) return fallback;
	return escapeMarkdown([...normalized].slice(0, 800).join(''));
}

function safeIdentifier(value: string, fallback: string): string {
	const normalized = value.replace(/[^A-Za-z0-9._:-]/g, '').slice(0, 128);
	return normalized || fallback;
}
