import {
	MessageFlags,
	type ButtonInteraction,
	type ChatInputCommandInteraction,
	type Client,
	type Interaction,
	type StringSelectMenuInteraction
} from 'discord.js';
import type { RecruitmentSnapshot } from '../api/contracts.js';
import { commandByName } from '../commands/index.js';
import { actorFromInteraction } from '../discord/actor.js';
import { renderRecruitmentMessage } from '../discord/recruitment-message.js';
import { parseRecruitmentCustomId } from './custom-id.js';
import type { InteractionContext } from './context.js';
import { guildInteractionError } from './guild-context.js';

type PreferredLane = RecruitmentSnapshot['participants'][number]['preferredLanes'][number];

const PREFERRED_LANES = new Set<PreferredLane>(['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT']);

export function installInteractionDispatcher(client: Client, context: InteractionContext): void {
	client.on('interactionCreate', (interaction) => {
		void dispatchInteraction(interaction, context);
	});
}

export async function dispatchInteraction(
	interaction: Interaction,
	context: InteractionContext
): Promise<void> {
	try {
		if (interaction.isChatInputCommand()) {
			await dispatchCommand(interaction, context);
			return;
		}
		if (interaction.isButton()) {
			await dispatchRecruitmentButton(interaction, context);
			return;
		}
		if (interaction.isStringSelectMenu()) {
			await dispatchRecruitmentLanes(interaction, context);
		}
	} catch (error) {
		context.logger.error(
			{
				error,
				interactionId: interaction.id,
				guildId: interaction.guildId,
				customId: 'customId' in interaction ? interaction.customId : undefined
			},
			'Discord interaction failed'
		);
		await reportInteractionError(interaction);
	}
}

async function dispatchCommand(
	interaction: ChatInputCommandInteraction,
	context: InteractionContext
): Promise<void> {
	const command = commandByName(interaction.commandName);
	if (!command) return;
	context.logger.info(
		{
			interactionId: interaction.id,
			commandName: interaction.commandName,
			guildId: interaction.guildId,
			interactionContext: interaction.context,
			integrationTypes: Object.keys(interaction.authorizingIntegrationOwners ?? {}),
			guildCached: Boolean(interaction.guild),
			memberPresent: Boolean(interaction.member)
		},
		'Discord command interaction received'
	);
	await command.execute(interaction, context);
}

async function dispatchRecruitmentButton(
	interaction: ButtonInteraction,
	context: InteractionContext
): Promise<void> {
	const component = parseRecruitmentCustomId(interaction.customId);
	if (!component || component.action === 'roles') return;
	const guildError = guildInteractionError(interaction);
	if (guildError) {
		await interaction.reply({
			content: guildError,
			flags: MessageFlags.Ephemeral
		});
		return;
	}
	if (!interaction.inCachedGuild()) return;

	await interaction.deferUpdate();
	const actor = actorFromInteraction(interaction);
	const requestContext = { idempotencyKey: interaction.id };
	let snapshot: RecruitmentSnapshot;

	if (component.action === 'join') {
		snapshot = await context.api.joinRecruitment(
			interaction.guildId,
			component.recruitmentId,
			{ actor, member: actor },
			requestContext
		);
	} else if (component.action === 'leave') {
		snapshot = await context.api.leaveRecruitment(
			interaction.guildId,
			component.recruitmentId,
			actor.id,
			{ actor },
			requestContext
		);
	} else {
		snapshot = await context.api.startEntry(
			interaction.guildId,
			component.recruitmentId,
			{ actor },
			requestContext
		);
	}

	await interaction.editReply(renderRecruitmentMessage(snapshot, context.config.webOrigin));
	if (component.action === 'start-entry') {
		const entryUrl = new URL(snapshot.links.entry, context.config.webOrigin);
		await interaction.followUp({
			content: `엔트리 편성을 시작했습니다: ${entryUrl.toString()}`,
			flags: MessageFlags.Ephemeral
		});
	}
}

async function dispatchRecruitmentLanes(
	interaction: StringSelectMenuInteraction,
	context: InteractionContext
): Promise<void> {
	const component = parseRecruitmentCustomId(interaction.customId);
	if (!component || component.action !== 'roles') return;
	const guildError = guildInteractionError(interaction);
	if (guildError) {
		await interaction.reply({
			content: guildError,
			flags: MessageFlags.Ephemeral
		});
		return;
	}
	if (!interaction.inCachedGuild()) return;

	await interaction.deferUpdate();
	const preferredLanes = parsePreferredLanes(interaction.values);
	if (!preferredLanes) {
		await interaction.followUp({
			content: '선호 라인 값이 올바르지 않습니다.',
			flags: MessageFlags.Ephemeral
		});
		return;
	}
	const actor = actorFromInteraction(interaction);
	const snapshot = await context.api.setPreferredLanes(
		interaction.guildId,
		component.recruitmentId,
		actor.id,
		{ actor, preferredLanes },
		{ idempotencyKey: interaction.id }
	);
	await interaction.editReply(renderRecruitmentMessage(snapshot, context.config.webOrigin));
}

function parsePreferredLanes(values: readonly string[]): PreferredLane[] | null {
	const lanes: PreferredLane[] = [];
	for (const value of values) {
		if (!PREFERRED_LANES.has(value as PreferredLane)) return null;
		const lane = value as PreferredLane;
		if (!lanes.includes(lane)) lanes.push(lane);
	}
	return lanes;
}

async function reportInteractionError(interaction: Interaction): Promise<void> {
	if (!interaction.isRepliable()) return;
	const reply = {
		content: '요청을 처리하지 못했습니다. 잠시 후 다시 시도해 주세요.',
		flags: MessageFlags.Ephemeral
	} as const;

	try {
		if (interaction.deferred || interaction.replied) {
			await interaction.followUp(reply);
		} else {
			await interaction.reply(reply);
		}
	} catch {
		// The interaction token may already be expired; the original error is logged above.
	}
}
