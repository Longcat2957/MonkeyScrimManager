import { integer, sqliteTable, text } from 'drizzle-orm/sqlite-core';

export const guilds = sqliteTable('guilds', {
	guildId: text('guild_id').primaryKey(),
	name: text('name').notNull(),
	iconUrl: text('icon_url'),
	status: text('status', { enum: ['INSTALLED', 'SETUP_REQUIRED', 'ACTIVE', 'DISABLED', 'LEFT'] })
		.notNull()
		.default('INSTALLED'),
	installedAt: integer('installed_at', { mode: 'timestamp_ms' }).notNull(),
	activatedAt: integer('activated_at', { mode: 'timestamp_ms' }),
	leftAt: integer('left_at', { mode: 'timestamp_ms' }),
	createdBy: text('created_by')
});

export const users = sqliteTable('users', {
	discordId: text('discord_id').primaryKey(),
	displayName: text('display_name').notNull(),
	deletedAt: integer('deleted_at', { mode: 'timestamp_ms' })
});

export const recruitments = sqliteTable('recruitments', {
	id: text('id').primaryKey(),
	guildId: text('guild_id')
		.notNull()
		.references(() => guilds.guildId),
	status: text('status', { enum: ['OPEN', 'CLOSED', 'CANCELLED'] }).notNull(),
	channelId: text('channel_id'),
	messageId: text('message_id'),
	createdAt: integer('created_at', { mode: 'timestamp_ms' }).notNull(),
	updatedAt: integer('updated_at', { mode: 'timestamp_ms' }).notNull()
});

export const series = sqliteTable('series', {
	id: text('id').primaryKey(),
	guildId: text('guild_id')
		.notNull()
		.references(() => guilds.guildId),
	status: text('status', { enum: ['DRAFT', 'PICK_BAN', 'RESULT_PENDING', 'COMPLETED'] }).notNull(),
	winningTeam: text('winning_team', { enum: ['BLUE', 'RED'] }),
	createdAt: integer('created_at', { mode: 'timestamp_ms' }).notNull(),
	updatedAt: integer('updated_at', { mode: 'timestamp_ms' }).notNull()
});
