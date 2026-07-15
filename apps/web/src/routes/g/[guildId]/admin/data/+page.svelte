<script lang="ts">
	import GuildDataTransferPanel from '$lib/components/workflow/GuildDataTransferPanel.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const activeDiscordMemberCount = $derived(
		data.memberSnapshot.members.filter(
			(member) => member.identityKind === 'DISCORD' && member.status === 'ACTIVE'
		).length
	);
</script>

<GuildDataTransferPanel
	guildId={data.guildId}
	guildName={data.guild.displayName}
	activeMemberCount={activeDiscordMemberCount}
	currentSeasonName={data.seasonSnapshot.current.name}
/>
