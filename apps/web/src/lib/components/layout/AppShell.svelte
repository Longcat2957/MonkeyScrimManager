<script lang="ts">
	import { goto } from '$app/navigation';
	import { navigating, page } from '$app/state';
	import { resolve } from '$app/paths';
	import {
		Activity,
		Bell,
		BookOpen,
		ChevronDown,
		Gamepad2,
		LogOut,
		Search,
		ShieldCheck,
		Swords,
		Trophy,
		UserCircle
	} from 'lucide-svelte';
	import { Avatar, DropdownMenu, Tabs } from 'bits-ui';
	import { cubicOut } from 'svelte/easing';
	import { fade, slide } from 'svelte/transition';
	import type { Snippet } from 'svelte';

	interface NavItem {
		href: string;
		match: string;
		label: string;
		icon: typeof Bell;
	}

	let { children }: { children: Snippet } = $props();
	let searchOpen = $state(false);
	let searchMode = $state<'riot' | 'custom'>('riot');

	const skeletonRows = [0, 1, 2, 3];

	const navItems: NavItem[] = [
		{
			href: resolve('/dev/components'),
			match: '/dev/components',
			label: '컴포넌트',
			icon: BookOpen
		},
		{
			href: resolve('/recruitments'),
			match: '/recruitments',
			label: '모집',
			icon: Bell
		},
		{
			href: resolve('/series'),
			match: '/series',
			label: '내전',
			icon: Swords
		},
		{
			href: resolve('/auction/[id]', { id: 'a-118' }),
			match: '/auction',
			label: '경매',
			icon: Trophy
		},
		{
			href: resolve('/leaderboard'),
			match: '/leaderboard',
			label: '랭킹',
			icon: Activity
		},
		{
			href: resolve('/admin/status'),
			match: '/admin',
			label: '관리',
			icon: ShieldCheck
		}
	];

	function isActive(match: string): boolean {
		const pathname = page.url.pathname;

		if (match === '/') {
			return pathname === resolve('/');
		}
		return pathname.startsWith(match);
	}
</script>

<div class="min-h-screen bg-[var(--color-app-bg)] text-slate-100">
	<header
		class="sticky top-0 z-20 border-b border-[var(--color-app-border)] bg-[var(--color-app-header)] backdrop-blur"
	>
		<div class="flex min-h-14 items-center gap-3 px-4 lg:px-5">
			<a
				href={resolve('/')}
				aria-label="홈으로 이동"
				class="flex min-w-0 shrink-0 items-center gap-3"
			>
				<img src="/brand/logo.png?v=2" alt="" class="size-11 shrink-0 object-contain" />
				<div class="hidden min-w-0 sm:block">
					<p class="truncate text-sm font-semibold tracking-normal text-slate-50">
						MonkeyBot
					</p>
					<p class="truncate text-[11px] text-slate-500">monkeybot.lol</p>
				</div>
			</a>

			<nav class="flex min-w-0 flex-1 gap-1 overflow-x-auto px-1 md:justify-center">
				{#each navItems as item (item.href)}
					<a
						href={item.href}
						aria-label={item.label}
						title={item.label}
						class={`flex h-10 shrink-0 items-center justify-center border px-3 text-xs font-semibold md:min-w-16 md:px-4 ${
							isActive(item.match)
								? 'rounded-md border-cyan-400/40 bg-cyan-400/10 text-cyan-100'
								: 'rounded-sm border-transparent text-slate-400 hover:border-slate-700 hover:text-slate-100'
						}`}
					>
						<item.icon size={16} class="md:hidden" />
						<span class="hidden md:inline">{item.label}</span>
					</a>
				{/each}
			</nav>

			<div class="ml-auto flex shrink-0 items-center gap-2">
				<button
					type="button"
					aria-label={searchOpen ? '검색 닫기' : '검색 열기'}
					aria-expanded={searchOpen}
					onclick={() => {
						searchOpen = !searchOpen;
					}}
					class={`grid size-10 place-items-center border outline-none ${
						searchOpen
							? 'rounded-md border-cyan-400/45 bg-cyan-400/15 text-cyan-100'
							: 'rounded-sm border-transparent bg-transparent text-slate-400 hover:bg-[var(--color-app-hover)] hover:text-cyan-100'
					}`}
				>
					<Search size={17} />
				</button>

				<DropdownMenu.Root>
					<DropdownMenu.Trigger
						aria-label="사용자 메뉴"
						class="flex h-10 items-center gap-2 rounded-sm border border-transparent px-1.5 text-slate-300 outline-none hover:border-slate-700 hover:text-slate-100 data-[state=open]:rounded-md data-[state=open]:border-cyan-400/40 data-[state=open]:bg-cyan-400/10 data-[state=open]:text-cyan-100"
					>
						<Avatar.Root
							class="grid size-8 place-items-center overflow-hidden border border-cyan-400/35 bg-cyan-400/10"
						>
							<Avatar.Fallback class="text-xs font-bold text-cyan-100">관</Avatar.Fallback>
						</Avatar.Root>
						<ChevronDown size={14} class="hidden text-slate-500 sm:block" />
					</DropdownMenu.Trigger>
					<DropdownMenu.Portal>
						<DropdownMenu.Content
							align="end"
							sideOffset={8}
							class="z-50 min-w-56 border border-[var(--color-app-border)] bg-[var(--color-app-surface)] p-1 text-sm text-slate-200 shadow-2xl shadow-black/40 outline-none"
						>
							<div class="px-3 py-2">
								<p class="font-semibold text-slate-50">내전 관리자</p>
								<p class="mt-0.5 text-xs text-slate-500">MSM Seoul</p>
							</div>
							<DropdownMenu.Separator class="my-1 h-px bg-[var(--color-app-border-muted)]" />
							<DropdownMenu.Item
								class="flex h-9 cursor-default items-center gap-2 px-3 text-xs font-semibold outline-none data-[highlighted]:bg-[var(--color-app-hover)] data-[highlighted]:text-slate-50"
								onclick={() => goto(resolve('/users/[discordId]', { discordId: 'user-tamsol' }))}
							>
								<UserCircle size={15} class="text-slate-500" />
								내 정보
							</DropdownMenu.Item>
							<DropdownMenu.Item
								class="flex h-9 cursor-default items-center gap-2 px-3 text-xs font-semibold outline-none data-[highlighted]:bg-[var(--color-app-hover)] data-[highlighted]:text-slate-50"
								onclick={() => goto(resolve('/me/riot-accounts'))}
							>
								<Gamepad2 size={15} class="text-slate-500" />
								Riot 계정
							</DropdownMenu.Item>
							<DropdownMenu.Item
								class="flex h-9 cursor-default items-center gap-2 px-3 text-xs font-semibold outline-none data-[highlighted]:bg-[var(--color-app-hover)] data-[highlighted]:text-slate-50"
							>
								<ShieldCheck size={15} class="text-slate-500" />
								서버 설정
							</DropdownMenu.Item>
							<DropdownMenu.Separator class="my-1 h-px bg-[var(--color-app-border-muted)]" />
							<DropdownMenu.Item
								class="flex h-9 cursor-default items-center gap-2 px-3 text-xs font-semibold text-red-100 outline-none data-[highlighted]:bg-red-500/10 data-[highlighted]:text-red-50"
							>
								<LogOut size={15} />
								나가기
							</DropdownMenu.Item>
						</DropdownMenu.Content>
					</DropdownMenu.Portal>
				</DropdownMenu.Root>
			</div>
		</div>

		{#if searchOpen}
			<div
				transition:slide={{ duration: 180, easing: cubicOut }}
				class="overflow-hidden border-t border-[var(--color-app-border-muted)] bg-[var(--color-app-bg-soft)]"
			>
				<Tabs.Root
					value={searchMode}
					onValueChange={(value) => {
						searchMode = value === 'custom' ? 'custom' : 'riot';
					}}
					class="mx-auto grid max-w-[1680px] gap-3 px-4 py-3 md:grid-cols-[180px_minmax(0,720px)] md:items-center lg:px-5"
				>
					<Tabs.List
						aria-label="검색 종류"
						class="grid grid-cols-2 border border-[var(--color-app-border)] bg-[var(--color-app-surface)] p-1"
					>
						<Tabs.Trigger
							value="riot"
							class="h-9 rounded-sm text-xs font-semibold text-slate-400 outline-none hover:text-slate-100 data-[state=active]:rounded-md data-[state=active]:bg-cyan-400/10 data-[state=active]:text-cyan-100"
						>
							Riot
						</Tabs.Trigger>
						<Tabs.Trigger
							value="custom"
							class="h-9 rounded-sm text-xs font-semibold text-slate-400 outline-none hover:text-slate-100 data-[state=active]:rounded-md data-[state=active]:bg-cyan-400/10 data-[state=active]:text-cyan-100"
						>
							Custom
						</Tabs.Trigger>
					</Tabs.List>

					<div
						class="flex min-w-0 items-center gap-2 rounded-sm border border-cyan-400/25 bg-[#08131d] px-3 shadow-[inset_0_1px_0_rgb(255_255_255_/_0.04)]"
					>
						<input
							aria-label={searchMode === 'riot' ? 'Riot 검색' : '내전 기록 검색'}
							placeholder={searchMode === 'riot'
								? 'Riot ID 또는 소환사명을 검색하세요'
								: '참가자, 팀, 내전 ID를 검색하세요'}
							class="min-w-0 flex-1 bg-transparent py-3 text-sm font-medium text-slate-50 outline-none placeholder:text-slate-500"
						/>
						<button
							type="button"
							aria-label="검색"
							class="grid size-9 shrink-0 place-items-center rounded-sm text-cyan-100 hover:bg-cyan-400/10 hover:text-cyan-50"
						>
							<Search size={17} />
						</button>
					</div>
				</Tabs.Root>
			</div>
		{/if}
	</header>

	<div class="min-h-[calc(100vh-57px)]">
		{#if navigating.to}
			<div
				transition:fade={{ duration: 120, easing: cubicOut }}
				class="border-b border-cyan-400/20 bg-[var(--color-app-bg-soft)]"
				aria-live="polite"
				aria-label="페이지 이동 중"
			>
				<div class="mx-auto flex max-w-[1680px] items-center gap-4 px-4 py-3 lg:px-6">
					<div
						class="size-4 shrink-0 animate-spin rounded-full border-2 border-cyan-300/25 border-t-cyan-200"
					></div>
					<div class="grid min-w-0 flex-1 gap-2">
						<div class="h-2 w-40 animate-pulse bg-cyan-300/25"></div>
						<div class="h-2 w-full max-w-lg animate-pulse bg-slate-700/60"></div>
					</div>
				</div>
			</div>
		{/if}

		<main class="mx-auto min-w-0 max-w-[1680px] px-4 py-5 lg:px-6">
			{#if navigating.to}
				<section
					transition:fade={{ duration: 120, easing: cubicOut }}
					class="space-y-5"
					aria-live="polite"
					aria-label="화면을 불러오는 중"
				>
					<div
						class="border border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] p-4"
					>
						<div class="flex items-center gap-3">
							<div
								class="size-5 shrink-0 animate-spin rounded-full border-2 border-cyan-300/25 border-t-cyan-200"
							></div>
							<div class="min-w-0 flex-1">
								<div class="h-3 w-36 animate-pulse bg-cyan-300/25"></div>
								<div class="mt-3 h-6 w-full max-w-xl animate-pulse bg-slate-700/70"></div>
								<div class="mt-2 h-3 w-full max-w-3xl animate-pulse bg-slate-800"></div>
							</div>
						</div>
					</div>

					<div class="grid gap-5 2xl:grid-cols-[minmax(0,1.35fr)_minmax(360px,0.65fr)]">
						<div class="space-y-5">
							<div
								class="border border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)]"
							>
								<div class="border-b border-[var(--color-app-border-muted)] px-4 py-3">
									<div class="h-3 w-24 animate-pulse bg-slate-700/70"></div>
									<div class="mt-2 h-4 w-48 animate-pulse bg-slate-800"></div>
								</div>
								<div class="divide-y divide-[var(--color-app-border-muted)]">
									{#each skeletonRows as row (row)}
										<div class="grid gap-3 px-4 py-4 md:grid-cols-[88px_minmax(0,1fr)_112px]">
											<div class="h-7 w-16 animate-pulse bg-slate-800"></div>
											<div class="min-w-0">
												<div class="h-4 w-full max-w-md animate-pulse bg-slate-700/70"></div>
												<div class="mt-2 h-3 w-44 animate-pulse bg-slate-800"></div>
											</div>
											<div class="h-4 w-20 animate-pulse bg-cyan-300/20"></div>
										</div>
									{/each}
								</div>
							</div>
						</div>

						<div
							class="border border-[var(--color-app-border-muted)] bg-[var(--color-app-surface)] p-4"
						>
							<div class="h-3 w-20 animate-pulse bg-slate-700/70"></div>
							<div class="mt-3 grid gap-3">
								<div class="h-16 animate-pulse bg-slate-800"></div>
								<div class="h-16 animate-pulse bg-slate-800"></div>
								<div class="h-16 animate-pulse bg-slate-800"></div>
							</div>
						</div>
					</div>
				</section>
			{:else}
				{#key page.url.pathname}
					<div in:fade={{ duration: 120, easing: cubicOut }}>{@render children()}</div>
				{/key}
			{/if}
		</main>
	</div>
</div>
