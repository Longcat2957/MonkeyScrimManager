import {
	ApplicationIntegrationType,
	InteractionContextType,
	MessageFlags,
	PermissionFlagsBits,
	SlashCommandBuilder
} from 'discord.js';
import { actorFromInteraction } from '../discord/actor.js';
import { guildInteractionError } from '../interactions/guild-context.js';
import type { BotCommand } from './types.js';

export const setupCommand: BotCommand = {
	data: new SlashCommandBuilder()
		.setName('초기설정')
		.setDescription('현재 Discord 서버를 MonkeyBot 길드로 설정합니다.')
		.setIntegrationTypes(ApplicationIntegrationType.GuildInstall)
		.setContexts(InteractionContextType.Guild)
		.setDefaultMemberPermissions(PermissionFlagsBits.ManageGuild)
		.addRoleOption((option) =>
			option
				.setName('운영자역할')
				.setDescription('길드 운영 권한으로 사용할 Discord 역할')
				.setRequired(true)
		)
		.addStringOption((option) =>
			option.setName('길드이름').setDescription('MonkeyBot에 표시할 길드 이름')
		),

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

		await interaction.deferReply({ flags: MessageFlags.Ephemeral });
		const operatorRole = interaction.options.getRole('운영자역할', true);
		if (operatorRole.id === interaction.guildId) {
			await interaction.editReply('@everyone 역할은 운영자 역할로 사용할 수 없습니다.');
			return;
		}
		const requestedName = interaction.options.getString('길드이름')?.trim();
		const actor = actorFromInteraction(interaction);

		await api.setupGuild(
			interaction.guildId,
			{
				actor,
				guildName: requestedName || interaction.guild.name,
				guildIconHash: interaction.guild.icon,
				operatorRoleId: operatorRole.id,
				seasonName: '시즌 1'
			},
			{ idempotencyKey: interaction.id }
		);

		const dashboardUrl = new URL(`/g/${interaction.guildId}`, config.webOrigin);
		await interaction.editReply(
			`초기 설정을 완료했습니다. 길드 운영 화면: ${dashboardUrl.toString()}`
		);
	}
};
