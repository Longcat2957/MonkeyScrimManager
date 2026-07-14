import { z } from 'zod';
import { championPreferenceLaneSchema, userIdSchema } from './workflow';

const snowflakeSchema = z.string().regex(/^[1-9][0-9]{0,19}$/);
const timestampSchema = z.iso.datetime({ offset: true });

export const recruitmentStatusSchema = z.enum(['OPEN', 'CLOSED', 'CANCELLED']);

export const recruitmentSummarySchema = z
	.object({
		id: z.uuid(),
		displayNumber: z.number().int().positive(),
		title: z.string().min(1).max(100),
		status: recruitmentStatusSchema,
		targetCount: z.literal(10),
		revision: z.number().int().nonnegative(),
		createdById: userIdSchema,
		channelId: snowflakeSchema.nullable(),
		messageId: snowflakeSchema.nullable(),
		updatedAt: timestampSchema
	})
	.strict();

export const recruitmentParticipantSchema = z
	.object({
		id: userIdSchema,
		displayName: z.string().min(1).max(100),
		preferredLanes: z.array(championPreferenceLaneSchema).max(5)
	})
	.strict();

export const recruitmentLinksSchema = z.object({ entry: z.string().startsWith('/g/') }).strict();

export const recruitmentSnapshotSchema = z
	.object({
		recruitment: recruitmentSummarySchema,
		participants: z.array(recruitmentParticipantSchema).max(10),
		links: recruitmentLinksSchema
	})
	.strict();

export type RecruitmentSnapshot = z.infer<typeof recruitmentSnapshotSchema>;
