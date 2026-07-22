<script lang="ts">
	import { page } from '$app/state';
	import { TriangleAlert } from 'lucide-svelte';
	import AppShell from '$lib/components/layout/AppShell.svelte';

	const isPublicRoute = $derived(['/landing', '/install'].includes(page.route.id ?? ''));
</script>

{#snippet errorContent()}
	<section class="mx-auto flex min-h-[70vh] max-w-3xl flex-col justify-center px-6">
		<div class="flex items-center gap-3 border border-red-400/30 bg-red-500/10 p-4">
			<TriangleAlert size={22} class="text-red-200" />
			<div>
				<p class="text-sm font-semibold text-red-100">{page.status}</p>
				<h1 class="mt-1 text-2xl font-semibold text-slate-50">
					{page.error?.message ?? 'Route error'}
				</h1>
			</div>
		</div>
	</section>
{/snippet}

{#if isPublicRoute}
	{@render errorContent()}
{:else}
	<AppShell>{@render errorContent()}</AppShell>
{/if}
