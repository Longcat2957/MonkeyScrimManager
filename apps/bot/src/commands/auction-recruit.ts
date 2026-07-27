import {
	ActionRowBuilder,
	ApplicationIntegrationType,
	ButtonBuilder,
	ButtonStyle,
	InteractionContextType,
	SlashCommandBuilder,
	type ChatInputCommandInteraction
} from 'discord.js';

import type { AuctionBestOf, AuctionRecruitmentFormat } from '../api/contracts.js';
import { actorFromInteraction } from '../discord/actor.js';
import {
	botDeferOptions,
	botEditReply,
	botEphemeralReply,
	botNotice
} from '../discord/message-v2.js';
import type { InteractionContext } from '../interactions/context.js';
import { guildInteractionError } from '../interactions/guild-context.js';
import {
	defaultExpectedStartAt,
	expectedStartAtFromKstInput,
	expectedStartInputExample
} from './expected-start-at.js';
import type { BotCommand } from './types.js';

const TEN_PERSON_BEST_OF_OPTION = '형식';
const SEMIFINAL_BEST_OF_OPTION = '4강';
const FINAL_BEST_OF_OPTION = '결승';
const HARD_FEARLESS_OPTION = '하드피어리스';
const EXPECTED_START_OPTION = '예상시작';
const DEFAULT_BEST_OF: AuctionBestOf = 3;
const DEFAULT_HARD_FEARLESS = true;

export const auctionRecruitTenCommand = auctionRecruitCommand('경매내전_10인', 'TEN_PERSON');
export const auctionRecruitTwentyCommand = auctionRecruitCommand('경매내전_20인', 'TWENTY_PERSON');

function auctionRecruitCommand(name: string, format: AuctionRecruitmentFormat): BotCommand {
	return {
		data: commandData(name, format),

		async execute(interaction, context) {
			await executeAuctionRecruitment(interaction, context, format);
		}
	};
}

function commandData(name: string, format: AuctionRecruitmentFormat): BotCommand['data'] {
	const builder = new SlashCommandBuilder()
		.setName(name)
		.setDescription(
			format === 'TEN_PERSON'
				? '10인 2팀 경매 내전 참가자를 모집합니다.'
				: '20인 4팀 경매 내전 참가자를 모집합니다.'
		)
		.setIntegrationTypes(ApplicationIntegrationType.GuildInstall)
		.setContexts(InteractionContextType.Guild)
		.addStringOption((option) =>
			option
				.setName(EXPECTED_START_OPTION)
				.setDescription(`예상 시작 시각 (KST, 비우면 30분 후 · 예: ${expectedStartInputExample})`)
		);

	if (format === 'TEN_PERSON') {
		return builder
			.addIntegerOption((option) =>
				option
					.setName(TEN_PERSON_BEST_OF_OPTION)
					.setDescription('시리즈 형식')
					.addChoices(
						{ name: 'BO1', value: 1 },
						{ name: 'BO3', value: 3 },
						{ name: 'BO5', value: 5 }
					)
			)
			.addBooleanOption((option) =>
				option
					.setName(HARD_FEARLESS_OPTION)
					.setDescription('각 시리즈에서 이전 세트 픽을 다시 사용할 수 없게 합니다.')
			);
	}

	return builder
		.addIntegerOption((option) =>
			option
				.setName(SEMIFINAL_BEST_OF_OPTION)
				.setDescription('4강 시리즈 형식')
				.addChoices({ name: 'BO1', value: 1 }, { name: 'BO3', value: 3 }, { name: 'BO5', value: 5 })
		)
		.addIntegerOption((option) =>
			option
				.setName(FINAL_BEST_OF_OPTION)
				.setDescription('결승 시리즈 형식')
				.addChoices({ name: 'BO1', value: 1 }, { name: 'BO3', value: 3 }, { name: 'BO5', value: 5 })
		)
		.addBooleanOption((option) =>
			option
				.setName(HARD_FEARLESS_OPTION)
				.setDescription('각 시리즈에서 이전 세트 픽을 다시 사용할 수 없게 합니다.')
		);
}

async function executeAuctionRecruitment(
	interaction: ChatInputCommandInteraction,
	{ api, config }: InteractionContext,
	format: AuctionRecruitmentFormat
) {
	const guildError = guildInteractionError(interaction);
	if (guildError) {
		await interaction.reply(commandErrorReply(guildError));
		return;
	}
	if (!interaction.inCachedGuild()) return;

	const semifinalBestOf =
		format === 'TWENTY_PERSON'
			? selectedBestOf(interaction.options.getInteger(SEMIFINAL_BEST_OF_OPTION))
			: null;
	const finalBestOf = selectedBestOf(
		interaction.options.getInteger(
			format === 'TEN_PERSON' ? TEN_PERSON_BEST_OF_OPTION : FINAL_BEST_OF_OPTION
		)
	);
	const hardFearless =
		interaction.options.getBoolean(HARD_FEARLESS_OPTION) ?? DEFAULT_HARD_FEARLESS;
	const expectedStartInput = interaction.options.getString(EXPECTED_START_OPTION);
	const expectedStartAt =
		expectedStartInput === null
			? defaultExpectedStartAt(interaction.createdAt)
			: expectedStartAtFromKstInput(expectedStartInput);
	if (!expectedStartAt) {
		await interaction.reply(
			commandValidationReply(
				`예상 시작 시각은 KST 24시간 형식으로 입력해 주세요. 예: \`${expectedStartInputExample}\``
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
					...(format === 'TEN_PERSON'
						? [`**형식** BO${finalBestOf}`]
						: [`**4강** BO${semifinalBestOf}`, `**결승** BO${finalBestOf}`]),
					`**하드 피어리스** ${hardFearless ? '사용' : '미사용'}`,
					`**예상 시작** ${expectedStartAt}`
				]
			}),
			recruitmentAction
		)
	);
}

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

function formatLabel(format: AuctionRecruitmentFormat): string {
	return format === 'TEN_PERSON' ? '10인' : '20인';
}

function selectedBestOf(value: number | null): AuctionBestOf {
	if (value === 1 || value === 5) return value;
	return DEFAULT_BEST_OF;
}
