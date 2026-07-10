import { z } from 'zod';

export const queueItemSchema = z.object({
	id: z.string(),
	kind: z.enum(['recruitment', 'series', 'auction', 'result']),
	title: z.string(),
	guildName: z.string(),
	status: z.string(),
	priority: z.enum(['critical', 'high', 'normal']),
	nextAction: z.string(),
	href: z.string(),
	updatedAt: z.string()
});

export const dashboardSummarySchema = z.object({
	openRecruitments: z.number().int().nonnegative(),
	activeSeries: z.number().int().nonnegative(),
	auctionRooms: z.number().int().nonnegative(),
	pendingResults: z.number().int().nonnegative()
});

export type QueueItem = z.infer<typeof queueItemSchema>;
export type DashboardSummary = z.infer<typeof dashboardSummarySchema>;
