import {
	ApplicationIntegrationType,
	InteractionContextType,
	MessageFlags,
	SlashCommandBuilder
} from 'discord.js';

import { uuidSchema } from '../api/contracts.js';
import { actorFromInteraction } from '../discord/actor.js';
import { guildInteractionError } from '../interactions/guild-context.js';
import type { BotCommand } from './types.js';

const TOURNAMENT_ID_OPTION = '토너먼트';
const REASON_OPTION = '사유';

export const auctionTournamentCancelCommand: BotCommand = {
	data: new SlashCommandBuilder()
		.setName('경매조기종료')
		.setDescription('[운영자] 진행 중인 경매 내전을 조기 종료합니다.')
		.setIntegrationTypes(ApplicationIntegrationType.GuildInstall)
		.setContexts(InteractionContextType.Guild)
		.addStringOption((option) =>
			option
				.setName(TOURNAMENT_ID_OPTION)
				.setDescription('조기 종료할 경매 토너먼트 UUID')
				.setRequired(true)
				.setMinLength(36)
				.setMaxLength(36)
		)
		.addStringOption((option) =>
			option
				.setName(REASON_OPTION)
				.setDescription('조기 종료 사유')
				.setRequired(true)
				.setMinLength(1)
				.setMaxLength(240)
		),

	async execute(interaction, { api }) {
		const guildError = guildInteractionError(interaction);
		if (guildError) {
			await interaction.reply({ content: guildError, flags: MessageFlags.Ephemeral });
			return;
		}
		if (!interaction.inCachedGuild()) return;

		const tournamentId = parseTournamentId(
			interaction.options.getString(TOURNAMENT_ID_OPTION, true)
		);
		if (!tournamentId) {
			await interaction.reply({
				content: '토너먼트 ID는 경매 내전 페이지 URL에 있는 UUID 형식이어야 합니다.',
				flags: MessageFlags.Ephemeral
			});
			return;
		}

		const reason = interaction.options.getString(REASON_OPTION, true).trim();
		if (!reason || [...reason].length > 240) {
			await interaction.reply({
				content: '조기 종료 사유는 공백이 아닌 1~240자로 입력해 주세요.',
				flags: MessageFlags.Ephemeral
			});
			return;
		}

		await interaction.deferReply({ flags: MessageFlags.Ephemeral });
		const preview = await api.getAuctionTournamentCancelPreview(interaction.guildId, tournamentId);
		const result = await api.cancelAuctionTournament(
			interaction.guildId,
			tournamentId,
			{
				actor: actorFromInteraction(interaction),
				expectedRevision: preview.tournament.revision,
				reason
			},
			{ idempotencyKey: interaction.id }
		);

		const reversedGames =
			result.reversedGameCount > 0
				? ` 확정된 세트 ${result.reversedGameCount}개와 MMR 반영을 되돌렸습니다.`
				: '';
		await interaction.editReply(
			`경매 내전 #${preview.tournament.displayNumber}을 조기 종료했습니다.${reversedGames}`
		);
	}
};

function parseTournamentId(value: string): string | null {
	const tournamentId = value.trim().toLowerCase();
	return uuidSchema.safeParse(tournamentId).success ? tournamentId : null;
}
