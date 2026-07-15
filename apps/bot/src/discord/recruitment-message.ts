import {
	ActionRowBuilder,
	ButtonBuilder,
	ButtonStyle,
	escapeMarkdown,
	StringSelectMenuBuilder
} from 'discord.js';
import type { RecruitmentSnapshot } from '../api/contracts.js';
import { recruitmentCustomId } from '../interactions/custom-id.js';
import { BOT_COLORS, botContainer, botReply, botSeparator, botText } from './message-v2.js';

const LANE_LABELS = {
	TOP: '탑',
	JUNGLE: '정글',
	MID: '미드',
	BOTTOM: '원딜',
	SUPPORT: '서폿'
} as const;

const LANES = Object.keys(LANE_LABELS) as Array<keyof typeof LANE_LABELS>;
const MAX_ROSTER_TEXT_LENGTH = 1_024;

export function renderRecruitmentMessage(snapshot: RecruitmentSnapshot, webOrigin: URL) {
	const recruitment = snapshot.recruitment;
	const joinedCount = snapshot.participants.length;
	const full = joinedCount >= recruitment.targetCount;
	const statusLabel =
		recruitment.status === 'OPEN'
			? full
				? '정원 도달'
				: '모집 중'
			: recruitment.status === 'CLOSED'
				? '엔트리 편성 중'
				: recruitment.status === 'CONVERTED'
					? '시리즈 진행 중'
					: '모집 취소';
	const rosterBlocks = recruitmentRosterBlocks(snapshot);
	const color =
		recruitment.status === 'OPEN'
			? full
				? BOT_COLORS.success
				: BOT_COLORS.info
			: recruitment.status === 'CONVERTED'
				? BOT_COLORS.success
				: recruitment.status === 'CANCELLED'
					? BOT_COLORS.error
					: BOT_COLORS.warning;

	return botReply(
		botContainer({
			color,
			children: [
				botText(
					`## 5v5 내전 모집 #${recruitment.displayNumber}\n${escapeMarkdown(
						compactText(recruitment.title)
					)}`
				),
				botText(
					`**${statusLabel}** · 정원 **${joinedCount} / ${recruitment.targetCount}** · BO${recruitment.bestOf} · ${
						recruitment.hardFearless ? '하드 피어리스' : '일반'
					}`
				),
				botSeparator(),
				botText('### 참가자 · 선호 라인'),
				...rosterBlocks.map((block) => botText(block))
			]
		}),
		...recruitmentComponents(snapshot, webOrigin, full)
	);
}

function recruitmentRosterBlocks(snapshot: RecruitmentSnapshot): string[] {
	if (snapshot.participants.length === 0) return ['_(아직 참가자가 없습니다.)_'];

	const blocks: string[] = [];
	let current = '';
	for (const [index, participant] of snapshot.participants.entries()) {
		const lanes = participant.preferredLanes.length
			? participant.preferredLanes.map((lane) => LANE_LABELS[lane]).join(' / ')
			: '라인 무관';
		const line = `\`${String(index + 1).padStart(2, ' ')}.\` **${escapeMarkdown(
			compactText(participant.displayName)
		)}** · ${lanes}`;
		const next = current ? `${current}\n${line}` : line;

		if (next.length > MAX_ROSTER_TEXT_LENGTH && current) {
			blocks.push(current);
			current = line;
		} else {
			current = next;
		}
	}
	if (current) blocks.push(current);
	return blocks;
}

function compactText(value: string): string {
	const normalized = value.replace(/\s+/g, ' ').trim() || '이름 미설정';
	return normalized.length <= 120 ? normalized : `${normalized.slice(0, 119)}…`;
}

function recruitmentComponents(
	snapshot: RecruitmentSnapshot,
	webOrigin: URL,
	full: boolean
): Array<ActionRowBuilder<ButtonBuilder | StringSelectMenuBuilder>> {
	const recruitment = snapshot.recruitment;
	if (recruitment.status === 'CLOSED') {
		const entryUrl = new URL(snapshot.links.entry, webOrigin);
		return [
			new ActionRowBuilder<ButtonBuilder>().addComponents(
				new ButtonBuilder()
					.setLabel('엔트리 편성 열기')
					.setStyle(ButtonStyle.Link)
					.setURL(entryUrl.toString())
			)
		];
	}

	if (recruitment.status !== 'OPEN') return [];
	if (full) {
		return [
			new ActionRowBuilder<ButtonBuilder>().addComponents(
				new ButtonBuilder()
					.setCustomId(recruitmentCustomId('start-entry', recruitment.id))
					.setLabel('엔트리 편성 시작')
					.setStyle(ButtonStyle.Success),
				new ButtonBuilder()
					.setCustomId(recruitmentCustomId('leave', recruitment.id))
					.setLabel('참가 취소')
					.setStyle(ButtonStyle.Secondary)
			),
			preferredLaneRow(recruitment.id)
		];
	}

	const participantActions = new ActionRowBuilder<ButtonBuilder>().addComponents(
		new ButtonBuilder()
			.setCustomId(recruitmentCustomId('join', recruitment.id))
			.setLabel('참가')
			.setStyle(ButtonStyle.Primary),
		new ButtonBuilder()
			.setCustomId(recruitmentCustomId('leave', recruitment.id))
			.setLabel('참가 취소')
			.setStyle(ButtonStyle.Secondary)
	);
	return [participantActions, preferredLaneRow(recruitment.id)];
}

function preferredLaneRow(recruitmentId: string): ActionRowBuilder<StringSelectMenuBuilder> {
	const laneSelect = new StringSelectMenuBuilder()
		.setCustomId(recruitmentCustomId('roles', recruitmentId))
		.setPlaceholder('선호 라인 선택')
		.setMinValues(1)
		.setMaxValues(LANES.length)
		.addOptions(LANES.map((lane) => ({ label: LANE_LABELS[lane], value: lane })));

	return new ActionRowBuilder<StringSelectMenuBuilder>().addComponents(laneSelect);
}
