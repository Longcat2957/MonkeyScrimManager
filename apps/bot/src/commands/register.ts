import {
	ActionRowBuilder,
	ApplicationIntegrationType,
	ButtonBuilder,
	ButtonStyle,
	Colors,
	EmbedBuilder,
	InteractionContextType,
	MessageFlags,
	SlashCommandBuilder
} from 'discord.js';

import { MonkeyBotApiError, MonkeyBotApiTransportError } from '../api/client.js';
import type { RegisterGuildMemberData } from '../api/contracts.js';
import { actorFromInteraction } from '../discord/actor.js';
import { botErrorReference, reasonWithErrorReference } from '../interactions/error-format.js';
import { guildInteractionError } from '../interactions/guild-context.js';
import type { BotCommand } from './types.js';

const RIOT_ID_OPTION = 'riot_id';
const INVALID_RIOT_ID_MESSAGE =
	'Riot ID 형식이 올바르지 않습니다. `GameName#TagLine` 형식으로 입력해 주세요.';

export const registerCommand: BotCommand = {
	data: new SlashCommandBuilder()
		.setName('등록')
		.setDescription('Riot 계정을 연결하고 현재 길드에 등록합니다.')
		.setIntegrationTypes(ApplicationIntegrationType.GuildInstall)
		.setContexts(InteractionContextType.Guild)
		.addStringOption((option) =>
			option
				.setName(RIOT_ID_OPTION)
				.setDescription('연결할 Riot ID (예: Longcat#KR1)')
				.setRequired(true)
				.setMaxLength(81)
		),

	async execute(interaction, { api, config, logger }) {
		const guildError = guildInteractionError(interaction);
		if (guildError) {
			await interaction.reply({ content: guildError, flags: MessageFlags.Ephemeral });
			return;
		}
		if (!interaction.inCachedGuild()) return;

		const riotId = parseRiotId(interaction.options.getString(RIOT_ID_OPTION, true));
		if (!riotId) {
			await interaction.reply({
				content: INVALID_RIOT_ID_MESSAGE,
				flags: MessageFlags.Ephemeral
			});
			return;
		}

		await interaction.deferReply({ flags: MessageFlags.Ephemeral });
		try {
			const result = await api.registerGuildMember(
				interaction.guildId,
				{
					actor: actorFromInteraction(interaction),
					gameName: riotId.gameName,
					tagLine: riotId.tagLine,
					platform: 'KR'
				},
				{ idempotencyKey: interaction.id }
			);
			await interaction.editReply(registrationReply(result, config.webOrigin));
		} catch (error) {
			logger.warn(
				{ err: error, guildId: interaction.guildId, userId: interaction.user.id },
				'Riot account registration failed'
			);
			await interaction.editReply(registrationErrorMessage(error, interaction.id));
		}
	}
};

export function parseRiotId(value: string): { gameName: string; tagLine: string } | null {
	const trimmed = value.trim();
	const separator = trimmed.indexOf('#');
	if (separator <= 0 || separator !== trimmed.lastIndexOf('#')) return null;

	const gameName = trimmed.slice(0, separator).trim();
	const tagLine = trimmed.slice(separator + 1).trim();
	if (
		!gameName ||
		!tagLine ||
		[...gameName].length > 64 ||
		[...tagLine].length > 16 ||
		gameName.includes('#') ||
		tagLine.includes('#') ||
		[...gameName, ...tagLine].some((character) => {
			const codePoint = character.codePointAt(0);
			return codePoint !== undefined && (codePoint <= 31 || codePoint === 127);
		})
	) {
		return null;
	}
	return { gameName, tagLine };
}

function registrationReply(result: RegisterGuildMemberData, webOrigin: URL) {
	const riotId = `${result.riotAccount.gameName}#${result.riotAccount.tagLine}`;
	const alreadyRegistered = result.accountLink === 'EXISTING' && result.registration === 'EXISTING';
	const embed = new EmbedBuilder()
		.setColor(alreadyRegistered ? Colors.Greyple : Colors.Green)
		.setTitle(alreadyRegistered ? '이미 등록된 계정입니다' : '계정 등록 완료')
		.addFields(
			{ name: 'Riot ID', value: riotId, inline: true },
			{ name: '길드 소속', value: '등록 완료', inline: true }
		);
	const actions = new ActionRowBuilder<ButtonBuilder>().addComponents(
		new ButtonBuilder()
			.setStyle(ButtonStyle.Link)
			.setLabel('내 정보')
			.setURL(new URL('/me', webOrigin).toString())
	);
	return { embeds: [embed], components: [actions] };
}

function registrationErrorMessage(error: unknown, interactionId: string): string {
	const reference = botErrorReference(error, interactionId);
	return reasonWithErrorReference(registrationErrorReason(error, reference.reason), reference);
}

function registrationErrorReason(error: unknown, fallback: string): string {
	if (error instanceof MonkeyBotApiError) {
		switch (error.code) {
			case 'invalid_riot_id':
				return INVALID_RIOT_ID_MESSAGE;
			case 'riot_account_not_found':
				return '입력한 Riot 계정을 찾을 수 없습니다. Riot ID와 태그를 확인해 주세요.';
			case 'riot_account_already_linked':
				return '이 Riot 계정은 이미 다른 Discord 사용자에게 연결되어 있습니다.';
			case 'member_inactive':
				return '현재 길드에서 비활성화된 사용자입니다. 길드 운영자에게 문의해 주세요.';
			case 'guild_inactive':
				return '현재 길드에서는 계정을 등록할 수 없습니다. 길드 운영자에게 문의해 주세요.';
			case 'guild_setup_required':
				return '길드 초기 설정을 먼저 완료해야 합니다.';
			case 'riot_rate_limited':
				return 'Riot 조회 요청이 많습니다. 잠시 후 다시 시도해 주세요.';
			case 'riot_unavailable':
			case 'riot_upstream_timeout':
			case 'riot_upstream_failure':
			case 'riot_upstream_invalid_response':
				return 'Riot 계정 조회 서비스를 일시적으로 사용할 수 없습니다. 잠시 후 다시 시도해 주세요.';
		}
	}
	if (error instanceof MonkeyBotApiTransportError) {
		return 'MonkeyBot API에 연결할 수 없습니다. 잠시 후 다시 시도해 주세요.';
	}
	return fallback;
}
