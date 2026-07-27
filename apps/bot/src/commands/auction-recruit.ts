import {
	ActionRowBuilder,
	ApplicationIntegrationType,
	ButtonBuilder,
	ButtonStyle,
	InteractionContextType,
	SlashCommandBuilder
} from 'discord.js';
import { z } from 'zod';

import { actorFromInteraction } from '../discord/actor.js';
import {
	botDeferOptions,
	botEditReply,
	botEphemeralReply,
	botNotice
} from '../discord/message-v2.js';
import { guildInteractionError } from '../interactions/guild-context.js';
import type { AuctionBestOf, AuctionRecruitmentFormat } from '../api/contracts.js';
import type { BotCommand } from './types.js';

const PARTICIPANT_COUNT_OPTION = '정원';
const SEMIFINAL_BEST_OF_OPTION = '4강';
const FINAL_BEST_OF_OPTION = '결승';
const HARD_FEARLESS_OPTION = '하드피어리스';
const EXPECTED_START_OPTION = '예상시작';
const DEFAULT_BEST_OF: AuctionBestOf = 3;
const DEFAULT_HARD_FEARLESS = true;
const expectedStartAtSchema = z.iso.datetime({ offset: true });

export const auctionRecruitCommand: BotCommand = {
	data: new SlashCommandBuilder()
		.setName('경매모집')
		.setDescription('10인 또는 20인 경매 내전 참가자를 모집합니다.')
		.setIntegrationTypes(ApplicationIntegrationType.GuildInstall)
		.setContexts(InteractionContextType.Guild)
		.addIntegerOption((option) =>
			option
				.setName(PARTICIPANT_COUNT_OPTION)
				.setDescription('경매 내전 참가 정원')
				.setRequired(true)
				.addChoices({ name: '10인', value: 10 }, { name: '20인', value: 20 })
		)
		.addStringOption((option) =>
			option
				.setName(EXPECTED_START_OPTION)
				.setDescription('예상 시작 시각 (예: 2026-07-26T20:00:00+09:00)')
				.setRequired(true)
		)
		.addIntegerOption((option) =>
			option
				.setName(FINAL_BEST_OF_OPTION)
				.setDescription('결승 시리즈 형식')
				.addChoices({ name: 'BO1', value: 1 }, { name: 'BO3', value: 3 }, { name: 'BO5', value: 5 })
		)
		.addIntegerOption((option) =>
			option
				.setName(SEMIFINAL_BEST_OF_OPTION)
				.setDescription('20인 경매 내전 4강 시리즈 형식')
				.addChoices({ name: 'BO1', value: 1 }, { name: 'BO3', value: 3 }, { name: 'BO5', value: 5 })
		)
		.addBooleanOption((option) =>
			option
				.setName(HARD_FEARLESS_OPTION)
				.setDescription('각 시리즈에서 이전 세트 픽을 다시 사용할 수 없게 합니다.')
		),

	async execute(interaction, { api, config }) {
		const guildError = guildInteractionError(interaction);
		if (guildError) {
			await interaction.reply(commandErrorReply(guildError));
			return;
		}
		if (!interaction.inCachedGuild()) return;

		const format = auctionFormat(interaction.options.getInteger(PARTICIPANT_COUNT_OPTION, true));
		if (!format) {
			await interaction.reply(
				commandValidationReply('경매 모집 정원은 10명 또는 20명만 선택할 수 있습니다.')
			);
			return;
		}

		const semifinalInput = interaction.options.getInteger(SEMIFINAL_BEST_OF_OPTION);
		if (format === 'TEN_PERSON' && semifinalInput !== null) {
			await interaction.reply(
				commandValidationReply('10인 경매 내전은 4강이 없으므로 `4강` 형식을 설정할 수 없습니다.')
			);
			return;
		}
		const semifinalBestOf = format === 'TWENTY_PERSON' ? selectedBestOf(semifinalInput) : null;
		const finalBestOf = selectedBestOf(interaction.options.getInteger(FINAL_BEST_OF_OPTION));
		const hardFearless =
			interaction.options.getBoolean(HARD_FEARLESS_OPTION) ?? DEFAULT_HARD_FEARLESS;
		const expectedStartAt = selectedExpectedStartAt(
			interaction.options.getString(EXPECTED_START_OPTION, true)
		);
		if (!expectedStartAt) {
			await interaction.reply(
				commandValidationReply(
					'예상 시작 시각은 시간대가 포함된 ISO 형식으로 입력해 주세요. 예: `2026-07-26T20:00:00+09:00`'
				)
			);
			return;
		}

		await interaction.deferReply(botDeferOptions(true));
		const snapshot = await api.createAuctionRecruitment(
			interaction.guildId,
			{
				actor: actorFromInteraction(interaction),
				format,
				semifinalBestOf,
				finalBestOf,
				hardFearless,
				expectedStartAt
			},
			{ idempotencyKey: interaction.id }
		);
		const recruitmentUrl = new URL(snapshot.recruitment.href, config.webOrigin);
		const recruitmentAction = new ActionRowBuilder<ButtonBuilder>().addComponents(
			new ButtonBuilder()
				.setStyle(ButtonStyle.Link)
				.setLabel('모집 화면 열기')
				.setURL(recruitmentUrl.toString())
		);
		await interaction.editReply(
			botEditReply(
				botNotice({
					tone: 'success',
					title: `${formatLabel(format)} 경매 모집 생성`,
					body: `${formatLabel(format)} 경매 모집을 생성했습니다.`,
					details: [
						`**결승** BO${finalBestOf}`,
						...(format === 'TWENTY_PERSON' && semifinalBestOf !== null
							? [`**4강** BO${semifinalBestOf}`]
							: []),
						`**하드 피어리스** ${hardFearless ? '사용' : '미사용'}`,
						`**예상 시작** ${expectedStartAt}`
					]
				}),
				recruitmentAction
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
		botNotice({ tone: 'warning', title: '경매 모집을 만들 수 없습니다', body: message })
	);
}

function auctionFormat(participantCount: number): AuctionRecruitmentFormat | null {
	if (participantCount === 10) return 'TEN_PERSON';
	if (participantCount === 20) return 'TWENTY_PERSON';
	return null;
}

function formatLabel(format: AuctionRecruitmentFormat): string {
	return format === 'TEN_PERSON' ? '10인' : '20인';
}

function selectedBestOf(value: number | null): AuctionBestOf {
	if (value === 1 || value === 5) return value;
	return DEFAULT_BEST_OF;
}

function selectedExpectedStartAt(value: string | null): string | null {
	const trimmed = value?.trim() ?? '';
	return expectedStartAtSchema.safeParse(trimmed).success ? trimmed : null;
}
