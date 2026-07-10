<script lang="ts">
	import { resolve } from '$app/paths';
	import { ArrowLeft, Construction, GitBranch, Rows3 } from 'lucide-svelte';
	import Panel from '$lib/components/layout/Panel.svelte';
	import StatusPill from '$lib/components/layout/StatusPill.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';

	interface Props {
		kicker: string;
		title: string;
		description: string;
		workflow?: string;
	}

	let { kicker, title, description, workflow = 'route contract pending' }: Props = $props();
</script>

<div class="space-y-5">
	<WorkspaceHeader
		{kicker}
		{title}
		description="이 route는 개별 화면 확장보다 공통 shell, guild context, operator state, 작업대 hierarchy 검증을 우선합니다."
	>
		{#snippet actions()}
			<a
				href={resolve('/')}
				class="inline-flex h-9 items-center gap-2 border border-slate-700 px-3 text-xs font-semibold text-slate-200 hover:border-cyan-400/40 hover:text-cyan-100"
			>
				<ArrowLeft size={14} />
				Ops queue
			</a>
		{/snippet}
	</WorkspaceHeader>

	<Panel kicker="Shared route frame" {title} {description}>
		<div class="grid gap-4 lg:grid-cols-[minmax(0,1fr)_260px]">
			<div class="min-w-0">
				<div class="flex items-start gap-3">
					<div
						class="grid size-10 shrink-0 place-items-center border border-cyan-400/40 bg-cyan-400/10"
					>
						<Construction size={18} class="text-cyan-200" />
					</div>
					<div class="min-w-0">
						<p class="text-sm font-semibold text-slate-100">{workflow}</p>
						<p class="mt-2 max-w-3xl text-sm leading-6 text-slate-400">
							{description}
						</p>
					</div>
				</div>
			</div>

			<div class="grid gap-2 border-l border-slate-800 pl-4 max-lg:border-l-0 max-lg:pl-0">
				<div class="flex items-center justify-between gap-3">
					<span class="flex items-center gap-2 text-xs text-slate-500">
						<Rows3 size={14} />
						layout primitive
					</span>
					<StatusPill label="shared" tone="info" />
				</div>
				<div class="flex items-center justify-between gap-3">
					<span class="flex items-center gap-2 text-xs text-slate-500">
						<GitBranch size={14} />
						route depth
					</span>
					<StatusPill label="deferred" />
				</div>
			</div>
		</div>
	</Panel>
</div>
