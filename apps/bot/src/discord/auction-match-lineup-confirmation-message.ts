import { Resvg } from '@resvg/resvg-js';
import { ActionRowBuilder, AttachmentBuilder, ButtonBuilder, ButtonStyle } from 'discord.js';

import type { AuctionMatchLineupConfirmationSnapshot } from '../api/contracts.js';

const WIDTH = 1200;
const HEIGHT = 720;
const PANEL_Y = 194;
const PANEL_WIDTH = 548;
const PANEL_HEIGHT = 448;
const TEAM_1_COLOR = '#38bdf8';
const TEAM_2_COLOR = '#fb7185';

const LANE_LABELS = {
	TOP: '탑',
	JUNGLE: '정글',
	MID: '미드',
	BOTTOM: '원딜',
	SUPPORT: '서폿'
} as const;

type AuctionLineupTeam = AuctionMatchLineupConfirmationSnapshot['teams'][number];

export function renderAuctionMatchLineupConfirmationMessage(
	snapshot: AuctionMatchLineupConfirmationSnapshot,
	webOrigin: URL
) {
	const imageName = `auction-${snapshot.tournament.displayNumber}-${snapshot.match.matchKey.toLowerCase()}-lineup.png`;
	const attachment = new AttachmentBuilder(renderAuctionMatchLineupConfirmationPng(snapshot), {
		name: imageName,
		description: `${snapshot.tournament.title} ${snapshot.match.label} 라인업 확정`
	});

	return {
		files: [attachment],
		components: [
			new ActionRowBuilder<ButtonBuilder>().addComponents(
				new ButtonBuilder()
					.setLabel('픽/밴 입력 열기')
					.setStyle(ButtonStyle.Link)
					.setURL(new URL(snapshot.links.pickBan, webOrigin).toString())
			)
		]
	};
}

export function renderAuctionMatchLineupConfirmationPng(
	snapshot: AuctionMatchLineupConfirmationSnapshot
): Buffer {
	return new Resvg(renderAuctionMatchLineupConfirmationSvg(snapshot), {
		font: {
			loadSystemFonts: true,
			defaultFontFamily: 'Noto Sans CJK KR'
		}
	})
		.render()
		.asPng();
}

export function renderAuctionMatchLineupConfirmationSvg(
	snapshot: AuctionMatchLineupConfirmationSnapshot
): string {
	const [leftTeam, rightTeam] = snapshot.teams;
	if (!leftTeam || !rightTeam) {
		throw new Error('Auction lineup confirmation snapshot must include both teams');
	}
	const format = snapshot.tournament.format === 'TEN_PERSON' ? '10인 경매 내전' : '20인 경매 내전';
	const rule = snapshot.tournament.hardFearless ? '하드 피어리스' : '일반';

	return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="${WIDTH}" height="${HEIGHT}" viewBox="0 0 ${WIDTH} ${HEIGHT}" font-family="'Noto Sans CJK KR','Noto Sans KR',sans-serif">
  <rect width="${WIDTH}" height="${HEIGHT}" fill="#0b1018"/>
  <rect x="24" y="24" width="1152" height="672" rx="14" fill="#111a26" stroke="#273244"/>
  <rect x="24" y="24" width="1152" height="150" rx="14" fill="#0e1520"/>
  <path d="M24 160h1152v14H24z" fill="#0e1520"/>

  <text x="58" y="67" font-size="17" font-weight="700" letter-spacing="1.1" fill="#22d3ee">MONKEYBOT</text>
  <text x="58" y="100" font-size="30" font-weight="700" fill="#f3f6fb">${escapeXml(truncate(snapshot.tournament.title, 26))}</text>
  <text x="58" y="132" font-size="16" fill="#9aa8ba">${escapeXml(format)} · ${escapeXml(truncate(snapshot.match.label, 22))} 라인업 확정 · 픽/밴을 시작할 수 있습니다.</text>

  <rect x="868" y="57" width="104" height="38" rx="8" fill="#131e2c" stroke="#273244"/>
  <text x="920" y="82" text-anchor="middle" font-size="17" font-weight="700" fill="#f3f6fb">BO${snapshot.match.bestOf}</text>
  <rect x="986" y="57" width="152" height="38" rx="8" fill="#131e2c" stroke="#273244"/>
  <text x="1062" y="82" text-anchor="middle" font-size="14" font-weight="700" fill="#d7e0eb">${rule}</text>
  <text x="1138" y="132" text-anchor="end" font-size="14" fill="#6f8198">AUCTION #${snapshot.tournament.displayNumber}</text>

  ${renderTeamPanel(leftTeam, 48, TEAM_1_COLOR, 'BLUE')}
  ${renderTeamPanel(rightTeam, 604, TEAM_2_COLOR, 'RED')}

  <line x1="48" y1="666" x2="1152" y2="666" stroke="#202a3a"/>
  <text x="48" y="684" font-size="13" fill="#6f8198">확정된 경매 라인업 기준</text>
  <text x="1152" y="684" text-anchor="end" font-size="13" fill="#6f8198">monkeybot.lol</text>
</svg>`;
}

function renderTeamPanel(
	team: AuctionLineupTeam,
	x: number,
	color: string,
	side: 'BLUE' | 'RED'
): string {
	const rows = team.players
		.map((player, index) => {
			const y = PANEL_Y + 76 + index * 68;
			return `<rect x="${x + 16}" y="${y}" width="${PANEL_WIDTH - 32}" height="56" rx="8" fill="#0e1520" stroke="#202a3a"/>
  <rect x="${x + 30}" y="${y + 12}" width="58" height="32" rx="6" fill="${color}" fill-opacity="0.14"/>
  <text x="${x + 59}" y="${y + 33}" text-anchor="middle" font-size="14" font-weight="700" fill="${color}">${LANE_LABELS[player.lane]}</text>
  <text x="${x + 108}" y="${y + 34}" font-size="18" font-weight="600" fill="#f3f6fb">${escapeXml(truncate(player.displayName, 22))}</text>`;
		})
		.join('\n');

	return `<rect x="${x}" y="${PANEL_Y}" width="${PANEL_WIDTH}" height="${PANEL_HEIGHT}" rx="12" fill="#111a26" stroke="#273244"/>
  <rect x="${x}" y="${PANEL_Y}" width="${PANEL_WIDTH}" height="6" rx="3" fill="${color}"/>
  <text x="${x + 28}" y="${PANEL_Y + 43}" font-size="22" font-weight="700" fill="#f3f6fb">${escapeXml(truncate(team.name, 18))}</text>
  <text x="${x + PANEL_WIDTH - 28}" y="${PANEL_Y + 43}" text-anchor="end" font-size="14" font-weight="700" letter-spacing="0.8" fill="${color}">${side} SIDE</text>
  <line x1="${x + 16}" y1="${PANEL_Y + 60}" x2="${x + PANEL_WIDTH - 16}" y2="${PANEL_Y + 60}" stroke="#202a3a"/>
  ${rows}`;
}

function truncate(value: string, maximumCharacters: number): string {
	const characters = [...value.trim()];
	return characters.length > maximumCharacters
		? `${characters.slice(0, Math.max(0, maximumCharacters - 3)).join('')}...`
		: characters.join('');
}

function escapeXml(value: string): string {
	return value.replace(/[&<>"']/g, (character) => {
		switch (character) {
			case '&':
				return '&amp;';
			case '<':
				return '&lt;';
			case '>':
				return '&gt;';
			case '"':
				return '&quot;';
			case "'":
				return '&apos;';
			default:
				return character;
		}
	});
}
