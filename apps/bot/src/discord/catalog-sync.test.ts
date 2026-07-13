import { EventEmitter } from 'node:events';

import {
	ChannelType,
	Collection,
	Events,
	type Client,
	type Guild,
	type NonThreadGuildBasedChannel
} from 'discord.js';
import type { Logger } from 'pino';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { MonkeyBotApiError, type MonkeyBotApiClient } from '../api/client.js';
import { buildDiscordCatalog, installGuildCatalogSync } from './catalog-sync.js';

const guildId = '200000000000000001';
const syncedAt = '2026-07-13T12:34:56.000Z';

describe('Discord catalog projection', () => {
	it('keeps managed roles and only message-capable text channels in stable order', () => {
		const guild = createGuild({
			roles: [
				role(guildId, '@everyone', 0, false),
				role('300000000000000003', '낮은 역할', 1, false),
				role('300000000000000002', '연동 역할', 5, true),
				role('300000000000000001', '운영자', 5, false)
			],
			channels: [
				channel('500000000000000005', '음성', ChannelType.GuildVoice, 0, true),
				channel('500000000000000004', '쓰기-불가', ChannelType.GuildText, 1, false),
				channel('500000000000000003', '공지', ChannelType.GuildAnnouncement, 3, true),
				channel('500000000000000002', '모집-b', ChannelType.GuildText, 2, true),
				channel('500000000000000001', '모집-a', ChannelType.GuildText, 2, true, '내전')
			]
		});

		expect(buildDiscordCatalog(guild)).toEqual({
			roles: [
				{ id: '300000000000000001', name: '운영자', position: 5, managed: false },
				{ id: '300000000000000002', name: '연동 역할', position: 5, managed: true },
				{ id: '300000000000000003', name: '낮은 역할', position: 1, managed: false }
			],
			channels: [
				{
					id: '500000000000000001',
					name: '모집-a',
					parentName: '내전',
					position: 2,
					kind: 'TEXT'
				},
				{
					id: '500000000000000002',
					name: '모집-b',
					parentName: null,
					position: 2,
					kind: 'TEXT'
				},
				{
					id: '500000000000000003',
					name: '공지',
					parentName: null,
					position: 3,
					kind: 'ANNOUNCEMENT'
				}
			]
		});
	});

	it('returns no channels when the bot member is unavailable', () => {
		const guild = createGuild({
			roles: [role('300000000000000001', '운영자', 1, false)],
			channels: [channel('500000000000000001', '모집', ChannelType.GuildText, 1, true)],
			botMember: null
		});

		expect(buildDiscordCatalog(guild).channels).toEqual([]);
	});
});

describe('GuildCatalogSync', () => {
	afterEach(() => {
		vi.useRealTimers();
	});

	it('syncs on ready, debounces guild events, repeats periodically, and stops cleanly', async () => {
		vi.useFakeTimers();
		const guild = createGuild();
		const client = createClient(guild);
		const syncDiscordCatalog = vi.fn(async () => ({ syncedAt }));
		const { logger } = createLogger();
		const manager = installGuildCatalogSync(
			client as unknown as Client,
			{ syncDiscordCatalog } as Pick<MonkeyBotApiClient, 'syncDiscordCatalog'>,
			logger,
			{ debounceMs: 100, fullSyncIntervalMs: 1_000 }
		);

		client.emit(Events.ClientReady, client);
		await vi.advanceTimersByTimeAsync(0);
		expect(syncDiscordCatalog).toHaveBeenCalledTimes(1);

		const changedChannel = channel('500000000000000001', '모집', ChannelType.GuildText, 1, true);
		client.emit(Events.ChannelCreate, changedChannel);
		client.emit(Events.ChannelUpdate, changedChannel, changedChannel);
		client.emit(Events.ChannelDelete, changedChannel);
		await vi.advanceTimersByTimeAsync(99);
		expect(syncDiscordCatalog).toHaveBeenCalledTimes(1);
		await vi.advanceTimersByTimeAsync(1);
		expect(syncDiscordCatalog).toHaveBeenCalledTimes(2);

		await vi.advanceTimersByTimeAsync(900);
		expect(syncDiscordCatalog).toHaveBeenCalledTimes(3);

		manager.close();
		client.emit(Events.ChannelCreate, changedChannel);
		await vi.advanceTimersByTimeAsync(2_000);
		expect(syncDiscordCatalog).toHaveBeenCalledTimes(3);
	});

	it('coalesces overlapping full sync passes', async () => {
		vi.useFakeTimers();
		const guild = createGuild();
		const client = createClient(guild);
		let release!: () => void;
		const pending = new Promise<void>((resolve) => {
			release = resolve;
		});
		const syncDiscordCatalog = vi.fn(async () => {
			await pending;
			return { syncedAt };
		});
		const { logger } = createLogger();
		const manager = installGuildCatalogSync(
			client as unknown as Client,
			{ syncDiscordCatalog } as Pick<MonkeyBotApiClient, 'syncDiscordCatalog'>,
			logger,
			{ fullSyncIntervalMs: 1_000 }
		);

		const first = manager.syncAll();
		const second = manager.syncAll();
		expect(second).toBe(first);
		await vi.advanceTimersByTimeAsync(1_000);
		expect(syncDiscordCatalog).toHaveBeenCalledTimes(1);

		release();
		await first;
		manager.close();
	});

	it('debounces every guild, channel, and role catalog event', async () => {
		vi.useFakeTimers();
		const guild = createGuild();
		const client = createClient(guild);
		const syncDiscordCatalog = vi.fn(async () => ({ syncedAt }));
		const { logger } = createLogger();
		const manager = installGuildCatalogSync(
			client as unknown as Client,
			{ syncDiscordCatalog } as Pick<MonkeyBotApiClient, 'syncDiscordCatalog'>,
			logger,
			{ debounceMs: 10, fullSyncIntervalMs: 10_000 }
		);
		const changedChannel = channel('500000000000000001', '모집', ChannelType.GuildText, 1, true);
		const changedRole = role('300000000000000001', '운영자', 1, false);
		const events: Array<() => void> = [
			() => client.emit(Events.GuildCreate, guild),
			() => client.emit(Events.ChannelCreate, changedChannel),
			() => client.emit(Events.ChannelUpdate, changedChannel, changedChannel),
			() => client.emit(Events.ChannelDelete, changedChannel),
			() => client.emit(Events.GuildRoleCreate, changedRole),
			() => client.emit(Events.GuildRoleUpdate, changedRole, changedRole),
			() => client.emit(Events.GuildRoleDelete, changedRole)
		];

		for (const [index, emit] of events.entries()) {
			emit();
			await vi.advanceTimersByTimeAsync(10);
			expect(syncDiscordCatalog).toHaveBeenCalledTimes(index + 1);
		}
		manager.close();
	});

	it('serializes a requested rerun behind an active guild sync', async () => {
		const guild = createGuild();
		const client = createClient(guild);
		let releaseFirst!: () => void;
		const firstPending = new Promise<void>((resolve) => {
			releaseFirst = resolve;
		});
		let activeCalls = 0;
		let maximumActiveCalls = 0;
		const syncDiscordCatalog = vi.fn(async () => {
			activeCalls += 1;
			maximumActiveCalls = Math.max(maximumActiveCalls, activeCalls);
			if (syncDiscordCatalog.mock.calls.length === 1) await firstPending;
			activeCalls -= 1;
			return { syncedAt };
		});
		const { logger } = createLogger();
		const manager = installGuildCatalogSync(
			client as unknown as Client,
			{ syncDiscordCatalog } as Pick<MonkeyBotApiClient, 'syncDiscordCatalog'>,
			logger
		);

		const first = manager.syncGuild(guild);
		const queued = manager.syncGuild(guild);
		expect(queued).toBe(first);
		expect(syncDiscordCatalog).toHaveBeenCalledTimes(1);
		releaseFirst();
		await first;

		expect(syncDiscordCatalog).toHaveBeenCalledTimes(2);
		expect(maximumActiveCalls).toBe(1);
		manager.close();
	});

	it('ignores pre-setup 404 at debug level and warns for other failures', async () => {
		const guild = createGuild();
		const client = createClient(guild);
		const syncDiscordCatalog = vi
			.fn()
			.mockRejectedValueOnce(
				new MonkeyBotApiError({ status: 404, code: 'not_found', message: 'not found' })
			)
			.mockRejectedValueOnce(
				new MonkeyBotApiError({ status: 503, code: 'not_ready', message: 'not ready' })
			);
		const { debug, logger, warn } = createLogger();
		const manager = installGuildCatalogSync(
			client as unknown as Client,
			{ syncDiscordCatalog } as Pick<MonkeyBotApiClient, 'syncDiscordCatalog'>,
			logger
		);

		await manager.syncGuild(guild);
		expect(debug).toHaveBeenCalledWith(
			expect.objectContaining({ guildId, status: 404 }),
			'Discord catalog ignored before guild setup'
		);
		expect(warn).not.toHaveBeenCalled();

		await manager.syncGuild(guild);
		expect(warn).toHaveBeenCalledWith(
			expect.objectContaining({ guildId }),
			'Discord catalog sync failed'
		);
		manager.close();
	});
});

function createGuild(
	options: {
		roles?: unknown[];
		channels?: unknown[];
		botMember?: object | null;
	} = {}
): Guild {
	const botMember =
		options.botMember === undefined ? { id: '100000000000000001' } : options.botMember;
	return {
		id: guildId,
		roles: { cache: collection(options.roles ?? []) },
		channels: { cache: collection(options.channels ?? []) },
		members: { me: botMember }
	} as unknown as Guild;
}

function role(id: string, name: string, position: number, managed: boolean) {
	return { id, name, position, managed, guild: { id: guildId } };
}

function channel(
	id: string,
	name: string,
	type: ChannelType,
	rawPosition: number,
	allowed: boolean,
	parentName: string | null = null
) {
	return {
		id,
		guildId,
		name,
		type,
		rawPosition,
		parent: parentName ? { name: parentName } : null,
		permissionsFor: () => ({ has: () => allowed })
	} as unknown as NonThreadGuildBasedChannel;
}

function collection(values: unknown[]) {
	return new Collection(values.map((value) => [(value as { id: string }).id, value] as const));
}

function createClient(guild: Guild) {
	const client = new EventEmitter() as EventEmitter & {
		guilds: { cache: Collection<string, Guild> };
	};
	client.guilds = { cache: new Collection([[guild.id, guild]]) };
	return client;
}

function createLogger() {
	const debug = vi.fn();
	const warn = vi.fn();
	return {
		debug,
		warn,
		logger: { debug, warn } as unknown as Logger
	};
}
