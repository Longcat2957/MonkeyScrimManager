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
const timestampSchema = z.iso.datetime({ offset: true });

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

const auctionTournamentCandidateRecordSummarySchema = z
	.object({
		games: z.number().int().nonnegative(),
		wins: z.number().int().nonnegative(),
		losses: z.number().int().nonnegative()
	})
	.strict()
	.superRefine((summary, context) => {
		if (summary.games !== summary.wins + summary.losses) {
			context.addIssue({
				code: 'custom',
				path: ['games'],
				message: 'Candidate record games must equal wins plus losses'
			});
		}
	});

const auctionTournamentCandidateLaneMmrSchema = z
	.object({
		lane: championPreferenceLaneSchema,
		mmr: z.number().finite(),
		games: z.number().int().nonnegative(),
		wins: z.number().int().nonnegative()
	})
	.strict()
	.refine((entry) => entry.wins <= entry.games, 'Candidate lane wins cannot exceed games');

const auctionTournamentCandidateChampionRecordSchema = z
	.object({
		championId: z.string().min(1).max(64),
		games: z.number().int().nonnegative(),
		wins: z.number().int().nonnegative(),
		losses: z.number().int().nonnegative()
	})
	.strict()
	.superRefine((record, context) => {
		if (record.games !== record.wins + record.losses) {
			context.addIssue({
				code: 'custom',
				path: ['games'],
				message: 'Candidate champion games must equal wins plus losses'
			});
		}
	});

const auctionTournamentCandidateRecentGameSchema = z
	.object({
		id: z.uuid(),
		playedAt: timestampSchema,
		result: z.enum(['WIN', 'LOSS']),
		lane: championPreferenceLaneSchema,
		championId: z.string().min(1).max(64).nullable()
	})
	.strict();

const auctionTournamentCurrentTargetRiotSchema = z
	.object({
		games: z.number().int().nonnegative(),
		wins: z.number().int().nonnegative(),
		championIds: z.array(z.string().min(1).max(64)).max(3),
		refreshedAt: timestampSchema
	})
	.strict()
	.refine((summary) => summary.wins <= summary.games, 'Riot wins cannot exceed games');

export const auctionTournamentCurrentTargetProfileSchema = z
	.object({
		avatarUrl: z.url().nullable(),
		profileIconId: z.number().int().nonnegative().nullable(),
		riotTier: z.string().min(1).max(16).nullable(),
		mainLane: championPreferenceLaneSchema.nullable(),
		summary: auctionTournamentCandidateRecordSummarySchema,
		laneMmr: z.array(auctionTournamentCandidateLaneMmrSchema).length(5),
		championIds: z.array(z.string().min(1).max(64)).max(10),
		championRecords: z.array(auctionTournamentCandidateChampionRecordSchema).max(3),
		recentGames: z.array(auctionTournamentCandidateRecentGameSchema).max(8),
		riot: auctionTournamentCurrentTargetRiotSchema.nullable()
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
				currentTargetProfile: auctionTournamentCurrentTargetProfileSchema.nullable(),
				unsold: z.array(auctionTournamentDraftCandidateSchema).max(20),
				recentAction: auctionTournamentRecentActionSchema.nullable()
			})
			.strict(),
		permissions: z
			.object({
				canManageDraft: z.boolean(),
				canManageBracket: z.boolean(),
				canRevertLast: z.boolean(),
				canCancel: z.boolean()
			})
			.strict()
	})
	.strict()
	.superRefine((snapshot, context) => {
		const { tournament } = snapshot;
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
		if (
			(snapshot.draft.currentTarget === null) !==
			(snapshot.draft.currentTargetProfile === null)
		) {
			context.addIssue({
				code: 'custom',
				path: ['draft', 'currentTargetProfile'],
				message: 'Current target and current target profile must be present together'
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
export type AuctionTournamentCurrentTargetProfile = z.infer<
	typeof auctionTournamentCurrentTargetProfileSchema
>;
export type AuctionTournamentTeam = z.infer<typeof auctionTournamentTeamSchema>;
export type AuctionTournamentTeamMember = z.infer<typeof auctionTournamentTeamMemberSchema>;
export type AuctionTournamentDraftCommand = z.infer<typeof auctionTournamentDraftCommandSchema>;
