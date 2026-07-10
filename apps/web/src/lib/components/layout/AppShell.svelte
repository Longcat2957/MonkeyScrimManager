<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import {
		Activity,
		Bell,
		BookOpen,
		Database,
		Gauge,
		RadioTower,
		Search,
		ShieldCheck,
		Swords,
		Trophy,
		UsersRound
	} from 'lucide-svelte';
	import type { Snippet } from 'svelte';
	import StatusPill from './StatusPill.svelte';

	interface NavItem {
		href: string;
		match: string;
		label: string;
		shortLabel: string;
		icon: typeof Gauge;
		phase: string;
	}

	let { children }: { children: Snippet } = $props();

	const guilds = [
		{ name: 'MSM Seoul', status: 'live', active: true },
		{ name: 'Jungle Diff Lab', status: 'pick-ban', active: false },
		{ name: 'Rift Night', status: 'auction', active: false }
	];

	const navItems: NavItem[] = [
		{
			href: resolve('/'),
			match: '/',
			label: 'Ops Queue',
			shortLabel: 'Ops',
			icon: Gauge,
			phase: 'P0'
		},
		{
			href: resolve('/recruitments'),
			match: '/recruitments',
			label: 'Recruitments',
			shortLabel: 'Recruit',
			icon: Bell,
			phase: 'P0'
		},
		{
			href: resolve('/series'),
			match: '/series',
			label: 'Series',
			shortLabel: 'Series',
			icon: Swords,
			phase: 'P0'
		},
		{
			href: resolve('/auction/[id]', { id: 'a-118' }),
			match: '/auction',
			label: 'Auction',
			shortLabel: 'Auction',
			icon: Trophy,
			phase: 'P1'
		},
		{
			href: resolve('/leaderboard'),
			match: '/leaderboard',
			label: 'Records',
			shortLabel: 'Rank',
			icon: Activity,
			phase: 'P1'
		},
		{
			href: resolve('/admin/status'),
			match: '/admin',
			label: 'Admin',
			shortLabel: 'Admin',
			icon: ShieldCheck,
			phase: 'P2'
		}
	];

	const operationContext = [
		{ label: 'Active guild', value: 'MSM Seoul', tone: 'info' },
		{ label: 'Operator role', value: 'scrim-admin', tone: 'success' },
		{ label: 'Open queue', value: '6 recruitments / 3 series', tone: 'warning' },
		{ label: 'Audit checkpoint', value: 'last revert 18m ago', tone: 'neutral' }
	] as const;

	function isActive(match: string): boolean {
		const pathname = page.url.pathname;

		if (match === '/') {
			return pathname === resolve('/');
		}

		return pathname.startsWith(match);
	}
</script>

<div class="min-h-screen bg-[#07090d] text-slate-100">
	<header class="sticky top-0 z-20 border-b border-slate-800/90 bg-[#0b0f16]/95 backdrop-blur">
		<div class="grid min-h-16 gap-3 px-4 py-3 lg:grid-cols-[260px_minmax(320px,1fr)_auto] lg:px-5">
			<a href={resolve('/')} class="flex min-w-0 items-center gap-3">
				<div class="grid size-9 place-items-center border border-cyan-400/40 bg-cyan-400/10">
					<Swords size={18} class="text-cyan-200" />
				</div>
				<div class="min-w-0">
					<p class="truncate text-sm font-semibold tracking-normal text-slate-50">
						MonkeyScrimManager
					</p>
					<p class="truncate text-[11px] text-slate-500">multi-guild scrim console</p>
				</div>
			</a>

			<div class="grid gap-2 md:grid-cols-[220px_minmax(240px,1fr)]">
				<div class="flex min-w-0 items-center border border-slate-800 bg-slate-950/70">
					<UsersRound size={15} class="ml-3 text-slate-500" />
					<select
						aria-label="Active guild"
						class="min-w-0 flex-1 bg-transparent px-2 py-2 text-xs font-semibold text-slate-100 outline-none"
					>
						{#each guilds as guild (guild.name)}
							<option selected={guild.active}>{guild.name} / {guild.status}</option>
						{/each}
					</select>
				</div>

				<div class="flex min-w-0 items-center gap-2 border border-slate-800 bg-slate-950/70 px-3">
					<Search size={15} class="shrink-0 text-slate-500" />
					<input
						aria-label="Command or search"
						placeholder="command, player, champion, series id"
						class="min-w-0 flex-1 bg-transparent py-2 text-sm text-slate-100 outline-none placeholder:text-slate-600"
					/>
				</div>
			</div>

			<div class="flex items-center gap-2 lg:justify-end">
				<StatusPill tone="success" label="Operator" />
				<StatusPill tone="info" label="Mock data" />
			</div>
		</div>

		<nav class="flex gap-1 overflow-x-auto border-t border-slate-800 px-4 py-2 lg:hidden">
			{#each navItems as item (item.href)}
				<a
					href={item.href}
					class={`flex h-9 shrink-0 items-center gap-2 border px-3 text-xs font-semibold ${
						isActive(item.match)
							? 'border-cyan-400/40 bg-cyan-400/10 text-cyan-100'
							: 'border-transparent text-slate-400 hover:border-slate-700 hover:text-slate-100'
					}`}
				>
					<item.icon size={15} />
					<span>{item.shortLabel}</span>
				</a>
			{/each}
		</nav>
	</header>

	<div
		class="grid min-h-[calc(100vh-65px)] lg:grid-cols-[72px_minmax(0,1fr)] xl:grid-cols-[72px_minmax(0,1fr)_300px]"
	>
		<aside class="hidden border-r border-slate-800/80 bg-[#090c12] lg:block">
			<div class="sticky top-16 flex min-h-[calc(100vh-65px)] flex-col items-center gap-2 py-4">
				{#each navItems as item (item.href)}
					<a
						href={item.href}
						aria-label={item.label}
						title={`${item.phase} / ${item.label}`}
						class={`grid size-11 place-items-center border ${
							isActive(item.match)
								? 'border-cyan-400/40 bg-cyan-400/10 text-cyan-100'
								: 'border-transparent text-slate-500 hover:border-slate-700 hover:text-slate-200'
						}`}
					>
						<item.icon size={18} />
					</a>
				{/each}
			</div>
		</aside>

		<main class="min-w-0 px-4 py-5 lg:px-6">{@render children()}</main>

		<aside class="hidden border-l border-slate-800/80 bg-[#090c12] xl:block">
			<div class="sticky top-16 min-h-[calc(100vh-65px)] p-4">
				<section class="border border-slate-800 bg-[#0b0f16]">
					<div class="border-b border-slate-800 px-4 py-3">
						<p class="text-xs font-semibold uppercase text-slate-500">Operations context</p>
						<h2 class="mt-0.5 text-sm font-semibold text-slate-100">현재 작업 기준</h2>
					</div>
					<div class="divide-y divide-slate-800">
						{#each operationContext as item (item.label)}
							<div class="px-4 py-3">
								<p class="text-[11px] uppercase text-slate-500">{item.label}</p>
								<p
									class={`mt-1 text-sm font-semibold ${
										item.tone === 'success'
											? 'text-emerald-100'
											: item.tone === 'warning'
												? 'text-amber-100'
												: item.tone === 'info'
													? 'text-cyan-100'
													: 'text-slate-200'
									}`}
								>
									{item.value}
								</p>
							</div>
						{/each}
					</div>
				</section>

				<section class="mt-4 border border-slate-800 bg-[#0b0f16]">
					<div class="border-b border-slate-800 px-4 py-3">
						<p class="text-xs font-semibold uppercase text-slate-500">Shared capability</p>
						<h2 class="mt-0.5 text-sm font-semibold text-slate-100">Data Dragon</h2>
					</div>
					<div class="space-y-3 p-4 text-sm text-slate-400">
						<div class="flex items-center gap-2">
							<Database size={15} class="text-cyan-200" />
							<span>champion catalog + image proxy</span>
						</div>
						<div class="flex items-center gap-2">
							<RadioTower size={15} class="text-emerald-200" />
							<span>versioned HTTP cache boundary</span>
						</div>
						<a
							href={resolve('/dd/catalog')}
							class="inline-flex h-8 items-center gap-2 border border-slate-700 px-3 text-xs font-semibold text-slate-200 hover:border-cyan-400/40 hover:text-cyan-100"
						>
							<BookOpen size={14} />
							Catalog
						</a>
					</div>
				</section>
			</div>
		</aside>
	</div>
</div>
