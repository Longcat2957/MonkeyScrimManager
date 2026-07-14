import { z } from 'zod';
import {
	auctionFormatSchema,
	auctionRecruitmentRulesSchema,
	championPreferenceLaneSchema,
	userIdSchema
} from './workflow';

const revisionSchema = z.number().int().nonnegative();
const pointsSchema = z.number().int().nonnegative();
const hrefSchema = z.string().startsWith('/g/');
const snapshotOriginSchema = z.enum(['CAPTURED', 'INFERRED', 'UNAVAILABLE']);

export const auctionTournamentTeamMemberSchema = z
	.object({
		teamMemberId: z.uuid(),
		userId: userIdSchema,
		displayName: z.string().min(1).max(100),
		declaredLane: championPreferenceLaneSchema.nullable(),
		mmrAtAcquisition: z.number().finite().nullable(),
		snapshotOrigin: snapshotOriginSchema,
		acquiredVia: z.enum(['CAPTAIN', 'BID', 'MANUAL']),
		rosterSlot: z.number().int().positive().max(5)
	})
	.strict();

export const auctionTournamentTeamSchema = z
	.object({
		id: z.uuid(),
		teamIndex: z.number().int().positive().max(4),
		teamName: z.string().min(1).max(100),
		initialPoints: pointsSchema,
		currentPoints: pointsSchema,
		pointRevision: revisionSchema,
		members: z.array(auctionTournamentTeamMemberSchema).max(5)
	})
	.strict();

export const auctionTournamentDraftCandidateSchema = z
	.object({
		userId: userIdSchema,
		displayName: z.string().min(1).max(100),
		declaredLane: championPreferenceLaneSchema.nullable(),
		mmrAtStart: z.number().finite().nullable(),
		snapshotOrigin: snapshotOriginSchema,
		queuePosition: z.number().int().nonnegative(),
		state: z.enum(['QUEUED', 'ACTIVE', 'UNSOLD', 'ASSIGNED']),
		drawCount: z.number().int().nonnegative()
	})
	.strict();

export const auctionTournamentDraftCommandKindSchema = z.enum([
	'DRAW_NEXT',
	'MARK_UNSOLD',
	'AWARD_BID',
	'ASSIGN_CURRENT',
	'ASSIGN_UNSOLD',
	'RESTORE_UNSOLD',
	'REVERT_LAST'
]);

export const auctionTournamentRecentActionSchema = z
	.object({
		kind: auctionTournamentDraftCommandKindSchema
	})
	.passthrough();

export const auctionTournamentDraftSnapshotSchema = z
	.object({
		tournament: z
			.object({
				id: z.uuid(),
				sourceRecruitmentId: z.uuid(),
				displayNumber: z.number().int().positive(),
				title: z.string().min(1).max(200),
				format: auctionFormatSchema,
				rules: auctionRecruitmentRulesSchema,
				status: z.enum(['BIDDING', 'BRACKET_SETUP', 'IN_GAME', 'COMPLETED', 'CANCELLED']),
				revision: revisionSchema,
				href: hrefSchema
			})
			.strict(),
		teams: z.array(auctionTournamentTeamSchema).min(2).max(4),
		candidates: z.array(auctionTournamentDraftCandidateSchema).max(20),
		draft: z
			.object({
				currentTarget: auctionTournamentDraftCandidateSchema.nullable(),
				unsold: z.array(auctionTournamentDraftCandidateSchema).max(20),
				recentAction: auctionTournamentRecentActionSchema.nullable()
			})
			.strict(),
		permissions: z
			.object({
				canManageDraft: z.boolean(),
				canManageBracket: z.boolean().optional()
			})
			.passthrough()
	})
	.strict()
	.superRefine(({ tournament }, context) => {
		const hasSemifinalRule = tournament.rules.semifinalBestOf !== null;
		if (tournament.format === 'TEN_PERSON' && hasSemifinalRule) {
			context.addIssue({
				code: 'custom',
				path: ['tournament', 'rules', 'semifinalBestOf'],
				message: 'TEN_PERSON tournament must not define semifinalBestOf'
			});
		}
		if (tournament.format === 'TWENTY_PERSON' && !hasSemifinalRule) {
			context.addIssue({
				code: 'custom',
				path: ['tournament', 'rules', 'semifinalBestOf'],
				message: 'TWENTY_PERSON tournament must define semifinalBestOf'
			});
		}
	});

export const auctionTournamentDraftCommandSchema = z.discriminatedUnion('kind', [
	z.object({ kind: z.literal('DRAW_NEXT'), expectedRevision: revisionSchema }).strict(),
	z
		.object({
			kind: z.literal('MARK_UNSOLD'),
			expectedRevision: revisionSchema,
			memberId: userIdSchema
		})
		.strict(),
	z
		.object({
			kind: z.literal('AWARD_BID'),
			expectedRevision: revisionSchema,
			memberId: userIdSchema,
			teamId: z.uuid(),
			points: pointsSchema
		})
		.strict(),
	z
		.object({
			kind: z.literal('ASSIGN_CURRENT'),
			expectedRevision: revisionSchema,
			memberId: userIdSchema,
			teamId: z.uuid()
		})
		.strict(),
	z
		.object({
			kind: z.literal('ASSIGN_UNSOLD'),
			expectedRevision: revisionSchema,
			memberId: userIdSchema,
			teamId: z.uuid()
		})
		.strict(),
	z
		.object({
			kind: z.literal('RESTORE_UNSOLD'),
			expectedRevision: revisionSchema,
			memberId: userIdSchema
		})
		.strict(),
	z.object({ kind: z.literal('REVERT_LAST'), expectedRevision: revisionSchema }).strict()
]);

export type AuctionTournamentDraftSnapshot = z.infer<typeof auctionTournamentDraftSnapshotSchema>;
export type AuctionTournamentDraftCandidate = z.infer<typeof auctionTournamentDraftCandidateSchema>;
export type AuctionTournamentTeam = z.infer<typeof auctionTournamentTeamSchema>;
export type AuctionTournamentTeamMember = z.infer<typeof auctionTournamentTeamMemberSchema>;
export type AuctionTournamentDraftCommand = z.infer<typeof auctionTournamentDraftCommandSchema>;
