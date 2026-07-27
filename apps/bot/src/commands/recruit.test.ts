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
import type { RecruitmentSnapshot } from '../api/contracts.js';
import type { BotConfig } from '../config.js';
import type { GuildCatalogSync } from '../discord/catalog-sync.js';
import type { InteractionContext } from '../interactions/context.js';
import { recruitCommand } from './recruit.js';

const guildId = '200000000000000001';
const userId = '100000000000000001';
const interactionId = '400000000000000001';
const recruitmentId = '01900000-0000-7000-8000-000000000101';
const expectedStartInput = '2026-07-26 20:00';
const expectedStartAt = '2026-07-26T20:00:00+09:00';

const snapshot: RecruitmentSnapshot = {
	recruitment: {
		id: recruitmentId,
		displayNumber: 214,
		title: '일반 내전 모집 #214 · 예상 시작 2026-07-26 20:00 (KST)',
		status: 'OPEN',
		targetCount: 10,
		bestOf: 3,
		hardFearless: false,
		revision: 0,
		channelId: null,
		messageId: null,
		createdById: userId,
		updatedAt: '2026-07-14T10:00:00Z'
	},
	participants: [],
	links: { entry: `/g/${guildId}/recruitments/${recruitmentId}/entry` }
};

function context(
	createRecruitment: ReturnType<typeof vi.fn>,
	bindRecruitmentMessage: ReturnType<typeof vi.fn>
): InteractionContext {
	return {
		api: { createRecruitment, bindRecruitmentMessage } as unknown as MonkeyBotApiClient,
		catalogSync: {} as GuildCatalogSync,
		config: { webOrigin: new URL('http://localhost:5173') } as BotConfig,
		logger: {} as Logger
	};
}

function interaction(
	bestOf: number | null,
	hardFearless: boolean | null,
	startInput = expectedStartInput
) {
	const editReply = vi.fn<(options: unknown) => Promise<{ channelId: string; id: string }>>(
		async () => ({
			channelId: '500000000000000001',
			id: '600000000000000001'
		})
	);
	const deferReply = vi.fn<(options: unknown) => Promise<void>>(async () => undefined);
	const reply = vi.fn<(options: unknown) => Promise<void>>(async () => undefined);
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
				getInteger: () => bestOf,
				getBoolean: () => hardFearless,
				getString: () => startInput
			},
			inCachedGuild: () => true,
			deferReply,
			editReply,
			reply
		} as unknown as ChatInputCommandInteraction,
		editReply,
		deferReply,
		reply
	};
}

describe('/내전모집', () => {
	it('requires a KST expected-start option before optional series rules', () => {
		const definition = recruitCommand.data.toJSON();

		expect(definition).toMatchObject({
			name: '내전모집',
			integration_types: [ApplicationIntegrationType.GuildInstall],
			contexts: [InteractionContextType.Guild]
		});
		expect(definition.options).toEqual([
			expect.objectContaining({
				name: '예상시작',
				type: ApplicationCommandOptionType.String,
				required: true
			}),
			expect.objectContaining({
				name: '형식',
				type: ApplicationCommandOptionType.Integer,
				required: false,
				choices: [
					{ name: 'BO1', value: 1 },
					{ name: 'BO3', value: 3 },
					{ name: 'BO5', value: 5 }
				]
			}),
			expect.objectContaining({
				name: '하드피어리스',
				type: ApplicationCommandOptionType.Boolean,
				required: false
			})
		]);
	});

	it.each([
		{ bestOf: null, hardFearless: null, expectedBestOf: 3, expectedHardFearless: true },
		{ bestOf: 5, hardFearless: true, expectedBestOf: 5, expectedHardFearless: true }
	])(
		'sends selected rules and a canonical expected start to the recruitment API',
		async (testCase) => {
			const createRecruitment = vi.fn(async () => snapshot);
			const bindRecruitmentMessage = vi.fn(async () => snapshot);
			const current = interaction(testCase.bestOf, testCase.hardFearless);

			await recruitCommand.execute(
				current.value,
				context(createRecruitment, bindRecruitmentMessage)
			);

			expect(current.deferReply).toHaveBeenCalledWith({ flags: MessageFlags.IsComponentsV2 });
			expect(current.editReply).toHaveBeenCalledWith(
				expect.objectContaining({ flags: MessageFlags.IsComponentsV2 })
			);
			const rendered = current.editReply.mock.calls[0]?.[0] as Record<string, unknown>;
			expect(rendered).not.toHaveProperty('embeds');
			expect(rendered).not.toHaveProperty('content');
			expect(createRecruitment).toHaveBeenCalledWith(
				guildId,
				{
					actor: {
						id: userId,
						globalDisplayName: 'Longcat',
						displayName: 'MSM Longcat',
						avatarHash: null,
						roleIds: ['300000000000000001']
					},
					bestOf: testCase.expectedBestOf,
					hardFearless: testCase.expectedHardFearless,
					expectedStartAt
				},
				{ idempotencyKey: interactionId }
			);
			expect(bindRecruitmentMessage).toHaveBeenCalledWith(
				guildId,
				recruitmentId,
				expect.objectContaining({
					channelId: '500000000000000001',
					messageId: '600000000000000001'
				}),
				{ idempotencyKey: interactionId }
			);
		}
	);

	it('rejects an invalid expected-start value before creating a recruitment', async () => {
		const createRecruitment = vi.fn(async () => snapshot);
		const bindRecruitmentMessage = vi.fn(async () => snapshot);
		const current = interaction(null, null, '2026-02-30 20:00');

		await recruitCommand.execute(current.value, context(createRecruitment, bindRecruitmentMessage));

		expect(createRecruitment).not.toHaveBeenCalled();
		expect(current.deferReply).not.toHaveBeenCalled();
		expect(JSON.stringify(current.reply.mock.calls[0]?.[0])).toContain('KST 24시간 형식');
	});

	it('returns the guild-context reason in an ephemeral V2 notice', async () => {
		const createRecruitment = vi.fn(async () => snapshot);
		const bindRecruitmentMessage = vi.fn(async () => snapshot);
		const current = interaction(null, null);
		(current.value as unknown as { guildId: string | null }).guildId = null;

		await recruitCommand.execute(current.value, context(createRecruitment, bindRecruitmentMessage));

		expect(current.reply).toHaveBeenCalledWith(
			expect.objectContaining({
				flags: MessageFlags.IsComponentsV2 | MessageFlags.Ephemeral
			})
		);
		const reply = current.reply.mock.calls[0]?.[0] as Record<string, unknown>;
		expect(JSON.stringify(reply)).toContain('Discord 서버에서만 사용할 수 있습니다.');
		expect(reply).not.toHaveProperty('content');
		expect(reply).not.toHaveProperty('embeds');
		expect(createRecruitment).not.toHaveBeenCalled();
	});
});
