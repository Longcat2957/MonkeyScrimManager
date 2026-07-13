import type { ChatInputCommandInteraction, Guild } from 'discord.js';
import type { Logger } from 'pino';
import { describe, expect, it, vi } from 'vitest';

import type { MonkeyBotApiClient } from '../api/client.js';
import type { BotConfig } from '../config.js';
import type { GuildCatalogSync } from '../discord/catalog-sync.js';
import type { InteractionContext } from '../interactions/context.js';
import { setupCommand } from './setup.js';

describe('/초기설정', () => {
	it('tries an immediate catalog sync without turning a sync failure into setup failure', async () => {
		const guild = { id: '200000000000000001', name: 'MSM Seoul', icon: null } as Guild;
		const setupGuild = vi.fn(async () => ({
			guild: {
				id: guild.id,
				displayName: guild.name,
				operatorRoleId: '300000000000000001',
				currentSeason: { id: '01900000-0000-7000-8000-000000000201', name: '시즌 1' },
				dashboardHref: `/g/${guild.id}`
			}
		}));
		const syncGuild = vi.fn(async () => {
			throw new Error('catalog unavailable');
		});
		const warn = vi.fn();
		const editReply = vi.fn(async () => undefined);
		const interaction = {
			id: '400000000000000001',
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
				getString: () => null
			},
			inCachedGuild: () => true,
			deferReply: vi.fn(async () => undefined),
			editReply,
			reply: vi.fn(async () => undefined)
		} as unknown as ChatInputCommandInteraction;
		const context = {
			api: { setupGuild } as unknown as MonkeyBotApiClient,
			catalogSync: { syncGuild } as unknown as GuildCatalogSync,
			config: { webOrigin: new URL('http://localhost:5173') } as BotConfig,
			logger: { warn } as unknown as Logger
		} satisfies InteractionContext;

		await expect(setupCommand.execute(interaction, context)).resolves.toBeUndefined();
		expect(setupGuild).toHaveBeenCalledOnce();
		expect(syncGuild).toHaveBeenCalledWith(guild);
		expect(setupGuild.mock.invocationCallOrder[0]).toBeLessThan(
			syncGuild.mock.invocationCallOrder[0]!
		);
		expect(warn).toHaveBeenCalledWith(
			expect.objectContaining({ guildId: guild.id }),
			'Discord catalog sync after guild setup failed'
		);
		expect(editReply).toHaveBeenCalledWith(
			`초기 설정을 완료했습니다. 길드 운영 화면: http://localhost:5173/g/${guild.id}`
		);
	});

	it.each([
		{
			name: '@everyone',
			role: { id: '200000000000000001', managed: false },
			message: '@everyone 역할은 운영자 역할로 사용할 수 없습니다.'
		},
		{
			name: 'managed/integration',
			role: { id: '300000000000000002', managed: true },
			message: 'Discord 또는 연동 서비스가 관리하는 역할은 운영자 역할로 사용할 수 없습니다.'
		}
	])('$name 역할을 API 호출 전에 거절한다', async ({ role, message }) => {
		const guild = {
			id: '200000000000000001',
			name: 'MSM Seoul',
			icon: null
		} as Guild;
		const setupGuild = vi.fn();
		const syncGuild = vi.fn();
		const editReply = vi.fn(async () => undefined);
		const interaction = {
			id: '400000000000000001',
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
				getRole: () => role,
				getString: () => null
			},
			inCachedGuild: () => true,
			deferReply: vi.fn(async () => undefined),
			editReply,
			reply: vi.fn(async () => undefined)
		} as unknown as ChatInputCommandInteraction;
		const context = {
			api: { setupGuild } as unknown as MonkeyBotApiClient,
			catalogSync: { syncGuild } as unknown as GuildCatalogSync,
			config: { webOrigin: new URL('http://localhost:5173') } as BotConfig,
			logger: { warn: vi.fn() } as unknown as Logger
		} satisfies InteractionContext;

		await expect(setupCommand.execute(interaction, context)).resolves.toBeUndefined();
		expect(editReply).toHaveBeenCalledWith(message);
		expect(setupGuild).not.toHaveBeenCalled();
		expect(syncGuild).not.toHaveBeenCalled();
	});
});
