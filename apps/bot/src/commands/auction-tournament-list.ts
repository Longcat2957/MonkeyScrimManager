import {
	ApplicationIntegrationType,
	InteractionContextType,
	MessageFlags,
	SlashCommandBuilder
} from 'discord.js';

import {
	uuidSchema,
	type AuctionTournamentListData,
	type AuctionTournamentListRequest
} from '../api/contracts.js';
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
			await interaction.reply({ content: guildError, flags: MessageFlags.Ephemeral });
			return;
		}
		if (!interaction.inCachedGuild()) return;

		const seasonId = parseSeasonId(interaction.options.getString(SEASON_OPTION));
		if (interaction.options.getString(SEASON_OPTION) !== null && !seasonId) {
			await interaction.reply({
				content: '시즌 ID는 UUID 형식이어야 합니다.',
				flags: MessageFlags.Ephemeral
			});
			return;
		}

		const input: AuctionTournamentListRequest = {
			state: selectedState(interaction.options.getString(STATE_OPTION)),
			limit: interaction.options.getInteger(LIMIT_OPTION) ?? MAX_VISIBLE_TOURNAMENTS,
			...(seasonId ? { seasonId } : {})
		};
		await interaction.deferReply({ flags: MessageFlags.Ephemeral });
		const page = await api.listAuctionTournaments(interaction.guildId, input);
		const dashboardUrl = new URL(`/g/${interaction.guildId}/auction`, config.webOrigin).toString();
		await interaction.editReply(
			renderTournamentList(page, input.state, dashboardUrl, config.webOrigin)
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
): string {
	const label = state === 'active' ? '진행 중' : '완료';
	if (page.tournaments.length === 0) {
		return `${label} 경매 내전이 없습니다.\n전체 보기: ${dashboardUrl}`;
	}

	const lines = page.tournaments.map((tournament) => {
		const href = new URL(tournament.href, webOrigin).toString();
		return `- #${tournament.displayNumber} ${truncate(tournament.title)} · ${statusLabel(tournament.status)} · ${href}`;
	});
	const continuation = page.nextCursor ? '\n추가 항목은 경매 페이지에서 확인할 수 있습니다.' : '';
	return `${label} 경매 내전\n${lines.join('\n')}${continuation}\n전체 보기: ${dashboardUrl}`;
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
