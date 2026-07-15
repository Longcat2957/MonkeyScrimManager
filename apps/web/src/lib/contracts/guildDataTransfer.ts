import { z } from 'zod';
import { championPreferenceLaneSchema, responseMetaSchema } from '$lib/contracts/workflow';

const snowflakeSchema = z.string().regex(/^[1-9][0-9]{0,19}$/);
const uuidSchema = z.uuid();
const timestampSchema = z.iso.datetime({ offset: true });
const nonnegativeInteger = z.number().int().nonnegative();

function boundedText(maximumCharacters: number) {
	return z.string().refine(
		(value) => {
			const length = Array.from(value.trim()).length;
			return length >= 1 && length <= maximumCharacters;
		},
		{ message: `Text must contain between 1 and ${maximumCharacters} Unicode characters` }
	);
}

export const memberSeasonMmrSeedArchiveKind = 'MEMBER_SEASON_MMR_SEED' as const;
export const memberSeasonMmrSeedArchiveSchemaVersion = 1 as const;
export const memberSeasonMmrSeedMaxMembers = 1_000;

export const memberSeasonMmrSeedArchiveFileNameSchema = z
	.string()
	.min(1)
	.max(255)
	.regex(/^[A-Za-z0-9][A-Za-z0-9._-]*\.json$/, 'Archive file name must be a single JSON file name');

const guildDataTransferSourceSeasonSchema = z
	.object({
		id: uuidSchema,
		name: boundedText(100)
	})
	.strict();

export const guildDataTransferSourceSchema = z
	.object({
		guildId: snowflakeSchema,
		guildName: boundedText(100),
		season: guildDataTransferSourceSeasonSchema
	})
	.strict();

export const guildDataTransferLaneMmrSchema = z
	.object({
		lane: championPreferenceLaneSchema,
		mmr: z.number().finite()
	})
	.strict();

export const memberSeasonMmrSeedMemberSchema = z
	.object({
		discordId: snowflakeSchema,
		displayName: boundedText(100),
		lanes: z.array(guildDataTransferLaneMmrSchema).length(5)
	})
	.strict()
	.superRefine((member, context) => {
		const lanes = new Set(member.lanes.map((entry) => entry.lane));
		if (lanes.size !== member.lanes.length) {
			context.addIssue({
				code: 'custom',
				path: ['lanes'],
				message: 'Lane MMR entries must be unique'
			});
		}
	});

/**
 * Portable, guild-operator archive. The source guild is descriptive only:
 * imports are matched to pre-existing ACTIVE target members by Discord ID.
 */
export const memberSeasonMmrSeedArchiveSchema = z
	.object({
		format: z.literal('MONKEYBOT_DATA_TRANSFER'),
		kind: z.literal(memberSeasonMmrSeedArchiveKind),
		schemaVersion: z.literal(memberSeasonMmrSeedArchiveSchemaVersion),
		exportedAt: timestampSchema,
		source: guildDataTransferSourceSchema,
		members: z.array(memberSeasonMmrSeedMemberSchema).max(memberSeasonMmrSeedMaxMembers)
	})
	.strict()
	.superRefine((archive, context) => {
		const members = new Set(archive.members.map((member) => member.discordId));
		if (members.size !== archive.members.length) {
			context.addIssue({
				code: 'custom',
				path: ['members'],
				message: 'Archive members must be unique by Discord ID'
			});
		}
	});

export const guildDataTransferExportDataSchema = z
	.object({
		archive: memberSeasonMmrSeedArchiveSchema,
		fileName: memberSeasonMmrSeedArchiveFileNameSchema
	})
	.strict();

export const guildDataTransferIssueSchema = z
	.object({
		reason: z.string().min(1).max(100),
		message: boundedText(500),
		discordId: snowflakeSchema,
		displayName: boundedText(100)
	})
	.strict();

export const guildDataTransferSummarySchema = z
	.object({
		archiveMembers: nonnegativeInteger,
		matchedMembers: nonnegativeInteger,
		laneEntries: nonnegativeInteger,
		changes: nonnegativeInteger
	})
	.strict();

export const guildDataTransferPreviewDataSchema = z
	.object({
		importId: z.uuid(),
		digest: z.string().regex(/^[a-f0-9]{64}$/),
		expiresAt: timestampSchema,
		source: guildDataTransferSourceSchema,
		summary: guildDataTransferSummarySchema,
		conflicts: z.array(guildDataTransferIssueSchema).max(memberSeasonMmrSeedMaxMembers),
		targetRevision: nonnegativeInteger
	})
	.strict();

export const guildDataTransferApplyDataSchema = z
	.object({
		importId: z.uuid(),
		status: z.literal('APPLIED'),
		appliedAt: timestampSchema,
		summary: guildDataTransferSummarySchema,
		targetRevision: nonnegativeInteger
	})
	.strict();

export const guildDataTransferExportEnvelopeSchema = z
	.object({
		data: guildDataTransferExportDataSchema,
		meta: responseMetaSchema
	})
	.strict();

export const guildDataTransferPreviewEnvelopeSchema = z
	.object({
		data: guildDataTransferPreviewDataSchema,
		meta: responseMetaSchema
	})
	.strict();

export const guildDataTransferApplyEnvelopeSchema = z
	.object({
		data: guildDataTransferApplyDataSchema,
		meta: responseMetaSchema
	})
	.strict();

export type MemberSeasonMmrSeedArchive = z.infer<typeof memberSeasonMmrSeedArchiveSchema>;
export type GuildDataTransferExportData = z.infer<typeof guildDataTransferExportDataSchema>;
export type GuildDataTransferPreview = z.infer<typeof guildDataTransferPreviewDataSchema>;
export type GuildDataTransferIssue = z.infer<typeof guildDataTransferIssueSchema>;
export type GuildDataTransferApplyResult = z.infer<typeof guildDataTransferApplyDataSchema>;
