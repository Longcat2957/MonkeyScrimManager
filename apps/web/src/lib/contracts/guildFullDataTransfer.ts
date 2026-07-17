import { z } from 'zod';
import { responseMetaSchema } from '$lib/contracts/workflow';

const snowflakeSchema = z.string().regex(/^[1-9][0-9]{0,19}$/);
const timestampSchema = z.iso.datetime({ offset: true });
const nonnegativeInteger = z.number().int().nonnegative();
const digestSchema = z.string().regex(/^[a-f0-9]{64}$/);
const archiveRowSchema = z.record(z.string(), z.unknown());
const archiveRowsSchema = z.array(archiveRowSchema);

export const guildFullSnapshotArchiveKind = 'GUILD_FULL_SNAPSHOT' as const;
export const guildFullSnapshotArchiveSchemaVersion = 1 as const;
export const guildFullSnapshotMaxRows = 250_000;
export const guildFullSnapshotMaxBytes = 16 * 1024 * 1024;

const archiveFileNameSchema = z
	.string()
	.min(1)
	.max(255)
	.regex(/^[A-Za-z0-9][A-Za-z0-9._-]*\.json$/, 'Archive file name must be a single JSON file name');

export const guildFullSnapshotSourceSchema = z
	.object({
		guildId: snowflakeSchema,
		guildName: z.string().trim().min(1).max(100)
	})
	.strict();

export const guildFullSnapshotTablesSchema = z
	.object({
		users: archiveRowsSchema,
		user_discord_identities: archiveRowsSchema,
		user_privacy_settings: archiveRowsSchema,
		user_champion_preferences: archiveRowsSchema,
		riot_accounts: archiveRowsSchema,
		dummy_user_profiles: archiveRowsSchema,
		guild_members: archiveRowsSchema,
		user_guild_preferences: archiveRowsSchema,
		guild_counters: archiveRowsSchema,
		seasons: archiveRowsSchema,
		recruitments: archiveRowsSchema,
		recruitment_participants: archiveRowsSchema,
		recruitment_preferred_lanes: archiveRowsSchema,
		recruitment_entry_drafts: archiveRowsSchema,
		recruitment_entry_assignments: archiveRowsSchema,
		series: archiveRowsSchema,
		series_participants: archiveRowsSchema,
		auction_recruitments: archiveRowsSchema,
		auction_recruitment_participants: archiveRowsSchema,
		auction_setup_drafts: archiveRowsSchema,
		auction_setup_teams: archiveRowsSchema,
		auction_tournaments: archiveRowsSchema,
		auction_teams: archiveRowsSchema,
		auction_bid_candidates: archiveRowsSchema,
		auction_lots: archiveRowsSchema,
		auction_actions: archiveRowsSchema,
		auction_team_members: archiveRowsSchema,
		auction_point_ledger: archiveRowsSchema,
		auction_bracket_slots: archiveRowsSchema,
		auction_matches: archiveRowsSchema,
		auction_match_lineups: archiveRowsSchema,
		game_inputs: archiveRowsSchema,
		game_input_picks: archiveRowsSchema,
		game_input_bans: archiveRowsSchema,
		games: archiveRowsSchema,
		game_participants: archiveRowsSchema,
		game_participant_stats: archiveRowsSchema,
		game_bans: archiveRowsSchema,
		user_lane_mmr: archiveRowsSchema,
		mmr_ledger_entries: archiveRowsSchema,
		admin_audit_log: archiveRowsSchema
	})
	.strict();

export const guildFullSnapshotArchiveSchema = z
	.object({
		format: z.literal('MONKEYBOT_DATA_TRANSFER'),
		kind: z.literal(guildFullSnapshotArchiveKind),
		schemaVersion: z.literal(guildFullSnapshotArchiveSchemaVersion),
		exportedAt: timestampSchema,
		source: guildFullSnapshotSourceSchema,
		tables: guildFullSnapshotTablesSchema
	})
	.strict()
	.superRefine((archive, context) => {
		const totalRows = Object.values(archive.tables).reduce((total, rows) => total + rows.length, 0);
		if (totalRows > guildFullSnapshotMaxRows) {
			context.addIssue({
				code: 'custom',
				path: ['tables'],
				message: `Archive contains more than ${guildFullSnapshotMaxRows} rows`
			});
		}
	});

export const guildFullDataTransferSummarySchema = z
	.object({
		totalRows: nonnegativeInteger,
		users: nonnegativeInteger,
		members: nonnegativeInteger,
		seasons: nonnegativeInteger,
		recruitments: nonnegativeInteger,
		series: nonnegativeInteger,
		games: nonnegativeInteger,
		auctionTournaments: nonnegativeInteger,
		auctionMatches: nonnegativeInteger,
		mmrLedgerEntries: nonnegativeInteger,
		auditEntries: nonnegativeInteger
	})
	.strict();

export const guildFullDataTransferIssueSchema = z
	.object({
		reason: z.string().min(1).max(100),
		message: z.string().trim().min(1).max(500),
		table: z.string().min(1).max(100).nullable(),
		rowCount: nonnegativeInteger
	})
	.strict();

export const guildFullDataTransferExportDataSchema = z
	.object({
		archive: guildFullSnapshotArchiveSchema,
		fileName: archiveFileNameSchema,
		summary: guildFullDataTransferSummarySchema
	})
	.strict();

export const guildFullDataTransferPreviewDataSchema = z
	.object({
		importId: z.uuid(),
		digest: digestSchema,
		targetDigest: digestSchema,
		expiresAt: timestampSchema,
		source: guildFullSnapshotSourceSchema,
		summary: guildFullDataTransferSummarySchema,
		conflicts: z.array(guildFullDataTransferIssueSchema).max(100),
		remapsSourceGuild: z.boolean(),
		usesTargetActiveSeason: z.boolean(),
		rebasesDisplayNumbers: z.boolean()
	})
	.strict();

export const guildFullDataTransferApplyDataSchema = z
	.object({
		importId: z.uuid(),
		status: z.literal('APPLIED'),
		appliedAt: timestampSchema,
		summary: guildFullDataTransferSummarySchema,
		remappedSourceGuild: z.boolean(),
		usedTargetActiveSeason: z.boolean(),
		rebasedDisplayNumbers: z.boolean()
	})
	.strict();

export const guildFullDataTransferExportEnvelopeSchema = z
	.object({ data: guildFullDataTransferExportDataSchema, meta: responseMetaSchema })
	.strict();

export const guildFullDataTransferPreviewEnvelopeSchema = z
	.object({ data: guildFullDataTransferPreviewDataSchema, meta: responseMetaSchema })
	.strict();

export const guildFullDataTransferApplyEnvelopeSchema = z
	.object({ data: guildFullDataTransferApplyDataSchema, meta: responseMetaSchema })
	.strict();

export type GuildFullSnapshotArchive = z.infer<typeof guildFullSnapshotArchiveSchema>;
export type GuildFullDataTransferExportData = z.infer<typeof guildFullDataTransferExportDataSchema>;
export type GuildFullDataTransferPreview = z.infer<typeof guildFullDataTransferPreviewDataSchema>;
export type GuildFullDataTransferApplyResult = z.infer<typeof guildFullDataTransferApplyDataSchema>;
