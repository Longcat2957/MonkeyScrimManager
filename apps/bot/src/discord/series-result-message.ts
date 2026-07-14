import { Resvg } from '@resvg/resvg-js';
import { ActionRowBuilder, AttachmentBuilder, ButtonBuilder, ButtonStyle } from 'discord.js';

import type {
	SeriesResultNotificationSnapshot,
	SeriesResultNotificationV1Snapshot,
	SeriesResultNotificationV2Snapshot
} from '../api/contracts.js';
import { championImageHref, resolveChampionImages } from './champion-images.js';

const WIDTH = 1200;
const HEADER_HEIGHT = 146;
const SCORE_Y = 194;
const SCORE_HEIGHT = 124;
const SET_HEADING_Y = 356;
const SET_ROWS_Y = 376;
const SET_ROW_HEIGHT = 50;
const FOOTER_HEIGHT = 48;
const TEAM_1_COLOR = '#38bdf8';
const TEAM_2_COLOR = '#fb7185';
const DRAW_COLOR = '#cbd5e1';
const SURFACE = '#111a26';
const SURFACE_STRONG = '#0e1520';
const BORDER = '#273244';
const BORDER_MUTED = '#202a3a';
const TEXT = '#f3f6fb';
const TEXT_MUTED = '#9aa8ba';
const TEXT_SUBTLE = '#6f8198';

const FULL_ROSTER_Y = 350;
const FULL_ROSTER_HEIGHT = 100;
const FULL_SET_START_Y = 484;
const FULL_SET_HEIGHT = 200;
const FULL_SET_GAP = 14;
const FULL_FOOTER_HEIGHT = 56;
const PICK_TILE_SIZE = 58;
const PICK_TILE_GAP = 8;
const BAN_TILE_SIZE = 42;
const BAN_TILE_GAP = 6;

const LANE_LABELS = {
	TOP: '탑',
	JUNGLE: '정글',
	MID: '미드',
	BOTTOM: '원딜',
	SUPPORT: '서폿'
} as const;

export type SeriesResultImageResolver = (
	resvg: Pick<Resvg, 'imagesToResolve' | 'resolveImage'>
) => Promise<void>;

export interface SeriesResultMessageRenderOptions {
	resolveImages?: SeriesResultImageResolver;
}

export async function renderSeriesResultMessage(
	snapshot: SeriesResultNotificationSnapshot,
	webOrigin: URL,
	options: SeriesResultMessageRenderOptions = {}
) {
	const imageName = `series-${snapshot.series.displayNumber}-result.png`;
	const attachment = new AttachmentBuilder(await renderSeriesResultPng(snapshot, options), {
		name: imageName,
		description: `${snapshot.series.title} 결과 확정`
	});

	return {
		files: [attachment],
		components: [
			new ActionRowBuilder<ButtonBuilder>().addComponents(
				new ButtonBuilder()
					.setLabel('결과 보기')
					.setStyle(ButtonStyle.Link)
					.setURL(new URL(snapshot.links.result, webOrigin).toString())
			)
		]
	};
}

export async function renderSeriesResultPng(
	snapshot: SeriesResultNotificationSnapshot,
	options: SeriesResultMessageRenderOptions = {}
): Promise<Buffer> {
	const resvg = new Resvg(renderSeriesResultSvg(snapshot), {
		font: {
			loadSystemFonts: true,
			defaultFontFamily: 'Noto Sans CJK KR'
		}
	});

	try {
		await (options.resolveImages ?? resolveChampionImages)(resvg);
	} catch {
		// Artwork is optional; a result notification must still be deliverable.
	}

	return resvg.render().asPng();
}

export function renderSeriesResultSvg(snapshot: SeriesResultNotificationSnapshot): string {
	return isFullSeriesResultSnapshot(snapshot)
		? renderFullSeriesResultSvg(snapshot)
		: renderCompactSeriesResultSvg(snapshot);
}

function renderCompactSeriesResultSvg(snapshot: SeriesResultNotificationV1Snapshot): string {
	const height = compactCardHeight(snapshot);
	const terminal = terminalLabel(snapshot.series.terminalReason);
	const winner = winnerLabel(snapshot.series.winningTeam);
	const accent = outcomeColor(snapshot.series.winningTeam);
	const rule = snapshot.series.hardFearless ? '하드 피어리스' : '일반';
	const setRows = snapshot.games
		.map((game, index) =>
			renderCompactSetRow(game.setNumber, game.winner, SET_ROWS_Y + index * SET_ROW_HEIGHT)
		)
		.join('\n');
	const footerLineY = height - FOOTER_HEIGHT - 16;
	const footerTextY = height - 36;

	return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="${WIDTH}" height="${height}" viewBox="0 0 ${WIDTH} ${height}" font-family="'Noto Sans CJK KR','Noto Sans KR',sans-serif">
  <rect width="${WIDTH}" height="${height}" fill="#0b1018"/>
  <rect x="24" y="24" width="1152" height="${height - 48}" rx="14" fill="${SURFACE}" stroke="${BORDER}"/>
  <rect x="24" y="24" width="1152" height="${HEADER_HEIGHT}" rx="14" fill="${SURFACE_STRONG}"/>
  <path d="M24 ${HEADER_HEIGHT + 10}h1152v12H24z" fill="${SURFACE_STRONG}"/>

  <text x="58" y="67" font-size="17" font-weight="700" letter-spacing="1.1" fill="#22d3ee">MONKEYBOT</text>
  <text x="58" y="101" font-size="30" font-weight="700" fill="${TEXT}">${escapeXml(truncate(snapshot.series.title, 20))}</text>
  <text x="58" y="132" font-size="16" fill="${TEXT_MUTED}">결과 확정 · ${escapeXml(terminal)}</text>

  <rect x="730" y="56" width="182" height="38" rx="8" fill="#131e2c" stroke="${BORDER}"/>
  <text x="821" y="82" text-anchor="middle" font-size="14" font-weight="700" fill="#d7e0eb">${escapeXml(terminal)}</text>
  <rect x="928" y="56" width="82" height="38" rx="8" fill="#131e2c" stroke="${BORDER}"/>
  <text x="969" y="82" text-anchor="middle" font-size="17" font-weight="700" fill="${TEXT}">BO${snapshot.series.bestOf}</text>
  <rect x="1024" y="56" width="114" height="38" rx="8" fill="#131e2c" stroke="${BORDER}"/>
  <text x="1081" y="82" text-anchor="middle" font-size="14" font-weight="700" fill="#d7e0eb">${rule}</text>
  <text x="1138" y="132" text-anchor="end" font-size="14" fill="${TEXT_SUBTLE}">SERIES #${snapshot.series.displayNumber}</text>

  <rect x="48" y="${SCORE_Y}" width="1104" height="${SCORE_HEIGHT}" rx="12" fill="${SURFACE_STRONG}" stroke="${BORDER}"/>
  <rect x="48" y="${SCORE_Y}" width="6" height="${SCORE_HEIGHT}" rx="3" fill="${accent}"/>
  <text x="232" y="235" text-anchor="middle" font-size="17" font-weight="700" fill="${TEAM_1_COLOR}">1팀</text>
  <text x="232" y="286" text-anchor="middle" font-size="48" font-weight="700" fill="${TEXT}">${snapshot.score.team1}</text>
  <text x="600" y="266" text-anchor="middle" font-size="28" font-weight="600" fill="${TEXT_SUBTLE}">:</text>
  <text x="968" y="235" text-anchor="middle" font-size="17" font-weight="700" fill="${TEAM_2_COLOR}">2팀</text>
  <text x="968" y="286" text-anchor="middle" font-size="48" font-weight="700" fill="${TEXT}">${snapshot.score.team2}</text>
  <text x="600" y="235" text-anchor="middle" font-size="15" font-weight="700" letter-spacing="1.1" fill="${TEXT_SUBTLE}">최종 결과</text>
  <text x="600" y="292" text-anchor="middle" font-size="25" font-weight="700" fill="${accent}">${escapeXml(winner)}</text>

  <text x="48" y="${SET_HEADING_Y}" font-size="16" font-weight="700" fill="#d7e0eb">확정 세트 결과</text>
  ${setRows}

  <line x1="48" y1="${footerLineY}" x2="1152" y2="${footerLineY}" stroke="${BORDER_MUTED}"/>
  <text x="48" y="${footerTextY}" font-size="13" fill="${TEXT_SUBTLE}">확정된 세트 기준 · ${snapshot.games.length}세트</text>
  <text x="1152" y="${footerTextY}" text-anchor="end" font-size="13" fill="${TEXT_SUBTLE}">monkeybot.lol</text>
</svg>`;
}

function renderFullSeriesResultSvg(snapshot: SeriesResultNotificationV2Snapshot): string {
	const height = fullCardHeight(snapshot);
	const terminal = terminalLabel(snapshot.series.terminalReason);
	const winner = winnerLabel(snapshot.series.winningTeam);
	const accent = outcomeColor(snapshot.series.winningTeam);
	const rule = snapshot.series.hardFearless ? '하드 피어리스' : '일반';
	const team1 = teamFor(snapshot, 'TEAM_1');
	const team2 = teamFor(snapshot, 'TEAM_2');
	const setCards = snapshot.games
		.map((game, index) =>
			renderFullSetCard(game, FULL_SET_START_Y + index * (FULL_SET_HEIGHT + FULL_SET_GAP))
		)
		.join('\n');
	const footerLineY = height - FULL_FOOTER_HEIGHT;

	return `<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" width="${WIDTH}" height="${height}" viewBox="0 0 ${WIDTH} ${height}" font-family="'Noto Sans CJK KR','Noto Sans KR',sans-serif">
  <rect width="${WIDTH}" height="${height}" fill="#0b1018"/>
  <rect x="24" y="24" width="1152" height="${height - 48}" rx="14" fill="${SURFACE}" stroke="${BORDER}"/>
  <rect x="24" y="24" width="1152" height="${HEADER_HEIGHT}" rx="14" fill="${SURFACE_STRONG}"/>
  <path d="M24 ${HEADER_HEIGHT + 10}h1152v12H24z" fill="${SURFACE_STRONG}"/>

  <text x="58" y="67" font-size="17" font-weight="700" letter-spacing="1.1" fill="#22d3ee">MONKEYBOT</text>
  <text x="58" y="101" font-size="30" font-weight="700" fill="${TEXT}">${escapeXml(truncate(snapshot.series.title, 26))}</text>
  <text x="58" y="132" font-size="16" fill="${TEXT_MUTED}">결과 확정 · ${escapeXml(terminal)}</text>

  <rect x="706" y="56" width="206" height="38" rx="8" fill="#131e2c" stroke="${BORDER}"/>
  <text x="809" y="82" text-anchor="middle" font-size="14" font-weight="700" fill="#d7e0eb">${escapeXml(terminal)}</text>
  <rect x="928" y="56" width="82" height="38" rx="8" fill="#131e2c" stroke="${BORDER}"/>
  <text x="969" y="82" text-anchor="middle" font-size="17" font-weight="700" fill="${TEXT}">BO${snapshot.series.bestOf}</text>
  <rect x="1024" y="56" width="114" height="38" rx="8" fill="#131e2c" stroke="${BORDER}"/>
  <text x="1081" y="82" text-anchor="middle" font-size="14" font-weight="700" fill="#d7e0eb">${rule}</text>
  <text x="1138" y="132" text-anchor="end" font-size="14" fill="${TEXT_SUBTLE}">SERIES #${snapshot.series.displayNumber}</text>

  <rect x="48" y="${SCORE_Y}" width="1104" height="${SCORE_HEIGHT}" rx="12" fill="${SURFACE_STRONG}" stroke="${BORDER}"/>
  <rect x="48" y="${SCORE_Y}" width="6" height="${SCORE_HEIGHT}" rx="3" fill="${accent}"/>
  <text x="232" y="235" text-anchor="middle" font-size="17" font-weight="700" fill="${TEAM_1_COLOR}">${escapeXml(truncate(team1.name, 18))}</text>
  <text x="232" y="286" text-anchor="middle" font-size="48" font-weight="700" fill="${TEXT}">${snapshot.score.team1}</text>
  <text x="600" y="266" text-anchor="middle" font-size="28" font-weight="600" fill="${TEXT_SUBTLE}">:</text>
  <text x="968" y="235" text-anchor="middle" font-size="17" font-weight="700" fill="${TEAM_2_COLOR}">${escapeXml(truncate(team2.name, 18))}</text>
  <text x="968" y="286" text-anchor="middle" font-size="48" font-weight="700" fill="${TEXT}">${snapshot.score.team2}</text>
  <text x="600" y="235" text-anchor="middle" font-size="15" font-weight="700" letter-spacing="1.1" fill="${TEXT_SUBTLE}">최종 결과</text>
  <text x="600" y="292" text-anchor="middle" font-size="25" font-weight="700" fill="${accent}">${escapeXml(winner)}</text>

  <text x="48" y="338" font-size="16" font-weight="700" fill="#d7e0eb">참가자</text>
  <line x1="48" y1="${FULL_ROSTER_Y}" x2="1152" y2="${FULL_ROSTER_Y}" stroke="${BORDER_MUTED}"/>
  ${renderCompactRosterColumn(team1, 48, 528, TEAM_1_COLOR, 'TEAM 1')}
  <line x1="600" y1="${FULL_ROSTER_Y}" x2="600" y2="${FULL_ROSTER_Y + FULL_ROSTER_HEIGHT}" stroke="${BORDER_MUTED}"/>
  ${renderCompactRosterColumn(team2, 624, 528, TEAM_2_COLOR, 'TEAM 2')}
  <line x1="48" y1="${FULL_ROSTER_Y + FULL_ROSTER_HEIGHT}" x2="1152" y2="${FULL_ROSTER_Y + FULL_ROSTER_HEIGHT}" stroke="${BORDER_MUTED}"/>

  <text x="48" y="${FULL_SET_START_Y - 20}" font-size="16" font-weight="700" fill="#d7e0eb">세트 기록</text>
  ${setCards}

  <line x1="48" y1="${footerLineY}" x2="1152" y2="${footerLineY}" stroke="${BORDER_MUTED}"/>
  <text x="48" y="${height - 28}" font-size="13" fill="${TEXT_SUBTLE}">확정된 세트 기준 · ${snapshot.games.length}세트</text>
  <text x="1152" y="${height - 28}" text-anchor="end" font-size="13" fill="${TEXT_SUBTLE}">monkeybot.lol</text>
</svg>`;
}

function renderCompactSetRow(setNumber: number, winner: 'TEAM_1' | 'TEAM_2', y: number): string {
	const color = winner === 'TEAM_1' ? TEAM_1_COLOR : TEAM_2_COLOR;
	const label = winner === 'TEAM_1' ? '1팀 승' : '2팀 승';
	return `<rect x="48" y="${y}" width="1104" height="38" rx="8" fill="${SURFACE_STRONG}" stroke="${BORDER_MUTED}"/>
  <text x="72" y="${y + 25}" font-size="15" font-weight="700" fill="#d7e0eb">${setNumber}세트</text>
  <rect x="974" y="${y + 6}" width="154" height="26" rx="6" fill="${color}" fill-opacity="0.14"/>
  <text x="1051" y="${y + 25}" text-anchor="middle" font-size="14" font-weight="700" fill="${color}">${label}</text>`;
}

function renderCompactRosterColumn(
	team: SeriesResultNotificationV2Snapshot['teams'][number],
	x: number,
	width: number,
	color: string,
	label: string
): string {
	const rows = team.players
		.map((player, index) => {
			const y = FULL_ROSTER_Y + 40 + index * 14;
			return `<text x="${x}" y="${y}" font-size="12" font-weight="700" fill="${color}">${LANE_LABELS[player.lane]}</text>
  <text x="${x + 48}" y="${y}" font-size="14" font-weight="600" fill="${TEXT}">${escapeXml(truncate(player.displayName, 32))}</text>`;
		})
		.join('\n');

	return `<text x="${x}" y="${FULL_ROSTER_Y + 21}" font-size="16" font-weight="700" fill="${TEXT}">${escapeXml(truncate(team.name, 24))}</text>
  <text x="${x + width}" y="${FULL_ROSTER_Y + 21}" text-anchor="end" font-size="12" font-weight="700" letter-spacing="0.8" fill="${color}">${label}</text>
  ${rows}`;
}

function renderFullSetCard(
	game: SeriesResultNotificationV2Snapshot['games'][number],
	y: number
): string {
	const winnerColor = game.winner === 'TEAM_1' ? TEAM_1_COLOR : TEAM_2_COLOR;
	const winner = game.winner === 'TEAM_1' ? '1팀 승' : '2팀 승';
	const team1Side = game.sidePreset === 'DEFAULT' ? 'BLUE' : 'RED';
	const team2Side = team1Side === 'BLUE' ? 'RED' : 'BLUE';

	return `<rect x="48" y="${y}" width="1104" height="${FULL_SET_HEIGHT}" rx="10" fill="${SURFACE_STRONG}"/>
  <rect x="48" y="${y}" width="4" height="${FULL_SET_HEIGHT}" rx="2" fill="${winnerColor}"/>
  <text x="76" y="${y + 29}" font-size="17" font-weight="700" fill="${TEXT}">${game.setNumber}세트</text>
  <text x="162" y="${y + 29}" font-size="13" font-weight="700" fill="${winnerColor}">${winner}</text>
  <line x1="76" y1="${y + 43}" x2="1124" y2="${y + 43}" stroke="${BORDER_MUTED}"/>
  ${renderPickStrip(game.team1, 163, y, TEAM_1_COLOR, team1Side, game.patchVersion)}
  ${renderPickStrip(game.team2, 715, y, TEAM_2_COLOR, team2Side, game.patchVersion)}
  <line x1="76" y1="${y + 141}" x2="1124" y2="${y + 141}" stroke="${BORDER_MUTED}"/>
  ${renderBanStrip(game.bans.blue, 207, y + 151, '#60a5fa', game.patchVersion)}
  ${renderBanStrip(game.bans.red, 759, y + 151, '#fb7185', game.patchVersion)}`;
}

function renderPickStrip(
	team: SeriesResultNotificationV2Snapshot['games'][number]['team1'],
	x: number,
	y: number,
	teamColor: string,
	side: 'BLUE' | 'RED',
	patchVersion: string | null
): string {
	const stripWidth = team.picks.length * PICK_TILE_SIZE + (team.picks.length - 1) * PICK_TILE_GAP;
	const sideColor = side === 'BLUE' ? '#60a5fa' : '#fb7185';
	const tiles = team.picks
		.map((pick, index) => {
			const tileX = x + index * (PICK_TILE_SIZE + PICK_TILE_GAP);
			return renderChampionTile(
				pick.championId,
				tileX,
				y + 69,
				PICK_TILE_SIZE,
				teamColor,
				patchVersion
			);
		})
		.join('\n');

	return `<rect x="${x}" y="${y + 58}" width="${stripWidth}" height="3" rx="1.5" fill="${sideColor}"/>
  ${tiles}`;
}

function renderBanStrip(
	bans: Array<string | null>,
	x: number,
	y: number,
	color: string,
	patchVersion: string | null
): string {
	const slots = bans
		.map((champion, index) =>
			renderChampionTile(
				champion,
				x + index * (BAN_TILE_SIZE + BAN_TILE_GAP),
				y,
				BAN_TILE_SIZE,
				color,
				patchVersion
			)
		)
		.join('\n');

	return `<rect x="${x - 16}" y="${y + 19}" width="5" height="5" rx="2.5" fill="${color}"/>
  ${slots}`;
}

function renderChampionTile(
	championId: string | null,
	x: number,
	y: number,
	size: number,
	color: string,
	patchVersion: string | null
): string {
	const imageHref = championImageHref(championId, patchVersion);
	const radius = size >= PICK_TILE_SIZE ? 5 : 4;
	const mutedSlot = `<path d="M${x + size * 0.3} ${y + size * 0.5}h${size * 0.4}" stroke="${TEXT_SUBTLE}" stroke-width="2" stroke-linecap="round"/>`;

	return `<g>
  <rect x="${x}" y="${y}" width="${size}" height="${size}" rx="${radius}" fill="${color}" fill-opacity="${imageHref ? '0.18' : '0.08'}"/>
  ${imageHref ? `<image href="${escapeXml(imageHref)}" x="${x}" y="${y}" width="${size}" height="${size}" preserveAspectRatio="xMidYMid slice"/>` : mutedSlot}
</g>`;
}

function compactCardHeight(snapshot: SeriesResultNotificationV1Snapshot): number {
	return SET_ROWS_Y + snapshot.games.length * SET_ROW_HEIGHT + FOOTER_HEIGHT + 24;
}

function fullCardHeight(snapshot: SeriesResultNotificationV2Snapshot): number {
	return (
		FULL_SET_START_Y +
		snapshot.games.length * FULL_SET_HEIGHT +
		(snapshot.games.length - 1) * FULL_SET_GAP +
		FULL_FOOTER_HEIGHT +
		24
	);
}

function teamFor(
	snapshot: SeriesResultNotificationV2Snapshot,
	teamId: 'TEAM_1' | 'TEAM_2'
): SeriesResultNotificationV2Snapshot['teams'][number] {
	const team = snapshot.teams.find((candidate) => candidate.team === teamId);
	if (!team) throw new Error(`Series result snapshot is missing ${teamId}`);
	return team;
}

function isFullSeriesResultSnapshot(
	snapshot: SeriesResultNotificationSnapshot
): snapshot is SeriesResultNotificationV2Snapshot {
	return 'teams' in snapshot;
}

function terminalLabel(
	terminalReason: SeriesResultNotificationSnapshot['series']['terminalReason']
): string {
	if (terminalReason === 'EARLY_WIN') return '조기 종료 · 승리 처리';
	if (terminalReason === 'EARLY_DRAW') return '조기 종료 · 무승부';
	return '정규 종료';
}

function winnerLabel(
	winningTeam: SeriesResultNotificationSnapshot['series']['winningTeam']
): string {
	if (winningTeam === 'TEAM_1') return '1팀 승리';
	if (winningTeam === 'TEAM_2') return '2팀 승리';
	return '무승부';
}

function outcomeColor(
	winningTeam: SeriesResultNotificationSnapshot['series']['winningTeam']
): string {
	if (winningTeam === 'TEAM_1') return TEAM_1_COLOR;
	if (winningTeam === 'TEAM_2') return TEAM_2_COLOR;
	return DRAW_COLOR;
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
