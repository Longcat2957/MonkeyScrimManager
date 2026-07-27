import { z } from 'zod';

export const participantConfirmationDmDispatchResultSchema = z
	.object({
		queuedRecipientCount: z.number().int().nonnegative(),
		alreadyQueuedRecipientCount: z.number().int().nonnegative()
	})
	.strict();

export type ParticipantConfirmationDmDispatchResult = z.infer<
	typeof participantConfirmationDmDispatchResultSchema
>;
