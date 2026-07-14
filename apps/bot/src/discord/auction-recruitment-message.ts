import {
	ActionRowBuilder,
	ButtonBuilder,
	ButtonStyle,
	EmbedBuilder,
	escapeMarkdown,
	StringSelectMenuBuilder
} from 'discord.js';

import type { AuctionRecruitmentSnapshot, Lane } from '../api/contracts.js';
import { auctionRecruitmentCustomId } from '../interactions/custom-id.js';

const MAX_EMBED_FIELD_VALUE_LENGTH = 1_024;
const LANE_LABELS = {
	TOP: '탑',
	JUNGLE: '정글',
	MID: '미드',
	BOTTOM: '원딜',
	SUPPORT: '서폿'
} as const satisfies Record<Lane, string>;
const LANES = Object.keys(LANE_LABELS) as Lane[];

export function renderAuctionRecruitmentMessage(
	snapshot: AuctionRecruitmentSnapshot,
	webOrigin: URL
) {
	const recruitment = snapshot.recruitment;
	const joinedCount = snapshot.participants.length;
	const full = joinedCount >= recruitment.participantCount;
	const formatLabel = `${
		recruitment.format === 'TEN_PERSON' ? '10인 · 2팀' : '20인 · 4팀'
	}\n${auctionRulesLabel(recruitment.format, recruitment.rules)}`;
	const statusLabel =
		recruitment.status === 'OPEN'
			? full
				? '정원 도달'
				: '모집 중'
			: recruitment.status === 'CLOSED'
				? '모집 마감'
				: recruitment.status === 'CANCELLED'
					? '모집 취소'
					: '경매 진행 중';
	const embed = new EmbedBuilder()
		.setColor(
			recruitment.status === 'OPEN'
				? full
					? 0x34d399
					: 0x22d3ee
				: recruitment.status === 'CANCELLED'
					? 0xf87171
					: 0xfbbf24
		)
		.setTitle(`경매 내전 모집 #${recruitment.displayNumber}`)
		.setDescription(recruitment.title)
		.addFields(
			{ name: '상태', value: statusLabel, inline: true },
			{ name: '형식', value: formatLabel, inline: true },
			{ name: '정원', value: `${joinedCount} / ${recruitment.participantCount}`, inline: true },
			...rosterFields(snapshot)
		);

	return {
		embeds: [embed],
		components: auctionRecruitmentComponents(snapshot, webOrigin, full)
	};
}

function auctionRulesLabel(
	format: AuctionRecruitmentSnapshot['recruitment']['format'],
	rules: AuctionRecruitmentSnapshot['recruitment']['rules']
): string {
	const fearless = rules.hardFearless ? '하드 피어리스' : '일반';
	if (format === 'TEN_PERSON') return `BO${rules.finalBestOf} · ${fearless}`;
	return `4강 BO${rules.semifinalBestOf ?? 3} · 결승 BO${rules.finalBestOf} · ${fearless}`;
}

function rosterFields(
	snapshot: AuctionRecruitmentSnapshot
): Array<{ name: string; value: string }> {
	if (snapshot.participants.length === 0) {
		return [{ name: '참가자 · 참가 라인', value: '아직 참가자가 없습니다.' }];
	}

	const chunks: string[] = [];
	let current = '';
	for (const [index, participant] of snapshot.participants.entries()) {
		const lane = participant.declaredLane ? LANE_LABELS[participant.declaredLane] : '라인 미설정';
		const line = truncateRosterLine(
			`\`${String(index + 1).padStart(2, ' ')}.\` **${escapeMarkdown(compactText(participant.displayName))}** · ${lane}`
		);
		const next = current ? `${current}\n${line}` : line;
		if (next.length > MAX_EMBED_FIELD_VALUE_LENGTH && current) {
			chunks.push(current);
			current = line;
		} else {
			current = next;
		}
	}
	if (current) chunks.push(current);

	return chunks.map((value, index) => ({
		name: index === 0 ? '참가자 · 참가 라인' : '참가자 · 참가 라인 (계속)',
		value
	}));
}

function compactText(value: string): string {
	return value.replace(/\s+/g, ' ').trim() || '이름 미설정';
}

function truncateRosterLine(value: string): string {
	if (value.length <= MAX_EMBED_FIELD_VALUE_LENGTH) return value;
	return `${value.slice(0, MAX_EMBED_FIELD_VALUE_LENGTH - 1)}…`;
}

function auctionRecruitmentComponents(
	snapshot: AuctionRecruitmentSnapshot,
	webOrigin: URL,
	full: boolean
): Array<ActionRowBuilder<ButtonBuilder | StringSelectMenuBuilder>> {
	const recruitment = snapshot.recruitment;
	if (recruitment.status === 'CANCELLED') return [];

	const target = recruitment.status === 'CLOSED' ? snapshot.links.draft : recruitment.href;
	if (!target) return [];
	const link = new ButtonBuilder()
		.setLabel(recruitment.status === 'CLOSED' ? '마감 명단 열기' : '경매 모집 열기')
		.setStyle(ButtonStyle.Link)
		.setURL(new URL(target, webOrigin).toString());

	if (recruitment.status !== 'OPEN') {
		return [new ActionRowBuilder<ButtonBuilder>().addComponents(link)];
	}

	const participantActions = new ActionRowBuilder<ButtonBuilder>();
	if (!full) {
		participantActions.addComponents(
			new ButtonBuilder()
				.setCustomId(auctionRecruitmentCustomId('join', recruitment.id))
				.setLabel('참가')
				.setStyle(ButtonStyle.Primary)
		);
	}
	participantActions.addComponents(
		new ButtonBuilder()
			.setCustomId(auctionRecruitmentCustomId('leave', recruitment.id))
			.setLabel('참가 취소')
			.setStyle(ButtonStyle.Secondary),
		link
	);

	return [participantActions, auctionLaneRow(recruitment.id)];
}

function auctionLaneRow(recruitmentId: string): ActionRowBuilder<StringSelectMenuBuilder> {
	const laneSelect = new StringSelectMenuBuilder()
		.setCustomId(auctionRecruitmentCustomId('lane', recruitmentId))
		.setPlaceholder('참가 라인 선택 후 참가')
		.setMinValues(1)
		.setMaxValues(1)
		.addOptions(LANES.map((lane) => ({ label: LANE_LABELS[lane], value: lane })));

	return new ActionRowBuilder<StringSelectMenuBuilder>().addComponents(laneSelect);
}
