import {
	ApplicationCommandOptionType,
	ApplicationIntegrationType,
	InteractionContextType,
	MessageFlags,
	type ChatInputCommandInteraction
} from 'discord.js';
import type { Logger } from 'pino';
import { describe, expect, it, vi } from 'vitest';

import type { MonkeyBotApiClient } from '../api/client.js';
import type { AuctionTournamentListData } from '../api/contracts.js';
import type { BotConfig } from '../config.js';
import type { GuildCatalogSync } from '../discord/catalog-sync.js';
import type { InteractionContext } from '../interactions/context.js';
import { auctionTournamentListCommand } from './auction-tournament-list.js';

const guildId = '200000000000000001';
const userId = '100000000000000001';
const interactionId = '400000000000000001';
const tournamentId = '01900000-0000-7000-8000-000000000103';
const seasonId = '01900000-0000-7000-8000-000000000201';

const page: AuctionTournamentListData = {
	tournaments: [
		{
			id: tournamentId,
			displayNumber: 216,
			title: '20인 금요 경매 내전',
			format: 'TWENTY_PERSON',
			participantCount: 20,
			teamCount: 4,
			status: 'IN_GAME',
			revision: 9,
			activeMemberCount: 20,
			bidMemberCount: 20,
			incompleteMatchCount: 2,
			nextMatch: {
				id: '01900000-0000-7000-8000-000000000104',
				label: '준결승 A',
				matchKey: 'SEMIFINAL_A',
				status: 'READY',
				confirmedGameCount: 0
			},
			championTeam: null,
			updatedAt: '2026-07-15T10:00:00Z',
			endedAt: null,
			href: `/g/${guildId}/auction/matches/01900000-0000-7000-8000-000000000104`
		}
	],
	nextCursor: null
};

function context(listAuctionTournaments: ReturnType<typeof vi.fn>): InteractionContext {
	return {
		api: { listAuctionTournaments } as unknown as MonkeyBotApiClient,
		catalogSync: {} as GuildCatalogSync,
		config: { webOrigin: new URL('http://localhost:5173') } as BotConfig,
		logger: {} as Logger
	};
}

function interaction(
	options: { state?: string | null; seasonId?: string | null; limit?: number | null } = {}
) {
	const deferReply = vi.fn(async () => undefined);
	const editReply = vi.fn(async () => undefined);
	const reply = vi.fn(async () => undefined);
	return {
		value: {
			id: interactionId,
			guildId,
			guild: {},
			member: { nick: 'MSM Longcat', roles: ['300000000000000001'] },
			user: {
				id: userId,
				username: 'longcat_login',
				globalName: 'Longcat',
				avatar: null
			},
			options: {
				getString: (name: string) => {
					if (name === '상태') return options.state ?? 'active';
					if (name === '시즌') return options.seasonId ?? null;
					return null;
				},
				getInteger: (name: string) => (name === 'limit' ? (options.limit ?? null) : null)
			},
			inCachedGuild: () => true,
			deferReply,
			editReply,
			reply
		} as unknown as ChatInputCommandInteraction,
		deferReply,
		editReply,
		reply
	};
}

describe('/경매목록', () => {
	it('defines optional state, season, and limit filters in guild context', () => {
		const definition = auctionTournamentListCommand.data.toJSON();

		expect(definition).toMatchObject({
			name: '경매목록',
			integration_types: [ApplicationIntegrationType.GuildInstall],
			contexts: [InteractionContextType.Guild]
		});
		expect(definition.options).toEqual([
			expect.objectContaining({ name: '상태', type: ApplicationCommandOptionType.String }),
			expect.objectContaining({
				name: '시즌',
				type: ApplicationCommandOptionType.String,
				min_length: 36,
				max_length: 36
			}),
			expect.objectContaining({
				name: 'limit',
				type: ApplicationCommandOptionType.Integer,
				min_value: 1,
				max_value: 10
			})
		]);
	});

	it('lists the requested tournament state with deep links to the web workflow', async () => {
		const listAuctionTournaments = vi.fn(async () => page);
		const current = interaction({ seasonId, limit: 5 });

		await auctionTournamentListCommand.execute(current.value, context(listAuctionTournaments));

		expect(current.deferReply).toHaveBeenCalledWith({
			flags: MessageFlags.IsComponentsV2 | MessageFlags.Ephemeral
		});
		expect(listAuctionTournaments).toHaveBeenCalledWith(guildId, {
			state: 'active',
			seasonId,
			limit: 5
		});
		expect(serializedReply(current.editReply).flags).toBe(MessageFlags.IsComponentsV2);
		expect(renderedReply(current.editReply)).toContain('진행 중 경매 내전');
		expect(renderedReply(current.editReply)).toContain('#216 20인 금요 경매 내전');
		expect(renderedReply(current.editReply)).toContain('경기 진행');
		expect(renderedReply(current.editReply)).toContain('작업 열기');
		expect(renderedReply(current.editReply)).toContain(
			`http://localhost:5173/g/${guildId}/auction/matches/01900000-0000-7000-8000-000000000104`
		);
		expect(renderedReply(current.editReply)).toContain(
			`http://localhost:5173/g/${guildId}/auction`
		);
	});

	it('rejects a malformed season ID before making an API request', async () => {
		const listAuctionTournaments = vi.fn(async () => page);
		const current = interaction({ seasonId: 'season-1' });

		await auctionTournamentListCommand.execute(current.value, context(listAuctionTournaments));

		expect(current.deferReply).not.toHaveBeenCalled();
		expect(listAuctionTournaments).not.toHaveBeenCalled();
		expect(serializedReply(current.reply).flags).toBe(
			MessageFlags.IsComponentsV2 | MessageFlags.Ephemeral
		);
		expect(renderedReply(current.reply)).toContain('시즌 ID는 UUID 형식이어야 합니다.');
	});
});

function serializedReply(reply: ReturnType<typeof vi.fn>): {
	flags: number;
	components: unknown[];
} {
	return JSON.parse(JSON.stringify(reply.mock.calls[0]?.[0])) as {
		flags: number;
		components: unknown[];
	};
}

function renderedReply(reply: ReturnType<typeof vi.fn>): string {
	return JSON.stringify(serializedReply(reply));
}
