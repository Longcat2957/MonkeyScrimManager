import {
	MessageFlags,
	type ButtonInteraction,
	type ChatInputCommandInteraction,
	type Client,
	type Interaction,
	type StringSelectMenuInteraction
} from 'discord.js';
import type { AuctionRecruitmentSnapshot, RecruitmentSnapshot } from '../api/contracts.js';
import { commandByName } from '../commands/index.js';
import { actorFromInteraction } from '../discord/actor.js';
import { renderAuctionRecruitmentMessage } from '../discord/auction-recruitment-message.js';
import { renderRecruitmentMessage } from '../discord/recruitment-message.js';
import {
	parseAuctionRecruitmentCustomId,
	parseRecruitmentCustomId,
	type AuctionRecruitmentComponentId
} from './custom-id.js';
import type { InteractionContext } from './context.js';
import { botErrorReference, reasonWithErrorReference } from './error-format.js';
import { guildInteractionError } from './guild-context.js';

type PreferredLane = RecruitmentSnapshot['participants'][number]['preferredLanes'][number];
type AuctionLane = Exclude<
	AuctionRecruitmentSnapshot['participants'][number]['declaredLane'],
	null
>;

const PREFERRED_LANES = new Set<PreferredLane>(['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT']);
const AUCTION_LANES = new Set<AuctionLane>(['TOP', 'JUNGLE', 'MID', 'BOTTOM', 'SUPPORT']);

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
			const auctionComponent = parseAuctionRecruitmentCustomId(interaction.customId);
			if (auctionComponent) {
				await dispatchAuctionRecruitmentButton(interaction, context, auctionComponent);
				return;
			}
			await dispatchRecruitmentButton(interaction, context);
			return;
		}
		if (interaction.isStringSelectMenu()) {
			const auctionComponent = parseAuctionRecruitmentCustomId(interaction.customId);
			if (auctionComponent) {
				await dispatchAuctionRecruitmentLane(interaction, context, auctionComponent);
				return;
			}
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
		await reportInteractionError(interaction, error);
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

async function dispatchAuctionRecruitmentButton(
	interaction: ButtonInteraction,
	context: InteractionContext,
	component: AuctionRecruitmentComponentId
): Promise<void> {
	if (component.action === 'lane') return;
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
	const snapshot =
		component.action === 'join'
			? await context.api.joinAuctionRecruitment(
					interaction.guildId,
					component.recruitmentId,
					{ actor },
					requestContext
				)
			: await context.api.leaveAuctionRecruitment(
					interaction.guildId,
					component.recruitmentId,
					actor.id,
					{ actor },
					requestContext
				);

	await interaction.editReply(renderAuctionRecruitmentMessage(snapshot, context.config.webOrigin));
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

async function dispatchAuctionRecruitmentLane(
	interaction: StringSelectMenuInteraction,
	context: InteractionContext,
	component: AuctionRecruitmentComponentId
): Promise<void> {
	if (component.action !== 'lane') return;
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
	const declaredLane = parseAuctionLane(interaction.values);
	if (!declaredLane) {
		await interaction.followUp({
			content: '참가 라인은 하나만 선택해 주세요.',
			flags: MessageFlags.Ephemeral
		});
		return;
	}
	const actor = actorFromInteraction(interaction);
	const snapshot = await context.api.joinAuctionRecruitment(
		interaction.guildId,
		component.recruitmentId,
		{ actor, declaredLane },
		{ idempotencyKey: interaction.id }
	);
	await interaction.editReply(renderAuctionRecruitmentMessage(snapshot, context.config.webOrigin));
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

function parseAuctionLane(values: readonly string[]): AuctionLane | null {
	if (values.length !== 1) return null;
	const lane = values[0];
	return lane && AUCTION_LANES.has(lane as AuctionLane) ? (lane as AuctionLane) : null;
}

export function interactionErrorContent(error: unknown, interactionId: string): string {
	const reference = botErrorReference(error, interactionId);
	return [
		'요청을 처리하지 못했습니다.',
		reasonWithErrorReference(`사유: ${reference.reason}`, reference)
	].join('\n');
}

async function reportInteractionError(interaction: Interaction, error: unknown): Promise<void> {
	if (!interaction.isRepliable()) return;
	const reply = {
		content: interactionErrorContent(error, interaction.id),
		allowedMentions: { parse: [] },
		flags: MessageFlags.Ephemeral
	} as const;

	try {
		if (interaction.isChatInputCommand() && interaction.deferred && !interaction.replied) {
			if (interaction.ephemeral === true) {
				await interaction.editReply({
					content: reply.content,
					allowedMentions: reply.allowedMentions
				});
			} else {
				await interaction.followUp(reply);
				await interaction.deleteReply();
			}
		} else if (interaction.deferred || interaction.replied) {
			await interaction.followUp(reply);
		} else {
			await interaction.reply(reply);
		}
	} catch {
		// The interaction token may already be expired; the original error is logged above.
	}
}
