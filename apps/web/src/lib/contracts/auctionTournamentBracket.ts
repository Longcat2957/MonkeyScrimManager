import { z } from 'zod';
import {
	auctionBestOfSchema,
	auctionFormatSchema,
	auctionRecruitmentRulesSchema,
	championPreferenceLaneSchema,
	userIdSchema
} from './workflow';

const revisionSchema = z.number().int().nonnegative();
const pointsSchema = z.number().int().nonnegative();

export const auctionBracketSlotKeySchema = z.enum(['A_0', 'A_1', 'B_0', 'B_1']);
export const auctionTournamentMatchKeySchema = z.enum(['SEMIFINAL_A', 'SEMIFINAL_B', 'FINAL']);

export const auctionTournamentBracketMemberSchema = z
	.object({
		teamMemberId: z.uuid(),
		userId: userIdSchema,
		displayName: z.string().min(1).max(100),
		declaredLane: championPreferenceLaneSchema.nullable(),
		mmrAtAcquisition: z.number().finite().nullable(),
		snapshotOrigin: z.enum(['CAPTURED', 'INFERRED', 'UNAVAILABLE']),
		acquiredVia: z.enum(['CAPTAIN', 'BID', 'MANUAL']),
		rosterSlot: z.number().int().positive().max(5)
	})
	.strict();

export const auctionTournamentBracketTeamSchema = z
	.object({
		id: z.uuid(),
		teamIndex: z.number().int().positive().max(4),
		teamName: z.string().min(1).max(100),
		initialPoints: pointsSchema,
		currentPoints: pointsSchema,
		members: z.array(auctionTournamentBracketMemberSchema).max(5)
	})
	.strict();

export const auctionTournamentBracketMatchSchema = z
	.object({
		id: z.uuid(),
		matchKey: auctionTournamentMatchKeySchema,
		bestOf: auctionBestOfSchema,
		hardFearless: z.boolean(),
		leftTeamId: z.uuid().nullable(),
		rightTeamId: z.uuid().nullable(),
		status: z.enum(['LINEUP_PENDING', 'READY', 'IN_PROGRESS', 'COMPLETED', 'CANCELLED']),
		revision: revisionSchema,
		leftScore: pointsSchema,
		rightScore: pointsSchema,
		winnerTeamId: z.uuid().nullable(),
		lineupsLocked: z.boolean()
	})
	.strict();

export const auctionTournamentBracketSnapshotSchema = z
	.object({
		tournament: z
			.object({
				id: z.uuid(),
				title: z.string().min(1).max(200),
				displayNumber: z.number().int().positive(),
				format: auctionFormatSchema,
				rules: auctionRecruitmentRulesSchema,
				status: z.enum(['BIDDING', 'BRACKET_SETUP', 'IN_GAME', 'COMPLETED', 'CANCELLED']),
				revision: revisionSchema,
				href: z.string().startsWith('/g/')
			})
			.strict(),
		teams: z.array(auctionTournamentBracketTeamSchema).min(2).max(4),
		bracket: z
			.object({
				locked: z.boolean(),
				slots: z
					.array(
						z
							.object({
								slotKey: auctionBracketSlotKeySchema,
								teamId: z.uuid().nullable()
							})
							.strict()
					)
					.length(4),
				matches: z.array(auctionTournamentBracketMatchSchema).max(3)
			})
			.strict(),
		permissions: z
			.object({
				canManageBracket: z.boolean(),
				canManageMatches: z.boolean(),
				canRecoverBracket: z.boolean()
			})
			.strict()
	})
	.strict()
	.superRefine((snapshot, context) => {
		const teamIds = new Set(snapshot.teams.map((team) => team.id));
		const slots = new Set(snapshot.bracket.slots.map((slot) => slot.slotKey));
		if (slots.size !== snapshot.bracket.slots.length) {
			context.addIssue({
				code: 'custom',
				path: ['bracket', 'slots'],
				message: '브래킷 슬롯이 중복되었습니다.'
			});
		}

		const assignedTeamIds = snapshot.bracket.slots.flatMap((slot) =>
			slot.teamId === null ? [] : [slot.teamId]
		);
		if (new Set(assignedTeamIds).size !== assignedTeamIds.length) {
			context.addIssue({
				code: 'custom',
				path: ['bracket', 'slots'],
				message: '같은 팀이 여러 브래킷 슬롯에 배치되었습니다.'
			});
		}
		for (const teamId of assignedTeamIds) {
			if (!teamIds.has(teamId)) {
				context.addIssue({
					code: 'custom',
					path: ['bracket', 'slots'],
					message: '브래킷 슬롯이 토너먼트 팀을 참조하지 않습니다.'
				});
			}
		}
	});

export const auctionTournamentBracketCommandSchema = z.discriminatedUnion('kind', [
	z
		.object({
			kind: z.literal('ASSIGN_TEAM'),
			expectedRevision: revisionSchema,
			slotKey: auctionBracketSlotKeySchema,
			teamId: z.uuid()
		})
		.strict(),
	z
		.object({
			kind: z.literal('CLEAR_SLOT'),
			expectedRevision: revisionSchema,
			slotKey: auctionBracketSlotKeySchema
		})
		.strict(),
	z.object({ kind: z.literal('LOCK_BRACKET'), expectedRevision: revisionSchema }).strict(),
	z.object({ kind: z.literal('UNLOCK_BRACKET'), expectedRevision: revisionSchema }).strict()
]);

export type AuctionBracketSlotKey = z.infer<typeof auctionBracketSlotKeySchema>;
export type AuctionTournamentBracketTeam = z.infer<typeof auctionTournamentBracketTeamSchema>;
export type AuctionTournamentBracketMatch = z.infer<typeof auctionTournamentBracketMatchSchema>;
export type AuctionTournamentBracketSnapshot = z.infer<
	typeof auctionTournamentBracketSnapshotSchema
>;
export type AuctionTournamentBracketCommand = z.infer<typeof auctionTournamentBracketCommandSchema>;
