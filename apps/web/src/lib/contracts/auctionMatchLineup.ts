import { z } from 'zod';
import { auctionBestOfSchema, championPreferenceLaneSchema, userIdSchema } from './workflow';

const nonnegativeIntegerSchema = z.number().int().nonnegative();

export const auctionMatchKeySchema = z.enum(['SEMIFINAL_A', 'SEMIFINAL_B', 'FINAL']);
export const auctionMatchStatusSchema = z.enum([
	'LINEUP_PENDING',
	'READY',
	'IN_PROGRESS',
	'COMPLETED',
	'CANCELLED'
]);
export const auctionMatchSnapshotOriginSchema = z.enum(['CAPTURED', 'INFERRED', 'UNAVAILABLE']);
export const auctionMatchMemberAcquisitionSchema = z.enum(['CAPTAIN', 'BID', 'MANUAL']);

export const auctionMatchLineupMemberSchema = z
	.object({
		teamMemberId: z.uuid(),
		userId: userIdSchema,
		displayName: z.string().min(1).max(100),
		declaredLane: championPreferenceLaneSchema.nullable(),
		mmrAtAcquisition: z.number().finite().nullable(),
		snapshotOrigin: auctionMatchSnapshotOriginSchema,
		acquiredVia: auctionMatchMemberAcquisitionSchema,
		rosterSlot: z.number().int().positive().max(5)
	})
	.strict();

export const auctionMatchLineupTeamSchema = z
	.object({
		id: z.uuid(),
		teamName: z.string().min(1).max(100),
		members: z.array(auctionMatchLineupMemberSchema).min(1).max(5)
	})
	.strict();

export const auctionMatchLineupAssignmentSchema = z
	.object({
		teamId: z.uuid(),
		lane: championPreferenceLaneSchema,
		teamMemberId: z.uuid(),
		userId: userIdSchema
	})
	.strict();

export const auctionMatchLineupMatchSchema = z
	.object({
		id: z.uuid(),
		tournamentId: z.uuid(),
		matchKey: auctionMatchKeySchema,
		label: z.string().min(1).max(100),
		bestOf: auctionBestOfSchema,
		hardFearless: z.boolean(),
		status: auctionMatchStatusSchema,
		revision: nonnegativeIntegerSchema,
		leftTeamId: z.uuid(),
		rightTeamId: z.uuid(),
		leftScore: nonnegativeIntegerSchema,
		rightScore: nonnegativeIntegerSchema,
		winnerTeamId: z.uuid().nullable(),
		lineupsLocked: z.boolean()
	})
	.strict();

export const auctionMatchLineupPermissionsSchema = z
	.object({
		canManageLineup: z.boolean(),
		canEnterResults: z.boolean(),
		canUnlockLineup: z.boolean(),
		canRevertLastGame: z.boolean()
	})
	.strict();

export const auctionMatchLineupSnapshotSchema = z
	.object({
		match: auctionMatchLineupMatchSchema,
		teams: z.array(auctionMatchLineupTeamSchema).length(2),
		lineups: z.array(auctionMatchLineupAssignmentSchema).max(10),
		permissions: auctionMatchLineupPermissionsSchema
	})
	.passthrough()
	.superRefine((snapshot, context) => {
		const { match, teams, lineups } = snapshot;
		if (match.leftTeamId === match.rightTeamId) {
			context.addIssue({
				code: 'custom',
				path: ['match'],
				message: '경기의 양 팀은 서로 달라야 합니다.'
			});
		}

		const teamById = new Map(teams.map((team) => [team.id, team]));
		const expectedTeamIds = new Set([match.leftTeamId, match.rightTeamId]);
		if (
			teamById.size !== teams.length ||
			teams.some((team) => !expectedTeamIds.has(team.id)) ||
			expectedTeamIds.size !== teamById.size
		) {
			context.addIssue({
				code: 'custom',
				path: ['teams'],
				message: '경기 팀 정보가 양 팀과 일치하지 않습니다.'
			});
		}

		const occupiedSlots = new Set<string>();
		const assignedMemberIds = new Set<string>();
		const assignedUserIds = new Set<string>();
		for (const [index, lineup] of lineups.entries()) {
			const team = teamById.get(lineup.teamId);
			if (!team) {
				context.addIssue({
					code: 'custom',
					path: ['lineups', index, 'teamId'],
					message: '라인업이 현재 경기 팀을 참조하지 않습니다.'
				});
				continue;
			}

			const slotKey = `${lineup.teamId}:${lineup.lane}`;
			if (occupiedSlots.has(slotKey)) {
				context.addIssue({
					code: 'custom',
					path: ['lineups', index],
					message: '같은 팀의 동일 라인을 중복 배치할 수 없습니다.'
				});
			}
			occupiedSlots.add(slotKey);

			const member = team.members.find(
				(candidate) => candidate.teamMemberId === lineup.teamMemberId
			);
			if (!member || member.userId !== lineup.userId) {
				context.addIssue({
					code: 'custom',
					path: ['lineups', index],
					message: '라인업 선수가 해당 팀 roster와 일치하지 않습니다.'
				});
			}
			if (assignedMemberIds.has(lineup.teamMemberId) || assignedUserIds.has(lineup.userId)) {
				context.addIssue({
					code: 'custom',
					path: ['lineups', index],
					message: '같은 선수는 경기 라인업에 한 번만 배치할 수 있습니다.'
				});
			}
			assignedMemberIds.add(lineup.teamMemberId);
			assignedUserIds.add(lineup.userId);
		}

		if (match.lineupsLocked && occupiedSlots.size !== 10) {
			context.addIssue({
				code: 'custom',
				path: ['lineups'],
				message: '확정된 라인업에는 양 팀의 다섯 라인이 모두 필요합니다.'
			});
		}
	});

export const auctionMatchLineupAssignCommandSchema = z
	.object({
		expectedRevision: nonnegativeIntegerSchema,
		teamId: z.uuid(),
		lane: championPreferenceLaneSchema,
		teamMemberId: z.uuid().nullable()
	})
	.strict();

export const auctionMatchLineupLockCommandSchema = z
	.object({ expectedRevision: nonnegativeIntegerSchema })
	.strict();

export const auctionMatchLineupUnlockCommandSchema = z
	.object({ expectedRevision: nonnegativeIntegerSchema })
	.strict();

export type AuctionMatchLineupAssignment = z.infer<typeof auctionMatchLineupAssignmentSchema>;
export type AuctionMatchLineupAssignCommand = z.infer<typeof auctionMatchLineupAssignCommandSchema>;
export type AuctionMatchLineupLockCommand = z.infer<typeof auctionMatchLineupLockCommandSchema>;
export type AuctionMatchLineupUnlockCommand = z.infer<typeof auctionMatchLineupUnlockCommandSchema>;
export type AuctionMatchLineupMember = z.infer<typeof auctionMatchLineupMemberSchema>;
export type AuctionMatchLineupSnapshot = z.infer<typeof auctionMatchLineupSnapshotSchema>;
export type AuctionMatchLineupTeam = z.infer<typeof auctionMatchLineupTeamSchema>;
