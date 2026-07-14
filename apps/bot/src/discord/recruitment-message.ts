import {
	ActionRowBuilder,
	ButtonBuilder,
	ButtonStyle,
	EmbedBuilder,
	StringSelectMenuBuilder
} from 'discord.js';
import type { RecruitmentSnapshot } from '../api/contracts.js';
import { recruitmentCustomId } from '../interactions/custom-id.js';

const LANE_LABELS = {
	TOP: '탑',
	JUNGLE: '정글',
	MID: '미드',
	BOTTOM: '원딜',
	SUPPORT: '서폿'
} as const;

const LANES = Object.keys(LANE_LABELS) as Array<keyof typeof LANE_LABELS>;

export function renderRecruitmentMessage(snapshot: RecruitmentSnapshot, webOrigin: URL) {
	const recruitment = snapshot.recruitment;
	const joinedCount = snapshot.participants.length;
	const full = joinedCount >= recruitment.targetCount;
	const roster = snapshot.participants
		.map((participant, index) => {
			const lanes = participant.preferredLanes.length
				? participant.preferredLanes.map((lane) => LANE_LABELS[lane]).join(' / ')
				: '라인 무관';
			return `\`${String(index + 1).padStart(2, ' ')}.\` **${participant.displayName}** · ${lanes}`;
		})
		.join('\n');

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
	const embed = new EmbedBuilder()
		.setColor(
			recruitment.status === 'OPEN'
				? full
					? 0x34d399
					: 0x22d3ee
				: recruitment.status === 'CONVERTED'
					? 0x34d399
					: 0xfbbf24
		)
		.setTitle(`5v5 내전 모집 #${recruitment.displayNumber}`)
		.setDescription(recruitment.title)
		.addFields(
			{ name: '상태', value: statusLabel, inline: true },
			{ name: '정원', value: `${joinedCount} / ${recruitment.targetCount}`, inline: true },
			{
				name: '규칙',
				value: `BO${recruitment.bestOf} · ${recruitment.hardFearless ? '하드 피어리스' : '일반'}`,
				inline: true
			},
			{ name: '참가자 · 선호 라인', value: roster || '아직 참가자가 없습니다.' }
		);

	return {
		embeds: [embed],
		components: recruitmentComponents(snapshot, webOrigin, full)
	};
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
