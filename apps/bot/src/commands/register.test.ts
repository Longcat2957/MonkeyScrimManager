import {
	ApplicationCommandOptionType,
	ApplicationIntegrationType,
	ButtonStyle,
	InteractionContextType,
	type ChatInputCommandInteraction
} from 'discord.js';
import type { Logger } from 'pino';
import { describe, expect, it, vi } from 'vitest';

import {
	MonkeyBotApiClient,
	MonkeyBotApiContractError,
	MonkeyBotApiError,
	MonkeyBotApiTransportError
} from '../api/client.js';
import type { BotConfig } from '../config.js';
import type { GuildCatalogSync } from '../discord/catalog-sync.js';
import type { InteractionContext } from '../interactions/context.js';
import { parseRiotId, registerCommand } from './register.js';

const guildId = '200000000000000001';
const userId = '100000000000000001';
const interactionId = '400000000000000001';
const apiRequestId = '01900000-0000-7000-8000-000000000001';

function context(registerGuildMember: ReturnType<typeof vi.fn>): InteractionContext {
	return {
		api: { registerGuildMember } as unknown as MonkeyBotApiClient,
		catalogSync: {} as GuildCatalogSync,
		config: { webOrigin: new URL('http://localhost:5173') } as BotConfig,
		logger: { warn: vi.fn() } as unknown as Logger
	};
}

function interaction(riotId = 'Longcat#KR1') {
	const reply = vi.fn(async () => undefined);
	const editReply = vi.fn(async (response: unknown) => {
		void response;
	});
	return {
		value: {
			id: interactionId,
			guildId,
			guild: {},
			member: { nick: 'MSM Longcat', roles: ['300000000000000001'] },
			user: {
				id: userId,
				username: 'longcat_login',
				globalName: 'Longcat',
				avatar: null
			},
			options: { getString: () => riotId },
			inCachedGuild: () => true,
			deferReply: vi.fn(async () => undefined),
			reply,
			editReply
		} as unknown as ChatInputCommandInteraction,
		reply,
		editReply
	};
}

const created = {
	member: { id: userId, displayName: 'MSM Longcat', status: 'ACTIVE' as const },
	riotAccount: {
		puuid: 'riot-puuid',
		gameName: 'Longcat',
		tagLine: 'KR1',
		platform: 'KR' as const,
		isMain: true
	},
	registration: 'CREATED' as const,
	accountLink: 'CREATED' as const
};

describe('/등록', () => {
	it('is a guild-only command with one required Riot ID option', () => {
		const definition = registerCommand.data.toJSON();

		expect(definition).toMatchObject({
			name: '등록',
			integration_types: [ApplicationIntegrationType.GuildInstall],
			contexts: [InteractionContextType.Guild]
		});
		expect(definition.options).toEqual([
			expect.objectContaining({
				name: 'riot_id',
				type: ApplicationCommandOptionType.String,
				required: true
			})
		]);
	});

	it('registers the invoking Discord user and its current guild membership', async () => {
		const registerGuildMember = vi.fn(async () => created);
		const current = interaction(' Longcat # KR1 ');

		await registerCommand.execute(current.value, context(registerGuildMember));

		expect(registerGuildMember).toHaveBeenCalledWith(
			guildId,
			{
				actor: {
					id: userId,
					globalDisplayName: 'Longcat',
					displayName: 'MSM Longcat',
					avatarHash: null,
					roleIds: ['300000000000000001']
				},
				gameName: 'Longcat',
				tagLine: 'KR1',
				platform: 'KR'
			},
			{ idempotencyKey: interactionId }
		);
		const reply = serializedReply(current.editReply);
		expect(reply.embeds).toEqual([
			expect.objectContaining({
				title: '계정 등록 완료',
				fields: [
					{ name: 'Riot ID', value: 'Longcat#KR1', inline: true },
					{ name: '길드 소속', value: '등록 완료', inline: true }
				]
			})
		]);
		expect(reply.components).toEqual([
			{
				type: 1,
				components: [
					{
						type: 2,
						style: ButtonStyle.Link,
						label: '내 정보',
						url: 'http://localhost:5173/me'
					}
				]
			}
		]);
	});

	it('reports a fully idempotent registration as already registered', async () => {
		const registerGuildMember = vi.fn(async () => ({
			...created,
			registration: 'EXISTING' as const,
			accountLink: 'EXISTING' as const
		}));
		const current = interaction();

		await registerCommand.execute(current.value, context(registerGuildMember));

		expect(serializedReply(current.editReply).embeds[0]?.title).toBe('이미 등록된 계정입니다');
	});

	it.each(['Longcat', '#KR1', 'Longcat#', 'Long#cat#KR1'])(
		'rejects malformed Riot ID %s before calling the API',
		async (riotId) => {
			const registerGuildMember = vi.fn();
			const current = interaction(riotId);

			await registerCommand.execute(current.value, context(registerGuildMember));

			expect(registerGuildMember).not.toHaveBeenCalled();
			expect(current.reply).toHaveBeenCalledWith(
				expect.objectContaining({
					content: 'Riot ID 형식이 올바르지 않습니다. `GameName#TagLine` 형식으로 입력해 주세요.'
				})
			);
		}
	);

	it.each([
		['member_inactive', '현재 길드에서 비활성화된 사용자입니다. 길드 운영자에게 문의해 주세요.'],
		['guild_inactive', '현재 길드에서는 계정을 등록할 수 없습니다. 길드 운영자에게 문의해 주세요.'],
		[
			'riot_account_already_linked',
			'이 Riot 계정은 이미 다른 Discord 사용자에게 연결되어 있습니다.'
		],
		[
			'riot_account_not_found',
			'입력한 Riot 계정을 찾을 수 없습니다. Riot ID와 태그를 확인해 주세요.'
		],
		[
			'riot_unavailable',
			'Riot 계정 조회 서비스를 일시적으로 사용할 수 없습니다. 잠시 후 다시 시도해 주세요.'
		]
	])('maps API error %s to a private Korean response', async (code, message) => {
		const registerGuildMember = vi.fn(async () => {
			throw new MonkeyBotApiError({
				status: 409,
				code,
				message: 'backend detail',
				requestId: apiRequestId,
				details: { sql: 'must stay private' }
			});
		});
		const current = interaction();

		await registerCommand.execute(current.value, context(registerGuildMember));

		expect(current.editReply).toHaveBeenCalledWith(
			[message, `오류 코드: \`${code}\``, `요청 ID: \`API:${apiRequestId}\``].join('\n')
		);
		expect(current.editReply.mock.calls[0]?.[0]).not.toContain('must stay private');
	});

	it('preserves an untailored public backend reason', async () => {
		const registerGuildMember = vi.fn(async () => {
			throw new MonkeyBotApiError({
				status: 404,
				code: 'not_found',
				message: '현재 Discord 길드를 찾을 수 없습니다.',
				requestId: apiRequestId
			});
		});
		const current = interaction();

		await registerCommand.execute(current.value, context(registerGuildMember));

		expect(current.editReply).toHaveBeenCalledWith(
			[
				'현재 Discord 길드를 찾을 수 없습니다.',
				'오류 코드: `not_found`',
				`요청 ID: \`API:${apiRequestId}\``
			].join('\n')
		);
	});

	it('reports transport failures without exposing backend details', async () => {
		const registerGuildMember = vi.fn(async () => {
			throw new MonkeyBotApiTransportError('connection refused');
		});
		const current = interaction();

		await registerCommand.execute(current.value, context(registerGuildMember));

		expect(current.editReply).toHaveBeenCalledWith(
			[
				'MonkeyBot API에 연결할 수 없습니다. 잠시 후 다시 시도해 주세요.',
				'오류 코드: `bot_api_transport_error`',
				`요청 ID: \`Discord:${interactionId}\``
			].join('\n')
		);
		expect(current.editReply.mock.calls[0]?.[0]).not.toContain('connection refused');
	});

	it.each([
		{
			error: new MonkeyBotApiContractError('raw response body'),
			reason: 'MonkeyBot API 응답 형식을 확인하지 못했습니다.',
			code: 'bot_api_contract_error',
			privateDetail: 'raw response body'
		},
		{
			error: new Error('stack with database password'),
			reason: '예기치 않은 내부 오류가 발생했습니다.',
			code: 'interaction_failed',
			privateDetail: 'database password'
		}
	])('reports $code with a safe reason and interaction ID', async (testCase) => {
		const registerGuildMember = vi.fn(async () => {
			throw testCase.error;
		});
		const current = interaction();

		await registerCommand.execute(current.value, context(registerGuildMember));

		expect(current.editReply).toHaveBeenCalledWith(
			[
				testCase.reason,
				`오류 코드: \`${testCase.code}\``,
				`요청 ID: \`Discord:${interactionId}\``
			].join('\n')
		);
		expect(current.editReply.mock.calls[0]?.[0]).not.toContain(testCase.privateDetail);
	});
});

describe('parseRiotId', () => {
	it('parses exactly one separator and trims both components', () => {
		expect(parseRiotId(' Longcat # KR1 ')).toEqual({ gameName: 'Longcat', tagLine: 'KR1' });
	});
});

interface SerializedReply {
	embeds: Array<{
		title: string;
		fields: Array<{ name: string; value: string; inline: boolean }>;
	}>;
	components: Array<{
		type: number;
		components: Array<{
			type: number;
			style: number;
			label: string;
			url: string;
		}>;
	}>;
}

function serializedReply(editReply: ReturnType<typeof vi.fn>): SerializedReply {
	const payload = editReply.mock.calls[0]?.[0];
	return JSON.parse(JSON.stringify(payload)) as SerializedReply;
}
