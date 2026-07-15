import {
	ApplicationIntegrationType,
	InteractionContextType,
	SlashCommandBuilder
} from 'discord.js';
import { actorFromInteraction } from '../discord/actor.js';
import {
	botDeferOptions,
	botEditReply,
	botEphemeralReply,
	botNotice
} from '../discord/message-v2.js';
import { renderRecruitmentMessage } from '../discord/recruitment-message.js';
import { guildInteractionError } from '../interactions/guild-context.js';
import type { BotCommand } from './types.js';

const BEST_OF_OPTION = '형식';
const HARD_FEARLESS_OPTION = '하드피어리스';
const DEFAULT_BEST_OF = 3;
const DEFAULT_HARD_FEARLESS = false;

export const recruitCommand: BotCommand = {
	data: new SlashCommandBuilder()
		.setName('내전모집')
		.setDescription('10명 5v5 내전 참가자를 모집합니다.')
		.setIntegrationTypes(ApplicationIntegrationType.GuildInstall)
		.setContexts(InteractionContextType.Guild)
		.addIntegerOption((option) =>
			option
				.setName(BEST_OF_OPTION)
				.setDescription('시리즈 형식')
				.addChoices({ name: 'BO1', value: 1 }, { name: 'BO3', value: 3 }, { name: 'BO5', value: 5 })
		)
		.addBooleanOption((option) =>
			option
				.setName(HARD_FEARLESS_OPTION)
				.setDescription('이전 세트 픽을 다시 사용할 수 없게 합니다.')
		),

	async execute(interaction, { api, config }) {
		const guildError = guildInteractionError(interaction);
		if (guildError) {
			await interaction.reply(
				botEphemeralReply(
					botNotice({ tone: 'error', title: '내전 모집을 시작할 수 없습니다.', body: guildError })
				)
			);
			return;
		}
		if (!interaction.inCachedGuild()) return;

		await interaction.deferReply(botDeferOptions(false));
		const actor = actorFromInteraction(interaction);
		const bestOf = selectedBestOf(interaction.options.getInteger(BEST_OF_OPTION));
		const hardFearless =
			interaction.options.getBoolean(HARD_FEARLESS_OPTION) ?? DEFAULT_HARD_FEARLESS;
		const snapshot = await api.createRecruitment(
			interaction.guildId,
			{ actor, bestOf, hardFearless },
			{ idempotencyKey: interaction.id }
		);
		const rendered = renderRecruitmentMessage(snapshot, config.webOrigin);
		const message = await interaction.editReply(botEditReply(...rendered.components));

		await api.bindRecruitmentMessage(
			interaction.guildId,
			snapshot.recruitment.id,
			{
				actor,
				channelId: message.channelId,
				messageId: message.id
			},
			{ idempotencyKey: interaction.id }
		);
	}
};

function selectedBestOf(value: number | null): 1 | 3 | 5 {
	if (value === 1 || value === 5) return value;
	return DEFAULT_BEST_OF;
}
