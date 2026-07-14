import {
	ChannelType,
	type ChatInputCommandInteraction,
	type Guild,
	type Interaction
} from 'discord.js';
import type { Logger } from 'pino';
import { describe, expect, it, vi } from 'vitest';

import {
	MonkeyBotApiContractError,
	MonkeyBotApiError,
	type MonkeyBotApiClient,
	MonkeyBotApiTransportError
} from '../api/client.js';
import type { BotConfig } from '../config.js';
import type { GuildCatalogSync } from '../discord/catalog-sync.js';
import type { InteractionContext } from './context.js';
import { dispatchInteraction, interactionErrorContent } from './dispatcher.js';

describe('Discord interaction error reporting', () => {
	it('returns the public API reason, code, and request ID without exposing details', () => {
		const content = interactionErrorContent(
			new MonkeyBotApiError({
				status: 409,
				code: 'guild_already_configured',
				message: '이미 초기 설정된 길드입니다.',
				requestId: '01900000-0000-7000-8000-000000000001',
				details: { sql: 'SELECT secret FROM credentials', token: 'never-show-this' }
			}),
			'400000000000000001'
		);

		expect(content).toContain('사유: 이미 초기 설정된 길드입니다.');
		expect(content).toContain('오류 코드: `guild_already_configured`');
		expect(content).toContain('요청 ID: `API:01900000-0000-7000-8000-000000000001`');
		expect(content).not.toContain('SELECT');
		expect(content).not.toContain('never-show-this');
	});

	it.each([
		{
			error: new MonkeyBotApiTransportError('Bearer secret transport detail'),
			code: 'bot_api_transport_error',
			reason: 'MonkeyBot API에 연결할 수 없거나 응답 시간이 초과되었습니다.'
		},
		{
			error: new MonkeyBotApiContractError('database response leaked'),
			code: 'bot_api_contract_error',
			reason: 'MonkeyBot API 응답 형식을 확인하지 못했습니다.'
		},
		{
			error: new Error('stack and token must stay private'),
			code: 'interaction_failed',
			reason: '예기치 않은 내부 오류가 발생했습니다.'
		}
	])('maps $code to a safe cause category and Discord request ID', ({ error, code, reason }) => {
		const content = interactionErrorContent(error, '400000000000000001');

		expect(content).toContain(`사유: ${reason}`);
		expect(content).toContain(`오류 코드: \`${code}\``);
		expect(content).toContain('요청 ID: `Discord:400000000000000001`');
		expect(content).not.toContain(error.message);
	});

	it('edits a deferred /초기설정 reply with the backend rejection reason', async () => {
		const apiError = new MonkeyBotApiError({
			status: 409,
			code: 'guild_already_configured',
			message: '이미 초기 설정이 완료된 길드입니다. 길드 관리 화면에서 설정을 변경해 주세요.',
			requestId: '01900000-0000-7000-8000-000000000001'
		});
		const setupGuild = vi.fn(async () => {
			throw apiError;
		});
		const current = setupInteraction();
		const error = vi.fn();
		const context = {
			api: { setupGuild } as unknown as MonkeyBotApiClient,
			catalogSync: { syncGuild: vi.fn() } as unknown as GuildCatalogSync,
			config: { webOrigin: new URL('http://localhost:5173') } as BotConfig,
			logger: { error, info: vi.fn(), warn: vi.fn() } as unknown as Logger
		} satisfies InteractionContext;

		await dispatchInteraction(current.value, context);

		expect(current.deferReply).toHaveBeenCalledWith({ flags: 64 });
		expect(current.editReply).toHaveBeenCalledWith({
			content: [
				'요청을 처리하지 못했습니다.',
				'사유: 이미 초기 설정이 완료된 길드입니다. 길드 관리 화면에서 설정을 변경해 주세요.',
				'오류 코드: `guild_already_configured`',
				'요청 ID: `API:01900000-0000-7000-8000-000000000001`'
			].join('\n'),
			allowedMentions: { parse: [] }
		});
		expect(current.followUp).not.toHaveBeenCalled();
		expect(context.catalogSync.syncGuild).not.toHaveBeenCalled();
		expect(error).toHaveBeenCalledWith(
			expect.objectContaining({
				error: apiError,
				interactionId: '400000000000000001',
				guildId: '200000000000000001'
			}),
			'Discord interaction failed'
		);
	});

	it('keeps an error private after a command deferred a public reply', async () => {
		const apiError = new MonkeyBotApiError({
			status: 503,
			code: 'database_unavailable',
			message: '데이터베이스를 일시적으로 사용할 수 없습니다.',
			requestId: '01900000-0000-7000-8000-000000000002'
		});
		const createRecruitment = vi.fn(async () => {
			throw apiError;
		});
		const current = recruitInteraction();
		const context = {
			api: { createRecruitment } as unknown as MonkeyBotApiClient,
			catalogSync: {} as GuildCatalogSync,
			config: { webOrigin: new URL('http://localhost:5173') } as BotConfig,
			logger: { error: vi.fn(), info: vi.fn(), warn: vi.fn() } as unknown as Logger
		} satisfies InteractionContext;

		await dispatchInteraction(current.value, context);

		expect(current.followUp).toHaveBeenCalledWith(
			expect.objectContaining({
				content: expect.stringContaining('오류 코드: `database_unavailable`'),
				flags: 64
			})
		);
		expect(current.deleteReply).toHaveBeenCalledOnce();
		expect(current.editReply).not.toHaveBeenCalled();
	});
});

function setupInteraction(): {
	value: Interaction;
	deferReply: ReturnType<typeof vi.fn>;
	editReply: ReturnType<typeof vi.fn>;
	followUp: ReturnType<typeof vi.fn>;
} {
	const guild = {
		id: '200000000000000001',
		name: 'MSM Seoul',
		icon: null
	} as Guild;
	const deferReply = vi.fn(async () => {
		interaction.deferred = true;
		interaction.ephemeral = true;
	});
	const editReply = vi.fn(async () => undefined);
	const followUp = vi.fn(async () => undefined);
	const interaction = {
		id: '400000000000000001',
		commandName: '초기설정',
		guildId: guild.id,
		guild,
		member: { nick: 'Longcat', roles: ['300000000000000001'] },
		user: {
			id: '100000000000000001',
			username: 'Longcat',
			globalName: 'Longcat',
			avatar: null
		},
		options: {
			getRole: () => ({ id: '300000000000000001', managed: false }),
			getString: () => null,
			getChannel: (name: string) =>
				name === '모집채널'
					? {
							id: '500000000000000001',
							guildId: guild.id,
							type: ChannelType.GuildText,
							isDMBased: () => false,
							isTextBased: () => true,
							isSendable: () => true
						}
					: null
		},
		context: 0,
		authorizingIntegrationOwners: {},
		deferred: false,
		replied: false,
		ephemeral: false,
		inCachedGuild: () => true,
		isChatInputCommand: () => true,
		isButton: () => false,
		isStringSelectMenu: () => false,
		isRepliable: () => true,
		deferReply,
		editReply,
		followUp,
		reply: vi.fn(async () => undefined)
	} as unknown as ChatInputCommandInteraction & { deferred: boolean; ephemeral: boolean };

	return {
		value: interaction as Interaction,
		deferReply,
		editReply,
		followUp
	};
}

function recruitInteraction(): {
	value: Interaction;
	editReply: ReturnType<typeof vi.fn>;
	followUp: ReturnType<typeof vi.fn>;
	deleteReply: ReturnType<typeof vi.fn>;
} {
	const guild = {
		id: '200000000000000001',
		name: 'MSM Seoul',
		icon: null
	} as Guild;
	const editReply = vi.fn(async () => undefined);
	const followUp = vi.fn(async () => undefined);
	const deleteReply = vi.fn(async () => undefined);
	const interaction = {
		id: '400000000000000002',
		commandName: '내전모집',
		guildId: guild.id,
		guild,
		member: { nick: 'Longcat', roles: [] },
		user: {
			id: '100000000000000001',
			username: 'Longcat',
			globalName: 'Longcat',
			avatar: null
		},
		options: {
			getInteger: () => null,
			getBoolean: () => null
		},
		context: 0,
		authorizingIntegrationOwners: {},
		deferred: false,
		replied: false,
		ephemeral: false,
		inCachedGuild: () => true,
		isChatInputCommand: () => true,
		isButton: () => false,
		isStringSelectMenu: () => false,
		isRepliable: () => true,
		deferReply: vi.fn(async () => {
			interaction.deferred = true;
		}),
		editReply,
		followUp,
		deleteReply,
		reply: vi.fn(async () => undefined)
	} as unknown as ChatInputCommandInteraction & { deferred: boolean; ephemeral: boolean };

	return {
		value: interaction as Interaction,
		editReply,
		followUp,
		deleteReply
	};
}
