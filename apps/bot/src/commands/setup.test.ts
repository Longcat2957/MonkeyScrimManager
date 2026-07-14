import {
	ApplicationCommandOptionType,
	ApplicationIntegrationType,
	ChannelType,
	InteractionContextType,
	type ChatInputCommandInteraction,
	type Guild
} from 'discord.js';
import type { Logger } from 'pino';
import { describe, expect, it, vi } from 'vitest';

import type { MonkeyBotApiClient } from '../api/client.js';
import type { BotConfig } from '../config.js';
import type { GuildCatalogSync } from '../discord/catalog-sync.js';
import type { InteractionContext } from '../interactions/context.js';
import { setupCommand } from './setup.js';

const guildId = '200000000000000001';
const operatorRoleId = '300000000000000001';
const interactionId = '400000000000000001';
const recruitmentChannelId = '500000000000000001';
const entryChannelId = '500000000000000002';
const resultChannelId = '500000000000000003';
const logChannelId = '500000000000000004';

type SetupChannel = {
	id: string;
	guildId: string | null;
	type: ChannelType;
	isDMBased: () => boolean;
	isTextBased: () => boolean;
	isSendable: () => boolean;
};

function channel(
	id: string,
	overrides: Partial<Pick<SetupChannel, 'guildId' | 'type'>> & {
		isDMBased?: boolean;
		isTextBased?: boolean;
		isSendable?: boolean;
	} = {}
): SetupChannel {
	return {
		id,
		guildId: overrides.guildId ?? guildId,
		type: overrides.type ?? ChannelType.GuildText,
		isDMBased: () => overrides.isDMBased ?? false,
		isTextBased: () => overrides.isTextBased ?? true,
		isSendable: () => overrides.isSendable ?? true
	};
}

function setupInteraction(
	options: {
		role?: { id: string; managed: boolean };
		guildName?: string | null;
		channels?: Record<string, SetupChannel | null | undefined>;
	} = {}
) {
	const guild = { id: guildId, name: 'MSM Seoul', icon: null } as Guild;
	const editReply = vi.fn(async () => undefined);
	const getChannel = vi.fn((name: string) => options.channels?.[name] ?? null);
	return {
		value: {
			id: interactionId,
			guildId,
			guild,
			member: { nick: 'Longcat', roles: [operatorRoleId] },
			user: {
				id: '100000000000000001',
				username: 'Longcat',
				globalName: 'Longcat',
				avatar: null
			},
			options: {
				getRole: () => options.role ?? { id: operatorRoleId, managed: false },
				getString: () => options.guildName ?? null,
				getChannel
			},
			inCachedGuild: () => true,
			deferReply: vi.fn(async () => undefined),
			editReply,
			reply: vi.fn(async () => undefined)
		} as unknown as ChatInputCommandInteraction,
		guild,
		editReply,
		getChannel
	};
}

function context(
	setupGuild: ReturnType<typeof vi.fn>,
	syncGuild: ReturnType<typeof vi.fn>,
	warn = vi.fn()
): InteractionContext {
	return {
		api: { setupGuild } as unknown as MonkeyBotApiClient,
		catalogSync: { syncGuild } as unknown as GuildCatalogSync,
		config: { webOrigin: new URL('http://localhost:5173') } as BotConfig,
		logger: { warn } as unknown as Logger
	};
}

function setupResult() {
	return {
		guild: {
			id: guildId,
			displayName: 'MSM Seoul',
			operatorRoleId,
			currentSeason: { id: '01900000-0000-7000-8000-000000000201', name: '시즌 1' },
			dashboardHref: `/g/${guildId}`
		}
	};
}

describe('/초기설정', () => {
	it('defines one required recruitment channel and three optional postable channels', () => {
		const definition = setupCommand.data.toJSON();

		expect(definition).toMatchObject({
			name: '초기설정',
			integration_types: [ApplicationIntegrationType.GuildInstall],
			contexts: [InteractionContextType.Guild]
		});
		expect(definition.options).toEqual([
			expect.objectContaining({
				name: '운영자역할',
				type: ApplicationCommandOptionType.Role,
				required: true
			}),
			expect.objectContaining({
				name: '모집채널',
				type: ApplicationCommandOptionType.Channel,
				required: true,
				channel_types: [ChannelType.GuildText, ChannelType.GuildAnnouncement]
			}),
			expect.objectContaining({
				name: '길드이름',
				type: ApplicationCommandOptionType.String,
				required: false
			}),
			...['엔트리채널', '결과알림채널', '운영로그채널'].map((name) =>
				expect.objectContaining({
					name,
					type: ApplicationCommandOptionType.Channel,
					required: false,
					channel_types: [ChannelType.GuildText, ChannelType.GuildAnnouncement]
				})
			)
		]);
	});

	it('sends the required recruitment channel and null optional channels before catalog sync', async () => {
		const setupGuild = vi.fn(async () => setupResult());
		const syncGuild = vi.fn(async () => {
			throw new Error('catalog unavailable');
		});
		const warn = vi.fn();
		const current = setupInteraction({
			channels: { 모집채널: channel(recruitmentChannelId) }
		});

		await expect(
			setupCommand.execute(current.value, context(setupGuild, syncGuild, warn))
		).resolves.toBeUndefined();

		expect(setupGuild).toHaveBeenCalledWith(
			guildId,
			{
				actor: {
					id: '100000000000000001',
					globalDisplayName: 'Longcat',
					displayName: 'Longcat',
					avatarHash: null,
					roleIds: [operatorRoleId]
				},
				guildName: 'MSM Seoul',
				guildIconHash: null,
				operatorRoleId,
				recruitmentChannelId,
				entryChannelId: null,
				resultChannelId: null,
				logChannelId: null,
				seasonName: '시즌 1'
			},
			{ idempotencyKey: interactionId }
		);
		expect(syncGuild).toHaveBeenCalledWith(current.guild);
		expect(setupGuild.mock.invocationCallOrder[0]).toBeLessThan(
			syncGuild.mock.invocationCallOrder[0]!
		);
		expect(warn).toHaveBeenCalledWith(
			expect.objectContaining({ guildId }),
			'Discord catalog sync after guild setup failed'
		);
		expect(current.editReply).toHaveBeenCalledWith(
			`초기 설정을 완료했습니다. 길드 운영 화면: http://localhost:5173/g/${guildId}`
		);
	});

	it('sends every optional channel when the operator selects them', async () => {
		const setupGuild = vi.fn(async () => setupResult());
		const syncGuild = vi.fn(async () => undefined);
		const current = setupInteraction({
			channels: {
				모집채널: channel(recruitmentChannelId),
				엔트리채널: channel(entryChannelId),
				결과알림채널: channel(resultChannelId, { type: ChannelType.GuildAnnouncement }),
				운영로그채널: channel(logChannelId)
			}
		});

		await setupCommand.execute(current.value, context(setupGuild, syncGuild));

		expect(setupGuild).toHaveBeenCalledWith(
			guildId,
			expect.objectContaining({
				recruitmentChannelId,
				entryChannelId,
				resultChannelId,
				logChannelId
			}),
			expect.anything()
		);
	});

	it.each([
		{
			name: 'a missing required recruitment channel',
			channels: {},
			message: '모집 채널을 선택해야 합니다.'
		},
		{
			name: 'a channel from another guild',
			channels: { 모집채널: channel(recruitmentChannelId, { guildId: '299999999999999999' }) },
			message:
				'선택한 모집 채널은 현재 서버에서 Bot이 메시지를 보낼 수 있는 텍스트 또는 공지 채널이어야 합니다.'
		},
		{
			name: 'a non-sendable channel',
			channels: { 모집채널: channel(recruitmentChannelId, { isSendable: false }) },
			message:
				'선택한 모집 채널은 현재 서버에서 Bot이 메시지를 보낼 수 있는 텍스트 또는 공지 채널이어야 합니다.'
		},
		{
			name: 'a DM channel',
			channels: {
				모집채널: channel(recruitmentChannelId, {
					guildId: null,
					isDMBased: true
				})
			},
			message:
				'선택한 모집 채널은 현재 서버에서 Bot이 메시지를 보낼 수 있는 텍스트 또는 공지 채널이어야 합니다.'
		}
	])('rejects $name before calling the API', async ({ channels, message }) => {
		const setupGuild = vi.fn();
		const syncGuild = vi.fn();
		const current = setupInteraction({ channels });

		await setupCommand.execute(current.value, context(setupGuild, syncGuild));

		expect(current.editReply).toHaveBeenCalledWith(message);
		expect(setupGuild).not.toHaveBeenCalled();
		expect(syncGuild).not.toHaveBeenCalled();
	});

	it.each([
		{
			name: '@everyone',
			role: { id: guildId, managed: false },
			message: '@everyone 역할은 운영자 역할로 사용할 수 없습니다.'
		},
		{
			name: 'managed/integration',
			role: { id: '300000000000000002', managed: true },
			message: 'Discord 또는 연동 서비스가 관리하는 역할은 운영자 역할로 사용할 수 없습니다.'
		}
	])('$name 역할을 API 호출 전에 거절한다', async ({ role, message }) => {
		const setupGuild = vi.fn();
		const syncGuild = vi.fn();
		const current = setupInteraction({ role });

		await setupCommand.execute(current.value, context(setupGuild, syncGuild));

		expect(current.editReply).toHaveBeenCalledWith(message);
		expect(setupGuild).not.toHaveBeenCalled();
		expect(syncGuild).not.toHaveBeenCalled();
	});
});
