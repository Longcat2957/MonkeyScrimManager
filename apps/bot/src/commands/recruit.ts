import {
	ApplicationIntegrationType,
	InteractionContextType,
	MessageFlags,
	SlashCommandBuilder
} from 'discord.js';
import { actorFromInteraction } from '../discord/actor.js';
import { renderRecruitmentMessage } from '../discord/recruitment-message.js';
import { guildInteractionError } from '../interactions/guild-context.js';
import type { BotCommand } from './types.js';

export const recruitCommand: BotCommand = {
	data: new SlashCommandBuilder()
		.setName('내전모집')
		.setDescription('10명 5v5 내전 참가자를 모집합니다.')
		.setIntegrationTypes(ApplicationIntegrationType.GuildInstall)
		.setContexts(InteractionContextType.Guild),

	async execute(interaction, { api, config }) {
		const guildError = guildInteractionError(interaction);
		if (guildError) {
			await interaction.reply({
				content: guildError,
				flags: MessageFlags.Ephemeral
			});
			return;
		}
		if (!interaction.inCachedGuild()) return;

		await interaction.deferReply();
		const actor = actorFromInteraction(interaction);
		const snapshot = await api.createRecruitment(
			interaction.guildId,
			{ actor },
			{ idempotencyKey: interaction.id }
		);
		const message = await interaction.editReply(
			renderRecruitmentMessage(snapshot, config.webOrigin)
		);

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
