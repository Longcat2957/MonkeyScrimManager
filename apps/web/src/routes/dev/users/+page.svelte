<script lang="ts">
	import DummyUserWorkspace from '$lib/components/dev/DummyUserWorkspace.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
</script>

<DummyUserWorkspace
	initialUsers={data.userSnapshot.users}
	initialTotal={data.userSnapshot.total}
	initialGuildId={data.initialGuildId}
	initialGroups={data.groups}
	guilds={data.dummyGuildCatalog}
	championCatalog={data.championCatalog}
/>
