import { EmbedBuilder, escapeMarkdown } from 'discord.js';
import type { AuctionTournamentResultNotificationPayload } from '../api/contracts.js';

const MATCH_FIELD_VALUE_LIMIT = 1_024;

export function renderAuctionTournamentResultMessage(
	payload: AuctionTournamentResultNotificationPayload
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
	const embed = new EmbedBuilder()
		.setColor(0xfbbf24)
		.setTitle(`경매 내전 완료 #${tournament.displayNumber}`)
		.setDescription(
			`**${escapeMarkdown(championName)}** 우승\n${escapeMarkdown(compactText(tournament.title, 200))}`
		)
		.addFields(
			{ name: '우승팀', value: escapeMarkdown(championName), inline: true },
			{ name: '형식', value: format, inline: true },
			{ name: '규칙', value: rules, inline: true },
			{ name: '대진 결과', value: matchResults(payload) }
		)
		.setTimestamp(new Date(payload.completedAt))
		.setFooter({ text: 'MonkeyBot' });

	return { embeds: [embed] };
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
