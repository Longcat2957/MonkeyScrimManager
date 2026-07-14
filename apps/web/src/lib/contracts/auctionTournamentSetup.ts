import { z } from 'zod';
import { championPreferenceLaneSchema, userIdSchema } from './workflow';

const nonnegativeIntegerSchema = z.number().int().nonnegative();
const positiveIntegerSchema = z.number().int().positive();

export const auctionSetupTeamSchema = z
	.object({
		teamIndex: z.number().int().min(1).max(4),
		teamName: z.string().min(1).max(100),
		captainUserId: userIdSchema.nullable(),
		initialPoints: positiveIntegerSchema
	})
	.strict();

export const auctionSetupConfiguredTeamSchema = auctionSetupTeamSchema
	.extend({ captainUserId: userIdSchema })
	.strict();

export const auctionSetupDraftSchema = z
	.object({
		recruitmentId: z.uuid(),
		revision: nonnegativeIntegerSchema,
		teamCount: z.union([z.literal(2), z.literal(4)]),
		teams: z.array(auctionSetupTeamSchema).min(2).max(4)
	})
	.strict()
	.superRefine((draft, context) => {
		if (draft.teams.length !== draft.teamCount) {
			context.addIssue({
				code: 'custom',
				path: ['teams'],
				message: '팀 수가 경매 형식과 일치하지 않습니다.'
			});
		}

		const teamIndexes = draft.teams
			.map((team) => team.teamIndex)
			.sort((left, right) => left - right);
		if (!teamIndexes.every((teamIndex, index) => teamIndex === index + 1)) {
			context.addIssue({
				code: 'custom',
				path: ['teams'],
				message: '팀 순서가 올바르지 않습니다.'
			});
		}

		const captainIds = draft.teams.flatMap((team) =>
			team.captainUserId === null ? [] : [team.captainUserId]
		);
		if (new Set(captainIds).size !== captainIds.length) {
			context.addIssue({
				code: 'custom',
				path: ['teams'],
				message: '팀장을 중복 지정할 수 없습니다.'
			});
		}
	});

export const auctionSetupParticipantSchema = z
	.object({
		id: userIdSchema,
		displayName: z.string().min(1).max(100),
		declaredLane: championPreferenceLaneSchema.nullable()
	})
	.strict();

export const auctionSetupPermissionsSchema = z
	.object({
		canEdit: z.boolean(),
		canCreateTournament: z.boolean()
	})
	.strict();

export const auctionSetupDraftSnapshotSchema = z
	.object({
		draft: auctionSetupDraftSchema,
		participants: z.array(auctionSetupParticipantSchema).max(20),
		permissions: auctionSetupPermissionsSchema
	})
	.strict();

export const auctionTournamentNavigationSchema = z
	.object({
		id: z.uuid(),
		href: z.string().startsWith('/g/')
	})
	.passthrough();

export type AuctionSetupDraft = z.infer<typeof auctionSetupDraftSchema>;
export type AuctionSetupDraftSnapshot = z.infer<typeof auctionSetupDraftSnapshotSchema>;
export type AuctionSetupParticipant = z.infer<typeof auctionSetupParticipantSchema>;
export type AuctionSetupTeam = z.infer<typeof auctionSetupTeamSchema>;
export type AuctionSetupConfiguredTeam = z.infer<typeof auctionSetupConfiguredTeamSchema>;
export type AuctionTournamentNavigation = z.infer<typeof auctionTournamentNavigationSchema>;
