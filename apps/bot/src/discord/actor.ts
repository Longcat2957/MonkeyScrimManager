import { GuildMember, type GuildBasedChannel, type User } from 'discord.js';
import type { Actor } from '../api/contracts.js';

interface GuildInteractionIdentity {
	user: User;
	member: GuildMember | { nick?: string | null; roles: string[] } | null;
}

export function actorFromInteraction(interaction: GuildInteractionIdentity): Actor {
	const member = interaction.member;
	const displayName =
		member instanceof GuildMember
			? member.displayName
			: (member?.nick ?? interaction.user.globalName ?? interaction.user.username);
	const roleIds =
		member instanceof GuildMember ? [...member.roles.cache.keys()] : [...(member?.roles ?? [])];

	return {
		id: interaction.user.id,
		globalDisplayName: interaction.user.globalName ?? interaction.user.username,
		displayName,
		avatarHash: interaction.user.avatar,
		roleIds
	};
}

export function messageChannelId(channel: GuildBasedChannel | null): string | null {
	return channel?.id ?? null;
}
