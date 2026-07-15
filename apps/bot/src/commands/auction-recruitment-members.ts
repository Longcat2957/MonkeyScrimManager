import {
	ApplicationIntegrationType,
	InteractionContextType,
	MessageFlags,
	SlashCommandBuilder,
	type GuildMember
} from 'discord.js';

import { uuidSchema, type Lane } from '../api/contracts.js';
import { actorFromGuildMember, actorFromInteraction } from '../discord/actor.js';
import { guildInteractionError } from '../interactions/guild-context.js';
import type { BotCommand } from './types.js';

const RECRUITMENT_ID_OPTION = '모집';
const MEMBER_OPTION = '멤버';
const LANE_OPTION = '라인';

const LANE_CHOICES = [
	{ name: '탑', value: 'TOP' },
	{ name: '정글', value: 'JUNGLE' },
	{ name: '미드', value: 'MID' },
	{ name: '원딜', value: 'BOTTOM' },
	{ name: '서포터', value: 'SUPPORT' }
] as const;

export const auctionRecruitmentAddMemberCommand: BotCommand = {
	data: new SlashCommandBuilder()
		.setName('경매인원추가')
		.setDescription('[운영자] 경매 모집에 길드 멤버를 강제 참가시킵니다.')
		.setIntegrationTypes(ApplicationIntegrationType.GuildInstall)
		.setContexts(InteractionContextType.Guild)
		.addStringOption((option) =>
			option
				.setName(RECRUITMENT_ID_OPTION)
				.setDescription('경매 모집 UUID')
				.setRequired(true)
				.setMinLength(36)
				.setMaxLength(36)
		)
		.addUserOption((option) =>
			option.setName(MEMBER_OPTION).setDescription('추가할 Discord 멤버').setRequired(true)
		)
		.addStringOption((option) =>
			option
				.setName(LANE_OPTION)
				.setDescription('참가 라인. 생략하면 등록된 주 라인을 사용합니다.')
				.addChoices(...LANE_CHOICES)
		),

	async execute(interaction, { api }) {
		const guildError = guildInteractionError(interaction);
		if (guildError) {
			await interaction.reply({ content: guildError, flags: MessageFlags.Ephemeral });
			return;
		}
		if (!interaction.inCachedGuild()) return;

		const recruitmentId = parseRecruitmentId(
			interaction.options.getString(RECRUITMENT_ID_OPTION, true)
		);
		if (!recruitmentId) {
			await interaction.reply({
				content: '모집 ID는 경매 모집 페이지 URL에 있는 UUID 형식이어야 합니다.',
				flags: MessageFlags.Ephemeral
			});
			return;
		}
		const declaredLane = parseLane(interaction.options.getString(LANE_OPTION));
		if (interaction.options.getString(LANE_OPTION) !== null && !declaredLane) {
			await interaction.reply({
				content: '참가 라인을 확인해 주세요.',
				flags: MessageFlags.Ephemeral
			});
			return;
		}

		const user = interaction.options.getUser(MEMBER_OPTION, true);
		let member: GuildMember;
		try {
			member = await interaction.guild.members.fetch(user.id);
		} catch {
			await interaction.reply({
				content: '현재 Discord 서버에 있는 멤버만 경매 모집에 추가할 수 있습니다.',
				flags: MessageFlags.Ephemeral
			});
			return;
		}

		await interaction.deferReply({ flags: MessageFlags.Ephemeral });
		const snapshot = await api.forceAddAuctionRecruitmentMember(
			interaction.guildId,
			recruitmentId,
			{
				actor: actorFromInteraction(interaction),
				member: actorFromGuildMember(user, member),
				...(declaredLane ? { declaredLane } : {})
			},
			{ idempotencyKey: interaction.id }
		);
		await interaction.editReply(
			`${member.displayName}님을 경매 모집에 추가했습니다. 현재 ${snapshot.participants.length}명 참여 중입니다.`
		);
	}
};

export const auctionRecruitmentRemoveMemberCommand: BotCommand = {
	data: new SlashCommandBuilder()
		.setName('경매인원삭제')
		.setDescription('[운영자] 경매 모집에서 참가자를 제거합니다.')
		.setIntegrationTypes(ApplicationIntegrationType.GuildInstall)
		.setContexts(InteractionContextType.Guild)
		.addStringOption((option) =>
			option
				.setName(RECRUITMENT_ID_OPTION)
				.setDescription('경매 모집 UUID')
				.setRequired(true)
				.setMinLength(36)
				.setMaxLength(36)
		)
		.addUserOption((option) =>
			option.setName(MEMBER_OPTION).setDescription('제거할 Discord 멤버').setRequired(true)
		),

	async execute(interaction, { api }) {
		const guildError = guildInteractionError(interaction);
		if (guildError) {
			await interaction.reply({ content: guildError, flags: MessageFlags.Ephemeral });
			return;
		}
		if (!interaction.inCachedGuild()) return;

		const recruitmentId = parseRecruitmentId(
			interaction.options.getString(RECRUITMENT_ID_OPTION, true)
		);
		if (!recruitmentId) {
			await interaction.reply({
				content: '모집 ID는 경매 모집 페이지 URL에 있는 UUID 형식이어야 합니다.',
				flags: MessageFlags.Ephemeral
			});
			return;
		}
		const user = interaction.options.getUser(MEMBER_OPTION, true);

		await interaction.deferReply({ flags: MessageFlags.Ephemeral });
		const snapshot = await api.leaveAuctionRecruitment(
			interaction.guildId,
			recruitmentId,
			user.id,
			{ actor: actorFromInteraction(interaction) },
			{ idempotencyKey: interaction.id }
		);
		await interaction.editReply(
			`${user.globalName ?? user.username}님을 경매 모집에서 제거했습니다. 현재 ${snapshot.participants.length}명 참여 중입니다.`
		);
	}
};

function parseRecruitmentId(value: string): string | null {
	const recruitmentId = value.trim().toLowerCase();
	return uuidSchema.safeParse(recruitmentId).success ? recruitmentId : null;
}

function parseLane(value: string | null): Lane | null {
	if (value === null) return null;
	return LANE_CHOICES.some((choice) => choice.value === value) ? (value as Lane) : null;
}
