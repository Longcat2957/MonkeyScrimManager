import { ActionRowBuilder, ButtonBuilder, ButtonStyle, escapeMarkdown } from 'discord.js';
import type { AuctionTournamentResultNotificationPayload } from '../api/contracts.js';
import { BOT_COLORS, botContainer, botReply, botSeparator, botText } from './message-v2.js';

const MATCH_FIELD_VALUE_LIMIT = 1_024;

export function renderAuctionTournamentResultMessage(
	payload: AuctionTournamentResultNotificationPayload,
	webOrigin: URL
) {
	const { tournament } = payload;
	const format = tournament.format === 'TEN_PERSON' ? '10인 경매 내전' : '20인 경매 내전';
	const semifinalRule =
		tournament.format === 'TWENTY_PERSON' && tournament.semifinalBestOf !== null
			? `4강 BO${tournament.semifinalBestOf} · `
			: '';
	const rules = `${semifinalRule}결승 BO${tournament.finalBestOf} · ${
		tournament.hardFearless ? '하드 피어리스' : '일반'
	}`;
	const championName = compactText(payload.champion.teamName, 100);
	const completedAt = new Date(payload.completedAt);
	const completedTimestamp = Number.isFinite(completedAt.getTime())
		? `\n완료 <t:${Math.floor(completedAt.getTime() / 1_000)}:R>`
		: '';

	return botReply(
		botContainer({
			color: BOT_COLORS.warning,
			children: [
				botText(
					`## 경매 내전 완료 #${tournament.displayNumber}\n**${escapeMarkdown(
						championName
					)}** 우승\n${escapeMarkdown(compactText(tournament.title, 200))}`
				),
				botSeparator(),
				botText(`**${format}** · ${rules}${completedTimestamp}`),
				botSeparator(),
				botText(`### 대진 결과\n${matchResults(payload)}`)
			]
		}),
		new ActionRowBuilder<ButtonBuilder>().addComponents(
			new ButtonBuilder()
				.setLabel('결과 보기')
				.setStyle(ButtonStyle.Link)
				.setURL(
					new URL(
						`/g/${payload.guildId}/auction/tournaments/${payload.tournamentId}/result`,
						webOrigin
					).toString()
				)
		)
	);
}

function matchResults(payload: AuctionTournamentResultNotificationPayload): string {
	const value = payload.matches
		.map((match) => {
			const label = compactText(match.label, 36);
			const leftTeamName = compactText(match.leftTeamName, 56);
			const rightTeamName = compactText(match.rightTeamName, 56);
			const winnerTeamName = compactText(match.winnerTeamName, 56);
			return `**${escapeMarkdown(label)} · BO${match.bestOf}**\n${escapeMarkdown(
				leftTeamName
			)} ${match.leftScore} : ${match.rightScore} ${escapeMarkdown(rightTeamName)} · **${escapeMarkdown(
				winnerTeamName
			)} 승리**`;
		})
		.join('\n');
	return value.length <= MATCH_FIELD_VALUE_LIMIT
		? value
		: `${value.slice(0, MATCH_FIELD_VALUE_LIMIT - 1)}…`;
}

function compactText(value: string, limit: number): string {
	const normalized = value.replace(/\s+/g, ' ').trim() || '이름 미설정';
	return normalized.length <= limit ? normalized : `${normalized.slice(0, limit - 1)}…`;
}
