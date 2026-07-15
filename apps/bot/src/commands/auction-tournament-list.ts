import {
	ActionRowBuilder,
	ApplicationIntegrationType,
	ButtonBuilder,
	ButtonStyle,
	escapeMarkdown,
	InteractionContextType,
	SlashCommandBuilder
} from 'discord.js';

import {
	uuidSchema,
	type AuctionTournamentListData,
	type AuctionTournamentListRequest
} from '../api/contracts.js';
import {
	BOT_COLORS,
	botContainer,
	botDeferOptions,
	botEditReply,
	botEphemeralReply,
	botNotice,
	botSeparator,
	botText
} from '../discord/message-v2.js';
import { guildInteractionError } from '../interactions/guild-context.js';
import type { BotCommand } from './types.js';

const STATE_OPTION = '상태';
const SEASON_OPTION = '시즌';
const LIMIT_OPTION = 'limit';
const MAX_VISIBLE_TOURNAMENTS = 10;
const MAX_TOURNAMENT_TITLE_LENGTH = 64;

export const auctionTournamentListCommand: BotCommand = {
	data: new SlashCommandBuilder()
		.setName('경매목록')
		.setDescription('경매 내전 목록을 조회합니다.')
		.setIntegrationTypes(ApplicationIntegrationType.GuildInstall)
		.setContexts(InteractionContextType.Guild)
		.addStringOption((option) =>
			option
				.setName(STATE_OPTION)
				.setDescription('조회할 상태')
				.addChoices({ name: '진행 중', value: 'active' }, { name: '완료', value: 'completed' })
		)
		.addStringOption((option) =>
			option.setName(SEASON_OPTION).setDescription('시즌 UUID').setMinLength(36).setMaxLength(36)
		)
		.addIntegerOption((option) =>
			option
				.setName(LIMIT_OPTION)
				.setDescription(`표시할 최대 개수 (1~${MAX_VISIBLE_TOURNAMENTS})`)
				.setMinValue(1)
				.setMaxValue(MAX_VISIBLE_TOURNAMENTS)
		),

	async execute(interaction, { api, config }) {
		const guildError = guildInteractionError(interaction);
		if (guildError) {
			await interaction.reply(commandErrorReply(guildError));
			return;
		}
		if (!interaction.inCachedGuild()) return;

		const seasonId = parseSeasonId(interaction.options.getString(SEASON_OPTION));
		if (interaction.options.getString(SEASON_OPTION) !== null && !seasonId) {
			await interaction.reply(commandValidationReply('시즌 ID는 UUID 형식이어야 합니다.'));
			return;
		}

		const input: AuctionTournamentListRequest = {
			state: selectedState(interaction.options.getString(STATE_OPTION)),
			limit: interaction.options.getInteger(LIMIT_OPTION) ?? MAX_VISIBLE_TOURNAMENTS,
			...(seasonId ? { seasonId } : {})
		};
		await interaction.deferReply(botDeferOptions(true));
		const page = await api.listAuctionTournaments(interaction.guildId, input);
		const dashboardUrl = new URL(`/g/${interaction.guildId}/auction`, config.webOrigin).toString();
		await interaction.editReply(
			botEditReply(...renderTournamentList(page, input.state, dashboardUrl, config.webOrigin))
		);
	}
};

function selectedState(value: string | null): AuctionTournamentListRequest['state'] {
	return value === 'completed' ? 'completed' : 'active';
}

function parseSeasonId(value: string | null): string | null {
	if (value === null) return null;
	const seasonId = value.trim().toLowerCase();
	return uuidSchema.safeParse(seasonId).success ? seasonId : null;
}

function renderTournamentList(
	page: AuctionTournamentListData,
	state: AuctionTournamentListRequest['state'],
	dashboardUrl: string,
	webOrigin: URL
): [ReturnType<typeof botContainer>, ActionRowBuilder<ButtonBuilder>] {
	const label = state === 'active' ? '진행 중' : '완료';
	const dashboardAction = new ActionRowBuilder<ButtonBuilder>().addComponents(
		new ButtonBuilder().setStyle(ButtonStyle.Link).setLabel('경매 내전 열기').setURL(dashboardUrl)
	);
	if (page.tournaments.length === 0) {
		return [
			botNotice({
				tone: 'info',
				title: `${label} 경매 내전`,
				body: '조회할 경매 내전이 없습니다.'
			}),
			dashboardAction
		];
	}

	const lines = page.tournaments.map((tournament) => {
		const href = new URL(tournament.href, webOrigin).toString();
		return [
			`### #${tournament.displayNumber} ${escapeMarkdown(truncate(tournament.title))}`,
			`${statusLabel(tournament.status)} · [작업 열기](${href})`
		].join('\n');
	});
	const children = [
		botText(`## ${label} 경매 내전\n현재 ${page.tournaments.length}개를 표시합니다.`),
		botSeparator(),
		botText(lines.join('\n\n'))
	];
	if (page.nextCursor) {
		children.push(botSeparator(), botText('추가 항목은 경매 내전 화면에서 확인할 수 있습니다.'));
	}
	return [botContainer({ color: BOT_COLORS.info, children }), dashboardAction];
}

function truncate(value: string): string {
	const characters = [...value];
	return characters.length > MAX_TOURNAMENT_TITLE_LENGTH
		? `${characters.slice(0, MAX_TOURNAMENT_TITLE_LENGTH - 3).join('')}...`
		: value;
}

function statusLabel(status: AuctionTournamentListData['tournaments'][number]['status']) {
	switch (status) {
		case 'BIDDING':
			return '경매 진행';
		case 'BRACKET_SETUP':
			return '대진 설정';
		case 'IN_GAME':
			return '경기 진행';
		case 'COMPLETED':
			return '완료';
		case 'CANCELLED':
			return '취소';
	}
}

function commandErrorReply(message: string) {
	return botEphemeralReply(
		botNotice({ tone: 'error', title: '명령어를 사용할 수 없습니다', body: message })
	);
}

function commandValidationReply(message: string) {
	return botEphemeralReply(
		botNotice({ tone: 'warning', title: '경매 내전을 조회할 수 없습니다', body: message })
	);
}
