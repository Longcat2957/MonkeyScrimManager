import { z } from 'zod';
import {
	auctionFormatSchema,
	auctionRecruitmentRulesSchema,
	championPreferenceLaneSchema,
	userIdSchema
} from './workflow';

const snowflakeSchema = z.string().regex(/^[1-9][0-9]{0,19}$/);
const timestampSchema = z.iso.datetime({ offset: true });

export const auctionRecruitmentStatusSchema = z.enum(['OPEN', 'CLOSED', 'CONVERTED', 'CANCELLED']);

export const auctionRecruitmentSummarySchema = z
	.object({
		id: z.uuid(),
		displayNumber: z.number().int().positive(),
		title: z.string().min(1).max(100),
		format: auctionFormatSchema,
		rules: auctionRecruitmentRulesSchema,
		participantCount: z.union([z.literal(10), z.literal(20)]),
		joinedParticipantCount: z.number().int().nonnegative(),
		teamCount: z.union([z.literal(2), z.literal(4)]),
		status: auctionRecruitmentStatusSchema,
		revision: z.number().int().nonnegative(),
		channelId: snowflakeSchema.nullable(),
		messageId: snowflakeSchema.nullable(),
		createdById: userIdSchema,
		updatedAt: timestampSchema,
		href: z.string().startsWith('/g/')
	})
	.strict()
	.superRefine(({ format, rules }, context) => {
		const valid =
			(format === 'TEN_PERSON' && rules.semifinalBestOf === null) ||
			(format === 'TWENTY_PERSON' && rules.semifinalBestOf !== null);
		if (!valid) {
			context.addIssue({
				code: 'custom',
				path: ['rules', 'semifinalBestOf'],
				message: '경매 인원 형식과 4강 시리즈 규칙이 일치하지 않습니다.'
			});
		}
	});

export const auctionRecruitmentParticipantSchema = z
	.object({
		id: userIdSchema,
		displayName: z.string().min(1).max(100),
		declaredLane: championPreferenceLaneSchema.nullable()
	})
	.strict();

export const auctionRecruitmentLinksSchema = z
	.object({
		draft: z.string().startsWith('/g/').nullable()
	})
	.strict();

export const auctionRecruitmentPermissionsSchema = z
	.object({
		canManageMembers: z.boolean(),
		canClose: z.boolean(),
		canCancel: z.boolean()
	})
	.strict();

export const auctionRecruitmentSnapshotSchema = z
	.object({
		recruitment: auctionRecruitmentSummarySchema,
		participants: z.array(auctionRecruitmentParticipantSchema).max(20),
		links: auctionRecruitmentLinksSchema,
		permissions: auctionRecruitmentPermissionsSchema
	})
	.strict();

export type AuctionRecruitmentSnapshot = z.infer<typeof auctionRecruitmentSnapshotSchema>;
