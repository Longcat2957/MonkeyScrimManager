import {
	ApplicationIntegrationType,
	InteractionContextType,
	SlashCommandBuilder
} from 'discord.js';

import { uuidSchema } from '../api/contracts.js';
import { actorFromInteraction } from '../discord/actor.js';
import {
	botDeferOptions,
	botEditReply,
	botEphemeralReply,
	botNotice
} from '../discord/message-v2.js';
import { guildInteractionError } from '../interactions/guild-context.js';
import type { BotCommand } from './types.js';

const RECRUITMENT_ID_OPTION = '모집';

export const auctionRecruitmentForceCancelCommand: BotCommand = {
	data: new SlashCommandBuilder()
		.setName('경매강제취소')
		.setDescription('[운영자] 경매 모집 또는 진행 중인 경매 내전을 강제 취소합니다.')
		.setIntegrationTypes(ApplicationIntegrationType.GuildInstall)
		.setContexts(InteractionContextType.Guild)
		.addStringOption((option) =>
			option
				.setName(RECRUITMENT_ID_OPTION)
				.setDescription('경매 모집 UUID')
				.setRequired(true)
				.setMinLength(36)
				.setMaxLength(36)
		),

	async execute(interaction, { api }) {
		const guildError = guildInteractionError(interaction);
		if (guildError) {
			await interaction.reply(commandErrorReply(guildError));
			return;
		}
		if (!interaction.inCachedGuild()) return;

		const recruitmentId = parseRecruitmentId(
			interaction.options.getString(RECRUITMENT_ID_OPTION, true)
		);
		if (!recruitmentId) {
			await interaction.reply(
				commandValidationReply('모집 ID는 경매 모집 페이지 URL에 있는 UUID 형식이어야 합니다.')
			);
			return;
		}

		await interaction.deferReply(botDeferOptions(true));
		const result = await api.forceCancelAuctionRecruitment(
			interaction.guildId,
			recruitmentId,
			{ actor: actorFromInteraction(interaction) },
			{ idempotencyKey: interaction.id }
		);
		const scope = result.tournamentId
			? '경매 모집과 연결된 경매 내전을 강제 취소했습니다.'
			: '경매 모집을 강제 취소했습니다.';
		await interaction.editReply(
			botEditReply(
				botNotice({
					tone: 'warning',
					title: '경매 모집 강제 취소 완료',
					body: scope,
					details:
						result.reversedGameCount > 0
							? [`**되돌린 세트** ${result.reversedGameCount}개`, '**MMR 반영** 되돌림']
							: undefined
				})
			)
		);
	}
};

function commandErrorReply(message: string) {
	return botEphemeralReply(
		botNotice({ tone: 'error', title: '명령어를 사용할 수 없습니다', body: message })
	);
}

function commandValidationReply(message: string) {
	return botEphemeralReply(
		botNotice({ tone: 'warning', title: '경매 모집을 취소할 수 없습니다', body: message })
	);
}

function parseRecruitmentId(value: string): string | null {
	const recruitmentId = value.trim().toLowerCase();
	return uuidSchema.safeParse(recruitmentId).success ? recruitmentId : null;
}
