import { z } from 'zod';

const uuidSchema = z.uuid();
const revisionSchema = z.number().int().nonnegative();

export const auctionTournamentFinishPreviewSchema = z
	.object({
		tournament: z
			.object({
				id: uuidSchema,
				displayNumber: z.number().int().positive(),
				title: z.string().min(1).max(200),
				status: z.enum(['BIDDING', 'BRACKET_SETUP', 'IN_GAME', 'COMPLETED', 'CANCELLED']),
				revision: revisionSchema
			})
			.strict()
	})
	.strict();

export const auctionTournamentFinishCommandSchema = z
	.object({
		expectedRevision: revisionSchema,
		reason: z.string().trim().min(1).max(240)
	})
	.strict();

export const auctionTournamentFinishResultSchema = z
	.object({
		tournamentId: uuidSchema,
		tournamentStatus: z.literal('COMPLETED'),
		tournamentRevision: revisionSchema,
		championTeamId: uuidSchema.nullable()
	})
	.strict();

export type AuctionTournamentFinishPreview = z.infer<typeof auctionTournamentFinishPreviewSchema>;
export type AuctionTournamentFinishCommand = z.infer<typeof auctionTournamentFinishCommandSchema>;
export type AuctionTournamentFinishResult = z.infer<typeof auctionTournamentFinishResultSchema>;
