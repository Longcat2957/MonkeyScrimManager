import {
	ApplicationIntegrationType,
	ChannelType,
	InteractionContextType,
	MessageFlags,
	PermissionFlagsBits,
	SlashCommandBuilder
} from 'discord.js';
import { actorFromInteraction } from '../discord/actor.js';
import { guildInteractionError } from '../interactions/guild-context.js';
import type { BotCommand } from './types.js';

const RECRUITMENT_CHANNEL_OPTION = '모집채널';
const ENTRY_CHANNEL_OPTION = '엔트리채널';
const RESULT_CHANNEL_OPTION = '결과알림채널';
const LOG_CHANNEL_OPTION = '운영로그채널';
const SETUP_CHANNEL_TYPES = [ChannelType.GuildText, ChannelType.GuildAnnouncement] as const;

type SetupChannelResolution =
	{ ok: true; channelId: string | null } | { ok: false; message: string };

export const setupCommand: BotCommand = {
	data: new SlashCommandBuilder()
		.setName('초기설정')
		.setDescription('현재 Discord 서버를 MonkeyBot 길드로 설정합니다.')
		.setIntegrationTypes(ApplicationIntegrationType.GuildInstall)
		.setContexts(InteractionContextType.Guild)
		.setDefaultMemberPermissions(PermissionFlagsBits.ManageGuild)
		.addRoleOption((option) =>
			option
				.setName('운영자역할')
				.setDescription('길드 운영 권한으로 사용할 Discord 역할')
				.setRequired(true)
		)
		.addChannelOption((option) =>
			option
				.setName(RECRUITMENT_CHANNEL_OPTION)
				.setDescription('내전과 경매 모집 메시지를 게시할 채널')
				.addChannelTypes(...SETUP_CHANNEL_TYPES)
				.setRequired(true)
		)
		.addStringOption((option) =>
			option.setName('길드이름').setDescription('MonkeyBot에 표시할 길드 이름')
		)
		.addChannelOption((option) =>
			option
				.setName(ENTRY_CHANNEL_OPTION)
				.setDescription('확정 엔트리와 픽밴 링크를 게시할 채널')
				.addChannelTypes(...SETUP_CHANNEL_TYPES)
		)
		.addChannelOption((option) =>
			option
				.setName(RESULT_CHANNEL_OPTION)
				.setDescription('시리즈 결과 알림을 게시할 채널')
				.addChannelTypes(...SETUP_CHANNEL_TYPES)
		)
		.addChannelOption((option) =>
			option
				.setName(LOG_CHANNEL_OPTION)
				.setDescription('운영 작업과 오류 알림을 게시할 채널')
				.addChannelTypes(...SETUP_CHANNEL_TYPES)
		),

	async execute(interaction, { api, catalogSync, config, logger }) {
		const guildError = guildInteractionError(interaction);
		if (guildError) {
			await interaction.reply({
				content: guildError,
				flags: MessageFlags.Ephemeral
			});
			return;
		}
		if (!interaction.inCachedGuild()) return;

		await interaction.deferReply({ flags: MessageFlags.Ephemeral });
		const operatorRole = interaction.options.getRole('운영자역할', true);
		if (operatorRole.id === interaction.guildId) {
			await interaction.editReply('@everyone 역할은 운영자 역할로 사용할 수 없습니다.');
			return;
		}
		if (operatorRole.managed) {
			await interaction.editReply(
				'Discord 또는 연동 서비스가 관리하는 역할은 운영자 역할로 사용할 수 없습니다.'
			);
			return;
		}
		const recruitmentChannel = resolveSetupChannel(
			interaction,
			RECRUITMENT_CHANNEL_OPTION,
			'모집 채널',
			true
		);
		if (!recruitmentChannel.ok) {
			await interaction.editReply(recruitmentChannel.message);
			return;
		}
		if (!recruitmentChannel.channelId) {
			await interaction.editReply('모집 채널을 선택해야 합니다.');
			return;
		}
		const entryChannel = resolveSetupChannel(
			interaction,
			ENTRY_CHANNEL_OPTION,
			'엔트리 채널',
			false
		);
		if (!entryChannel.ok) {
			await interaction.editReply(entryChannel.message);
			return;
		}
		const resultChannel = resolveSetupChannel(
			interaction,
			RESULT_CHANNEL_OPTION,
			'결과 알림 채널',
			false
		);
		if (!resultChannel.ok) {
			await interaction.editReply(resultChannel.message);
			return;
		}
		const logChannel = resolveSetupChannel(
			interaction,
			LOG_CHANNEL_OPTION,
			'운영 로그 채널',
			false
		);
		if (!logChannel.ok) {
			await interaction.editReply(logChannel.message);
			return;
		}
		const requestedName = interaction.options.getString('길드이름')?.trim();
		const actor = actorFromInteraction(interaction);

		await api.setupGuild(
			interaction.guildId,
			{
				actor,
				guildName: requestedName || interaction.guild.name,
				guildIconHash: interaction.guild.icon,
				operatorRoleId: operatorRole.id,
				recruitmentChannelId: recruitmentChannel.channelId,
				entryChannelId: entryChannel.channelId,
				resultChannelId: resultChannel.channelId,
				logChannelId: logChannel.channelId,
				seasonName: '시즌 1'
			},
			{ idempotencyKey: interaction.id }
		);
		try {
			await catalogSync.syncGuild(interaction.guild);
		} catch (error) {
			logger.warn(
				{ err: error, guildId: interaction.guildId },
				'Discord catalog sync after guild setup failed'
			);
		}

		const dashboardUrl = new URL(`/g/${interaction.guildId}`, config.webOrigin);
		await interaction.editReply(
			`초기 설정을 완료했습니다. 길드 운영 화면: ${dashboardUrl.toString()}`
		);
	}
};

function resolveSetupChannel(
	interaction: Parameters<BotCommand['execute']>[0],
	optionName: string,
	label: string,
	required: boolean
): SetupChannelResolution {
	const channel = interaction.options.getChannel(optionName, required, SETUP_CHANNEL_TYPES);
	if (!channel) {
		return required
			? { ok: false, message: `${label}을 선택해야 합니다.` }
			: { ok: true, channelId: null };
	}
	if (
		channel.isDMBased() ||
		channel.guildId !== interaction.guildId ||
		!channel.isTextBased() ||
		!channel.isSendable() ||
		(channel.type !== ChannelType.GuildText && channel.type !== ChannelType.GuildAnnouncement)
	) {
		return {
			ok: false,
			message: `선택한 ${label}은 현재 서버에서 Bot이 메시지를 보낼 수 있는 텍스트 또는 공지 채널이어야 합니다.`
		};
	}
	return { ok: true, channelId: channel.id };
}
