import { z } from 'zod';

const uuidSchema = z.uuid();
const revisionSchema = z.number().int().nonnegative();

export const auctionTournamentCancelPreviewSchema = z
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

export const auctionTournamentCancelCommandSchema = z
	.object({
		expectedRevision: revisionSchema,
		reason: z.string().trim().min(1).max(240)
	})
	.strict();

export const auctionTournamentCancelResultSchema = z
	.object({
		tournamentId: uuidSchema,
		tournamentStatus: z.literal('CANCELLED'),
		tournamentRevision: revisionSchema,
		sourceRecruitmentId: uuidSchema,
		sourceRecruitmentRevision: revisionSchema,
		reversedGameCount: revisionSchema
	})
	.strict();

export type AuctionTournamentCancelPreview = z.infer<
	typeof auctionTournamentCancelPreviewSchema
>;
export type AuctionTournamentCancelCommand = z.infer<
	typeof auctionTournamentCancelCommandSchema
>;
export type AuctionTournamentCancelResult = z.infer<typeof auctionTournamentCancelResultSchema>;
