import type { ChatInputCommandInteraction, SlashCommandBuilder } from 'discord.js';
import type { InteractionContext } from '../interactions/context.js';

export interface BotCommand {
	data: Pick<SlashCommandBuilder, 'name' | 'toJSON'>;
	execute(interaction: ChatInputCommandInteraction, context: InteractionContext): Promise<void>;
}
