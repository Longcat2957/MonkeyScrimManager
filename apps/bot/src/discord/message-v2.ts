import {
	ActionRowBuilder,
	ButtonBuilder,
	ContainerBuilder,
	MessageFlags,
	SeparatorBuilder,
	StringSelectMenuBuilder,
	TextDisplayBuilder
} from 'discord.js';

export const BOT_COLORS = {
	info: 0x22d3ee,
	success: 0x34d399,
	warning: 0xfbbf24,
	error: 0xf87171,
	neutral: 0x64748b
} as const;

export type BotActionRow = ActionRowBuilder<ButtonBuilder | StringSelectMenuBuilder>;
export type BotTopLevelComponent = ContainerBuilder | BotActionRow;
export type BotContainerChild = TextDisplayBuilder | SeparatorBuilder | BotActionRow;
export type BotNoticeTone = 'info' | 'success' | 'warning' | 'error' | 'neutral';

export type BotReplyPayload = {
	flags: number;
	components: BotTopLevelComponent[];
	allowedMentions: { parse: [] };
};

const NO_ALLOWED_MENTIONS: { parse: [] } = { parse: [] };

export function botText(content: string): TextDisplayBuilder {
	return new TextDisplayBuilder().setContent(content);
}

export function botSeparator(): SeparatorBuilder {
	return new SeparatorBuilder();
}

export function botContainer({
	color,
	children
}: {
	color?: number;
	children: BotContainerChild[];
}): ContainerBuilder {
	const container = new ContainerBuilder();
	if (color !== undefined) container.setAccentColor(color);

	for (const child of children) {
		if (child instanceof TextDisplayBuilder) {
			container.addTextDisplayComponents(child);
		} else if (child instanceof SeparatorBuilder) {
			container.addSeparatorComponents(child);
		} else {
			container.addActionRowComponents(child);
		}
	}

	return container;
}

export function botNotice({
	tone,
	title,
	body,
	details
}: {
	tone: BotNoticeTone;
	title: string;
	body?: string | undefined;
	details?: string | readonly string[] | undefined;
}): ContainerBuilder {
	const detailText = typeof details === 'string' ? details : details?.filter(Boolean).join('\n');
	const heading = body ? `## ${title}\n${body}` : `## ${title}`;
	const children: BotContainerChild[] = [botText(heading)];

	if (detailText) {
		children.push(botSeparator(), botText(detailText));
	}

	return botContainer({ color: BOT_COLORS[tone], children });
}

export function botReply(...components: BotTopLevelComponent[]): BotReplyPayload {
	return {
		flags: MessageFlags.IsComponentsV2,
		components,
		allowedMentions: NO_ALLOWED_MENTIONS
	};
}

export function botEphemeralReply(...components: BotTopLevelComponent[]): BotReplyPayload {
	return {
		flags: MessageFlags.IsComponentsV2 | MessageFlags.Ephemeral,
		components,
		allowedMentions: NO_ALLOWED_MENTIONS
	};
}

// Discord accepts IsComponentsV2 for deferred interactions, although the discord.js type only
// exposes the Ephemeral bit for this call.
export function botDeferOptions(ephemeral = false): { flags: MessageFlags.Ephemeral } {
	const flags = ephemeral
		? MessageFlags.IsComponentsV2 | MessageFlags.Ephemeral
		: MessageFlags.IsComponentsV2;
	return { flags } as unknown as { flags: MessageFlags.Ephemeral };
}

export function botEditReply(...components: BotTopLevelComponent[]): BotReplyPayload {
	return {
		flags: MessageFlags.IsComponentsV2,
		components,
		allowedMentions: NO_ALLOWED_MENTIONS
	};
}
