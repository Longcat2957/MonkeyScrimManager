import { z } from 'zod';
import { dummyLaneSchema, dummyUserIdSchema } from './dummyUsers';

export const dummyRecruitmentActionSchema = z.enum(['ADD', 'REMOVE']);

export const dummyRecruitmentSummarySchema = z
	.object({
		id: z.uuid(),
		title: z.string().min(1),
		status: z.enum(['OPEN', 'CLOSED', 'CONVERTED', 'CANCELLED']),
		targetCount: z.number().int().positive(),
		joinedCount: z.number().int().nonnegative(),
		remainingSlots: z.number().int().nonnegative(),
		revision: z.number().int().nonnegative()
	})
	.strict();

export const dummyRecruitmentCandidateSchema = z
	.object({
		userId: dummyUserIdSchema,
		displayName: z.string().min(1),
		mainLane: dummyLaneSchema.nullable(),
		profileState: z.enum(['ACTIVE', 'ARCHIVED']),
		membershipStatus: z.enum(['ACTIVE', 'INACTIVE']),
		participationState: z.enum(['AVAILABLE', 'JOINED', 'WITHDRAWN'])
	})
	.strict();

export const dummyRecruitmentOverviewSchema = z
	.object({
		recruitment: dummyRecruitmentSummarySchema,
		candidates: z.array(dummyRecruitmentCandidateSchema)
	})
	.strict();

export const dummyRecruitmentPreviewSchema = z
	.object({
		action: dummyRecruitmentActionSchema,
		canApply: z.boolean(),
		currentRevision: z.number().int().nonnegative(),
		projectedJoinedCount: z.number().int().nonnegative(),
		remainingSlots: z.number().int().nonnegative(),
		selected: z.array(dummyRecruitmentCandidateSchema),
		issues: z.array(
			z
				.object({
					userId: dummyUserIdSchema.nullable(),
					code: z.string().min(1),
					message: z.string().min(1)
				})
				.strict()
		)
	})
	.strict();

export const dummyRecruitmentApplyResultSchema = z
	.object({
		action: dummyRecruitmentActionSchema,
		changedUserIds: z.array(dummyUserIdSchema),
		overview: dummyRecruitmentOverviewSchema
	})
	.strict();

export type DummyRecruitmentAction = z.infer<typeof dummyRecruitmentActionSchema>;
export type DummyRecruitmentCandidate = z.infer<typeof dummyRecruitmentCandidateSchema>;
export type DummyRecruitmentOverview = z.infer<typeof dummyRecruitmentOverviewSchema>;
export type DummyRecruitmentPreview = z.infer<typeof dummyRecruitmentPreviewSchema>;
