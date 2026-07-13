import {
	ChannelType,
	Events,
	PermissionFlagsBits,
	type Client,
	type DMChannel,
	type Guild,
	type GuildBasedChannel,
	type NonThreadGuildBasedChannel,
	type Role
} from 'discord.js';
import type { Logger } from 'pino';

import { MonkeyBotApiError, type MonkeyBotApiClient } from '../api/client.js';
import type {
	DiscordCatalogChannel,
	DiscordCatalogRole,
	DiscordCatalogSyncRequest
} from '../api/contracts.js';

const DEFAULT_DEBOUNCE_MS = 1_000;
const DEFAULT_FULL_SYNC_INTERVAL_MS = 60_000;

type CatalogApi = Pick<MonkeyBotApiClient, 'syncDiscordCatalog'>;

export interface GuildCatalogSync {
	syncGuild(guild: Guild): Promise<void>;
	syncAll(): Promise<void>;
	close(): void;
}

export interface GuildCatalogSyncOptions {
	debounceMs?: number;
	fullSyncIntervalMs?: number;
}

export function buildDiscordCatalog(guild: Guild): DiscordCatalogSyncRequest {
	const roles = [...guild.roles.cache.values()]
		.filter((role) => role.id !== guild.id)
		.map(mapRole)
		.sort(compareRoles);
	const botMember = guild.members.me;
	const channels = botMember
		? [...guild.channels.cache.values()]
				.map((channel) => mapChannel(channel, botMember))
				.filter((channel): channel is DiscordCatalogChannel => channel !== null)
				.sort(compareChannels)
		: [];

	return { roles, channels };
}

export function installGuildCatalogSync(
	client: Client,
	api: CatalogApi,
	logger: Logger,
	options: GuildCatalogSyncOptions = {}
): GuildCatalogSync {
	return new GuildCatalogSyncManager(client, api, logger, options);
}

class GuildCatalogSyncManager implements GuildCatalogSync {
	readonly #client: Client;
	readonly #api: CatalogApi;
	readonly #logger: Logger;
	readonly #debounceMs: number;
	readonly #fullSyncTimer: NodeJS.Timeout;
	readonly #debounceTimers = new Map<string, NodeJS.Timeout>();
	readonly #guildSyncs = new Map<string, Promise<void>>();
	readonly #rerunGuilds = new Set<string>();
	readonly #abortController = new AbortController();
	#fullSync: Promise<void> | null = null;
	#closed = false;

	readonly #onClientReady = () => {
		void this.syncAll();
	};
	readonly #onGuildCreate = (guild: Guild) => {
		this.#scheduleGuild(guild.id);
	};
	readonly #onGuildDelete = (guild: Guild) => {
		this.#clearGuildTimer(guild.id);
	};
	readonly #onChannelCreate = (channel: NonThreadGuildBasedChannel) => {
		this.#scheduleGuild(channel.guildId);
	};
	readonly #onChannelDelete = (channel: DMChannel | NonThreadGuildBasedChannel) => {
		const guildId = guildIdForChannel(channel);
		if (guildId) this.#scheduleGuild(guildId);
	};
	readonly #onChannelUpdate = (
		_oldChannel: DMChannel | NonThreadGuildBasedChannel,
		newChannel: DMChannel | NonThreadGuildBasedChannel
	) => {
		const guildId = guildIdForChannel(newChannel);
		if (guildId) this.#scheduleGuild(guildId);
	};
	readonly #onRoleCreate = (role: Role) => {
		this.#scheduleGuild(role.guild.id);
	};
	readonly #onRoleDelete = (role: Role) => {
		this.#scheduleGuild(role.guild.id);
	};
	readonly #onRoleUpdate = (_oldRole: Role, newRole: Role) => {
		this.#scheduleGuild(newRole.guild.id);
	};

	constructor(client: Client, api: CatalogApi, logger: Logger, options: GuildCatalogSyncOptions) {
		this.#client = client;
		this.#api = api;
		this.#logger = logger;
		this.#debounceMs = positiveDuration(options.debounceMs, DEFAULT_DEBOUNCE_MS, 'debounceMs');
		const fullSyncIntervalMs = positiveDuration(
			options.fullSyncIntervalMs,
			DEFAULT_FULL_SYNC_INTERVAL_MS,
			'fullSyncIntervalMs'
		);

		client.on(Events.ClientReady, this.#onClientReady);
		client.on(Events.GuildCreate, this.#onGuildCreate);
		client.on(Events.GuildDelete, this.#onGuildDelete);
		client.on(Events.ChannelCreate, this.#onChannelCreate);
		client.on(Events.ChannelDelete, this.#onChannelDelete);
		client.on(Events.ChannelUpdate, this.#onChannelUpdate);
		client.on(Events.GuildRoleCreate, this.#onRoleCreate);
		client.on(Events.GuildRoleDelete, this.#onRoleDelete);
		client.on(Events.GuildRoleUpdate, this.#onRoleUpdate);

		this.#fullSyncTimer = setInterval(() => void this.syncAll(), fullSyncIntervalMs);
		this.#fullSyncTimer.unref();
	}

	syncAll(): Promise<void> {
		if (this.#closed) return Promise.resolve();
		if (this.#fullSync) return this.#fullSync;

		const operation = Promise.all(
			[...this.#client.guilds.cache.values()].map((guild) => this.syncGuild(guild))
		)
			.then(() => undefined)
			.finally(() => {
				if (this.#fullSync === operation) this.#fullSync = null;
			});
		this.#fullSync = operation;
		return operation;
	}

	syncGuild(guild: Guild): Promise<void> {
		if (this.#closed) return Promise.resolve();
		const active = this.#guildSyncs.get(guild.id);
		if (active) {
			this.#rerunGuilds.add(guild.id);
			return active;
		}

		const operation = this.#runGuildQueue(guild).finally(() => {
			if (this.#guildSyncs.get(guild.id) === operation) this.#guildSyncs.delete(guild.id);
		});
		this.#guildSyncs.set(guild.id, operation);
		return operation;
	}

	close(): void {
		if (this.#closed) return;
		this.#closed = true;
		clearInterval(this.#fullSyncTimer);
		for (const timer of this.#debounceTimers.values()) clearTimeout(timer);
		this.#debounceTimers.clear();
		this.#rerunGuilds.clear();
		this.#abortController.abort(new Error('Discord catalog sync is shutting down'));

		this.#client.off(Events.ClientReady, this.#onClientReady);
		this.#client.off(Events.GuildCreate, this.#onGuildCreate);
		this.#client.off(Events.GuildDelete, this.#onGuildDelete);
		this.#client.off(Events.ChannelCreate, this.#onChannelCreate);
		this.#client.off(Events.ChannelDelete, this.#onChannelDelete);
		this.#client.off(Events.ChannelUpdate, this.#onChannelUpdate);
		this.#client.off(Events.GuildRoleCreate, this.#onRoleCreate);
		this.#client.off(Events.GuildRoleDelete, this.#onRoleDelete);
		this.#client.off(Events.GuildRoleUpdate, this.#onRoleUpdate);
	}

	async #runGuildQueue(initialGuild: Guild): Promise<void> {
		let guild: Guild | undefined = initialGuild;
		do {
			this.#rerunGuilds.delete(initialGuild.id);
			await this.#syncOnce(guild);
			guild = this.#client.guilds.cache.get(initialGuild.id);
		} while (!this.#closed && this.#rerunGuilds.delete(initialGuild.id) && guild);
	}

	async #syncOnce(guild: Guild): Promise<void> {
		const catalog = buildDiscordCatalog(guild);
		try {
			const result = await this.#api.syncDiscordCatalog(guild.id, catalog, {
				signal: this.#abortController.signal
			});
			this.#logger.debug(
				{
					guildId: guild.id,
					roleCount: catalog.roles.length,
					channelCount: catalog.channels.length,
					syncedAt: result.syncedAt
				},
				'Discord catalog synchronized'
			);
		} catch (error) {
			if (this.#closed && this.#abortController.signal.aborted) return;
			if (error instanceof MonkeyBotApiError && error.status === 404) {
				this.#logger.debug(
					{ guildId: guild.id, status: error.status, code: error.code },
					'Discord catalog ignored before guild setup'
				);
				return;
			}
			this.#logger.warn({ err: error, guildId: guild.id }, 'Discord catalog sync failed');
		}
	}

	#scheduleGuild(guildId: string): void {
		if (this.#closed) return;
		this.#clearGuildTimer(guildId);
		const timer = setTimeout(() => {
			this.#debounceTimers.delete(guildId);
			const guild = this.#client.guilds.cache.get(guildId);
			if (guild) void this.syncGuild(guild);
		}, this.#debounceMs);
		timer.unref();
		this.#debounceTimers.set(guildId, timer);
	}

	#clearGuildTimer(guildId: string): void {
		const timer = this.#debounceTimers.get(guildId);
		if (!timer) return;
		clearTimeout(timer);
		this.#debounceTimers.delete(guildId);
	}
}

function mapRole(role: Role): DiscordCatalogRole {
	return {
		id: role.id,
		name: role.name,
		position: role.position,
		managed: role.managed
	};
}

function mapChannel(
	channel: GuildBasedChannel,
	botMember: NonNullable<Guild['members']['me']>
): DiscordCatalogChannel | null {
	let kind: DiscordCatalogChannel['kind'];
	if (channel.type === ChannelType.GuildText) kind = 'TEXT';
	else if (channel.type === ChannelType.GuildAnnouncement) kind = 'ANNOUNCEMENT';
	else return null;

	const permissions = channel.permissionsFor(botMember);
	if (
		!permissions?.has([PermissionFlagsBits.ViewChannel, PermissionFlagsBits.SendMessages], true)
	) {
		return null;
	}

	return {
		id: channel.id,
		name: channel.name,
		parentName: channel.parent?.name ?? null,
		position: channel.rawPosition,
		kind
	};
}

function compareRoles(left: DiscordCatalogRole, right: DiscordCatalogRole): number {
	return right.position - left.position || compareId(left.id, right.id);
}

function compareChannels(left: DiscordCatalogChannel, right: DiscordCatalogChannel): number {
	return left.position - right.position || compareId(left.id, right.id);
}

function compareId(left: string, right: string): number {
	return left < right ? -1 : left > right ? 1 : 0;
}

function guildIdForChannel(channel: DMChannel | NonThreadGuildBasedChannel): string | null {
	return 'guildId' in channel ? channel.guildId : null;
}

function positiveDuration(value: number | undefined, fallback: number, name: string): number {
	const duration = value ?? fallback;
	if (!Number.isInteger(duration) || duration <= 0) throw new Error(`${name} must be positive`);
	return duration;
}
