import { GuildMember, type GuildBasedChannel, type User } from 'discord.js';
import type { Actor } from '../api/contracts.js';

interface GuildInteractionIdentity {
	user: User;
	member: GuildMember | { nick?: string | null; roles: string[] } | null;
}

export function actorFromInteraction(interaction: GuildInteractionIdentity): Actor {
	const member = interaction.member;
	if (member instanceof GuildMember) return actorFromGuildMember(interaction.user, member);
	const displayName = member?.nick ?? interaction.user.globalName ?? interaction.user.username;
	const roleIds = [...(member?.roles ?? [])];

	return {
		id: interaction.user.id,
		globalDisplayName: interaction.user.globalName ?? interaction.user.username,
		displayName,
		avatarHash: interaction.user.avatar,
		roleIds
	};
}

export function actorFromGuildMember(user: User, member: GuildMember): Actor {
	return {
		id: user.id,
		globalDisplayName: user.globalName ?? user.username,
		displayName: member.displayName,
		avatarHash: user.avatar,
		roleIds: [...member.roles.cache.keys()]
	};
}

export function messageChannelId(channel: GuildBasedChannel | null): string | null {
	return channel?.id ?? null;
}
