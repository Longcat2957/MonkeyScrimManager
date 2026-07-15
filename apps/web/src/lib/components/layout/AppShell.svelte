<script lang="ts">
	import { afterNavigate, goto } from '$app/navigation';
	import { navigating, page } from '$app/state';
	import { resolve } from '$app/paths';
	import {
		Activity,
		Check,
		ChevronDown,
		ChevronsUpDown,
		LoaderCircle,
		LogOut,
		Search,
		Server,
		ShieldCheck,
		Swords,
		Trophy,
		UserCircle,
		UsersRound,
		X
	} from 'lucide-svelte';
	import { Avatar, DropdownMenu, Tabs } from 'bits-ui';
	import { cubicOut } from 'svelte/easing';
	import { fade, slide } from 'svelte/transition';
	import type { Snippet } from 'svelte';
	import QuickActionFab from '$lib/components/fab/QuickActionFab.svelte';
	import AppFooter from '$lib/components/layout/AppFooter.svelte';
	import { fetchSearchSuggestions, type SearchSuggestion } from '$lib/api/searchSuggestions';

	interface NavItem {
		href: string;
		match: string;
		label: string;
		icon: typeof Activity;
	}

	interface ShellGuild {
		id: string;
		displayName: string;
		memberCount: number;
		currentUserRole: 'MEMBER' | 'OPERATOR';
	}

	interface ShellUser {
		displayName: string;
		defaultGuildId: string | null;
		serviceAdmin?: boolean;
	}

	interface ShellPageData {
		guild?: ShellGuild;
		guilds?: ShellGuild[];
		user?: ShellUser;
	}

	interface SearchKeyEvent {
		key: string;
		preventDefault: () => void;
	}

	let { children }: { children: Snippet } = $props();
	let searchOpen = $state(false);
	let searchMode = $state<'riot' | 'custom'>('riot');
	let searchQuery = $state('');
	let searchInputFocused = $state(false);
	let searchSuggestions = $state<SearchSuggestion[]>([]);
	let searchSuggestionsLoading = $state(false);
	let highlightedSuggestion = $state(-1);
	let loggingOut = $state(false);
	let logoutError = $state<string | null>(null);
	const searchSuggestionsId = $props.id();

	afterNavigate(() => {
		searchOpen = false;
		searchInputFocused = false;
		searchSuggestions = [];
		highlightedSuggestion = -1;
	});

	const skeletonRows = [0, 1, 2, 3];
	const shellData = $derived(page.data as ShellPageData);
	const availableGuilds = $derived(shellData.guilds ?? []);
	const activeGuildId = $derived(
		page.params.guildId ??
			shellData.guild?.id ??
			shellData.user?.defaultGuildId ??
			availableGuilds[0]?.id ??
			''
	);
	const activeGuild = $derived(
		shellData.guild ?? availableGuilds.find((guild) => guild.id === activeGuildId) ?? null
	);
	const activeGuildBase = $derived(
		activeGuildId ? resolve('/g/[guildId]', { guildId: activeGuildId }) : ''
	);
	const staticDocumentRouteIds = ['/docs', '/terms', '/privacy'];
	const viewportWorkspaceRouteIds = ['/g/[guildId]/recruitments/[id]'];
	const usesViewportWorkspace = $derived(
		viewportWorkspaceRouteIds.includes(page.route.id ?? '')
	);
	const usesViewportShell = $derived(
		usesViewportWorkspace || (page.route.id ?? '').startsWith('/g/[guildId]/admin')
	);
	const showQuickActionFab = $derived(
		!staticDocumentRouteIds.includes(page.route.id ?? '') && !usesViewportWorkspace
	);
	const navItems: NavItem[] = $derived(
		activeGuildId
			? [
					{
						href: resolve('/g/[guildId]/series', { guildId: activeGuildId }),
						match: `${activeGuildBase}/series`,
						label: '내전',
						icon: Swords
					},
					{
						href: resolve('/g/[guildId]/auction', { guildId: activeGuildId }),
						match: `${activeGuildBase}/auction`,
						label: '경매',
						icon: Trophy
					},
					{
						href: resolve('/g/[guildId]/leaderboard', { guildId: activeGuildId }),
						match: `${activeGuildBase}/leaderboard`,
						label: '랭킹',
						icon: Activity
					},
					...(activeGuild?.currentUserRole === 'OPERATOR'
						? [
								{
									href: resolve('/g/[guildId]/admin', { guildId: activeGuildId }),
									match: `${activeGuildBase}/admin`,
									label: '관리',
									icon: ShieldCheck
								}
							]
						: [])
				]
			: []
	);
	const showSearchSuggestions = $derived(
		searchInputFocused &&
			searchQuery.trim().length >= 2 &&
			(searchSuggestionsLoading || searchSuggestions.length > 0)
	);

	$effect(() => {
		const query = searchQuery.trim();
		const mode = searchMode;
		const guildId = activeGuildId;
		searchSuggestions = [];
		highlightedSuggestion = -1;

		if (!searchOpen || query.length < 2 || (mode === 'custom' && !guildId)) {
			searchSuggestionsLoading = false;
			return;
		}

		const controller = new globalThis.AbortController();
		searchSuggestionsLoading = true;
		const timer = globalThis.setTimeout(async () => {
			try {
				searchSuggestions = await fetchSearchSuggestions(globalThis.fetch, {
					mode,
					query,
					guildId,
					signal: controller.signal
				});
			} catch (error) {
				if (!(error instanceof globalThis.DOMException && error.name === 'AbortError')) {
					searchSuggestions = [];
				}
			} finally {
				if (!controller.signal.aborted) searchSuggestionsLoading = false;
			}
		}, 200);

		return () => {
			globalThis.clearTimeout(timer);
			controller.abort();
		};
	});

	function switchGuild(guildId: string) {
		goto(resolve('/g/[guildId]', { guildId }));
	}

	function submitSearch() {
		const target =
			searchMode === 'riot' || !activeGuildId
				? resolve('/records')
				: resolve('/g/[guildId]/records', { guildId: activeGuildId });
		const query = searchQuery.trim();
		goto(query ? `${target}?q=${globalThis.encodeURIComponent(query)}` : target);
		searchOpen = false;
		searchInputFocused = false;
	}

	function selectSearchSuggestion(suggestion: SearchSuggestion) {
		searchQuery = suggestion.value;
		submitSearch();
	}

	function handleSearchKeydown(event: SearchKeyEvent) {
		if (event.key === 'ArrowDown' && searchSuggestions.length > 0) {
			event.preventDefault();
			highlightedSuggestion =
				(highlightedSuggestion + 1 + searchSuggestions.length) % searchSuggestions.length;
			return;
		}
		if (event.key === 'ArrowUp' && searchSuggestions.length > 0) {
			event.preventDefault();
			highlightedSuggestion =
				(highlightedSuggestion - 1 + searchSuggestions.length) % searchSuggestions.length;
			return;
		}
		if (event.key === 'Escape') {
			event.preventDefault();
			searchInputFocused = false;
			return;
		}
		if (event.key !== 'Enter') return;
		event.preventDefault();
		const suggestion = searchSuggestions[highlightedSuggestion];
		if (showSearchSuggestions && suggestion) {
			selectSearchSuggestion(suggestion);
			return;
		}
		submitSearch();
	}

	function readCookie(name: string): string | null {
		const prefix = `${name}=`;
		const value = globalThis.document.cookie
			.split(';')
			.map((cookie) => cookie.trim())
			.find((cookie) => cookie.startsWith(prefix));
		return value ? decodeURIComponent(value.slice(prefix.length)) : null;
	}

	async function logoutErrorMessage(response: Response): Promise<string> {
		const fallback =
			response.status === 403
				? '로그아웃 요청을 확인할 수 없습니다. 새로고침 후 다시 시도해 주세요.'
				: '로그아웃 요청을 처리하지 못했습니다. 잠시 후 다시 시도해 주세요.';

		try {
			const payload: unknown = await response.json();
			if (!payload || typeof payload !== 'object' || !('error' in payload)) return fallback;

			const error = payload.error;
			if (!error || typeof error !== 'object') return fallback;

			const message =
				'message' in error && typeof error.message === 'string' ? error.message : fallback;
			const requestId =
				'requestId' in error && typeof error.requestId === 'string' ? error.requestId : null;

			return requestId ? `${message} (요청 ID: ${requestId})` : message;
		} catch {
			return fallback;
		}
	}

	async function logout() {
		if (loggingOut) return;

		loggingOut = true;
		logoutError = null;
		const csrfToken = readCookie('monkeybot_csrf');

		try {
			const response = await globalThis.fetch(resolve('/api/[...path]', { path: 'logout' }), {
				method: 'POST',
				credentials: 'same-origin',
				headers: csrfToken ? { 'X-CSRF-Token': csrfToken } : undefined
			});

			if (response.status === 401) {
				await goto(resolve('/landing'));
				return;
			}

			if (!response.ok) {
				logoutError = await logoutErrorMessage(response);
				return;
			}

			await goto(resolve('/landing'));
		} catch {
			logoutError = '로그아웃 요청을 전송하지 못했습니다. 네트워크 연결을 확인해 주세요.';
		} finally {
			loggingOut = false;
		}
	}

	function isActive(match: string): boolean {
		const pathname = page.url.pathname;

		if (match === '/') {
			return pathname === resolve('/');
		}
		return pathname.startsWith(match);
	}
</script>

<div
	class={`flex flex-col bg-[var(--color-app-bg)] text-slate-100 ${
		usesViewportShell ? 'h-[100dvh] overflow-hidden' : 'min-h-screen'
	}`}
>
	<header
		class="sticky top-0 z-20 shrink-0 border-b border-[var(--color-app-border)] bg-[var(--color-app-header)] backdrop-blur"
	>
		<div class="flex min-h-14 items-center gap-3 px-4 lg:px-5">
			<a
				href={resolve('/')}
				aria-label="홈으로 이동"
				class="flex min-w-0 shrink-0 items-center gap-3"
			>
				<img src="/brand/logo.png?v=2" alt="" class="size-11 shrink-0 object-contain" />
				<div class="hidden min-w-0 sm:block">
					<p class="truncate text-sm font-semibold tracking-normal text-slate-50">MonkeyBot</p>
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
				{#if activeGuild && availableGuilds.length > 0}
					<DropdownMenu.Root>
						<DropdownMenu.Trigger
							aria-label="운영 서버 선택"
							class="flex h-10 max-w-44 shrink-0 items-center gap-2 border border-[var(--color-app-border)] bg-[var(--color-app-control)] px-2.5 text-slate-300 outline-none hover:border-cyan-400/35 hover:text-slate-50 data-[state=open]:border-cyan-400/40 data-[state=open]:bg-cyan-400/10"
						>
							<Server size={15} class="shrink-0 text-cyan-200" aria-hidden="true" />
							<span class="hidden truncate text-xs font-semibold xl:block"
								>{activeGuild.displayName}</span
							>
							<ChevronsUpDown size={13} class="shrink-0 text-slate-500" aria-hidden="true" />
						</DropdownMenu.Trigger>
						<DropdownMenu.Portal>
							<DropdownMenu.Content
								align="end"
								sideOffset={8}
								class="z-50 min-w-64 border border-[var(--color-app-border)] bg-[var(--color-app-surface)] p-1 text-sm text-slate-200 shadow-2xl shadow-black/40 outline-none"
							>
								<div class="px-3 py-2 text-[10px] font-semibold text-slate-600">Discord 서버</div>
								{#each availableGuilds as guild (guild.id)}
									<DropdownMenu.Item
										class="grid h-11 cursor-default grid-cols-[minmax(0,1fr)_18px] items-center gap-3 px-3 outline-none data-[highlighted]:bg-[var(--color-app-hover)]"
										onclick={() => switchGuild(guild.id)}
									>
										<span class="min-w-0">
											<strong class="block truncate text-xs text-slate-100"
												>{guild.displayName}</strong
											>
											<small class="text-[10px] text-slate-600"
												>{guild.currentUserRole === 'OPERATOR' ? '운영자' : '참가자'} · {guild.memberCount}명</small
											>
										</span>
										{#if guild.id === activeGuildId}
											<Check size={14} class="text-cyan-200" aria-hidden="true" />
										{/if}
									</DropdownMenu.Item>
								{/each}
							</DropdownMenu.Content>
						</DropdownMenu.Portal>
					</DropdownMenu.Root>
				{/if}

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
							<Avatar.Fallback class="text-xs font-bold text-cyan-100"
								>{shellData.user?.displayName.slice(0, 1) ?? 'M'}</Avatar.Fallback
							>
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
								<p class="font-semibold text-slate-50">
									{shellData.user?.displayName ?? '내 정보'}
								</p>
								{#if activeGuild}
									<p class="mt-0.5 text-xs text-slate-500">{activeGuild.displayName}</p>
								{/if}
							</div>
							<DropdownMenu.Separator class="my-1 h-px bg-[var(--color-app-border-muted)]" />
							<DropdownMenu.Item
								class="flex h-9 cursor-default items-center gap-2 px-3 text-xs font-semibold outline-none data-[highlighted]:bg-[var(--color-app-hover)] data-[highlighted]:text-slate-50"
								onclick={() => goto(resolve('/me'))}
							>
								<UserCircle size={15} class="text-slate-500" />
								내 정보
							</DropdownMenu.Item>
							{#if shellData.user?.serviceAdmin}
								<DropdownMenu.Item
									class="flex h-9 cursor-default items-center gap-2 px-3 text-xs font-semibold outline-none data-[highlighted]:bg-[var(--color-app-hover)] data-[highlighted]:text-slate-50"
									onclick={() => goto(resolve('/dev/users'))}
								>
									<UsersRound size={15} class="text-slate-500" />
									더미 사용자
								</DropdownMenu.Item>
							{/if}
							<DropdownMenu.Separator class="my-1 h-px bg-[var(--color-app-border-muted)]" />
							<DropdownMenu.Item
								class="flex h-9 cursor-default items-center gap-2 px-3 text-xs font-semibold text-red-100 outline-none data-[highlighted]:bg-red-500/10 data-[highlighted]:text-red-50 aria-disabled:opacity-45"
								aria-disabled={loggingOut}
								onclick={logout}
							>
								{#if loggingOut}
									<LoaderCircle size={15} class="animate-spin" aria-hidden="true" />
									나가는 중
								{:else}
									<LogOut size={15} aria-hidden="true" />
									나가기
								{/if}
							</DropdownMenu.Item>
						</DropdownMenu.Content>
					</DropdownMenu.Portal>
				</DropdownMenu.Root>
			</div>
		</div>

		{#if logoutError}
			<div class="border-t border-red-400/20 bg-red-500/10">
				<div class="mx-auto flex max-w-[1680px] items-center gap-3 px-4 py-2 lg:px-5">
					<p role="alert" class="min-w-0 flex-1 text-xs font-medium text-red-100">
						{logoutError}
					</p>
					<button
						type="button"
						aria-label="로그아웃 오류 닫기"
						class="grid size-6 shrink-0 place-items-center text-red-200 outline-none hover:bg-red-500/15 hover:text-red-50"
						onclick={() => {
							logoutError = null;
						}}
					>
						<X size={14} aria-hidden="true" />
					</button>
				</div>
			</div>
		{/if}

		{#if searchOpen}
			<div
				transition:slide={{ duration: 180, easing: cubicOut }}
				class="pointer-events-none absolute inset-x-0 top-full"
			>
				<Tabs.Root
					value={searchMode}
					onValueChange={(value) => {
						searchMode = value === 'custom' ? 'custom' : 'riot';
					}}
					class="pointer-events-auto relative z-10 mx-auto mt-3 h-12 w-[calc(100%-2rem)] max-w-[860px] lg:w-[calc(100%-2.5rem)]"
				>
					<div
						class="flex h-full w-full items-stretch overflow-hidden rounded-md border border-cyan-300/20 bg-[var(--color-app-surface)] shadow-[0_14px_36px_rgb(0_0_0_/_0.42)] transition-colors focus-within:border-cyan-300/45"
					>
						<Tabs.List
							aria-label="검색 종류"
							class="flex shrink-0 items-center gap-0.5 border-r border-[var(--color-app-border-muted)] bg-[var(--color-app-bg-soft)] p-1"
						>
							<Tabs.Trigger
								value="riot"
								class="h-full min-w-12 rounded-sm px-2 text-[11px] font-semibold text-slate-500 outline-none hover:text-slate-200 data-[state=active]:bg-cyan-400/10 data-[state=active]:text-cyan-100 sm:min-w-16"
							>
								Riot
							</Tabs.Trigger>
							<Tabs.Trigger
								value="custom"
								class="h-full min-w-12 rounded-sm px-2 text-[11px] font-semibold text-slate-500 outline-none hover:text-slate-200 data-[state=active]:bg-cyan-400/10 data-[state=active]:text-cyan-100 sm:min-w-16"
							>
								내전
							</Tabs.Trigger>
						</Tabs.List>

						<div class="flex min-w-0 flex-1 items-center gap-1 px-2 pl-3 sm:pl-4">
							<input
								aria-label={searchMode === 'riot' ? 'Riot 검색' : '내전 기록 검색'}
								role="combobox"
								autocomplete="off"
								aria-autocomplete="list"
								aria-expanded={showSearchSuggestions}
								aria-controls={searchSuggestionsId}
								aria-activedescendant={highlightedSuggestion >= 0
									? `${searchSuggestionsId}-${highlightedSuggestion}`
									: undefined}
								placeholder={searchMode === 'riot'
									? 'GameName#TagLine'
									: '참가자, 팀, 내전 ID를 검색하세요'}
								bind:value={searchQuery}
								onfocus={() => {
									searchInputFocused = true;
								}}
								oninput={() => {
									searchInputFocused = true;
									highlightedSuggestion = -1;
								}}
								onkeydown={handleSearchKeydown}
								onblur={() => {
									searchInputFocused = false;
								}}
								class="h-full min-w-0 flex-1 bg-transparent text-sm font-medium text-slate-100 outline-none placeholder:font-normal placeholder:text-slate-600"
							/>
							<button
								type="button"
								aria-label="검색"
								onclick={submitSearch}
								class="grid size-9 shrink-0 place-items-center rounded-sm bg-cyan-400/10 text-cyan-100 hover:bg-cyan-400/15 hover:text-cyan-50"
							>
								<Search size={17} />
							</button>
						</div>
					</div>

					{#if showSearchSuggestions}
						<div
							id={searchSuggestionsId}
							role="listbox"
							aria-label="검색어 자동완성"
							class="absolute top-[calc(100%+0.625rem)] right-0 left-0 z-30 overflow-hidden rounded-md border border-[var(--color-app-border)] bg-[var(--color-app-surface)] p-1 shadow-[0_18px_42px_rgb(0_0_0_/_0.48)]"
						>
							{#if searchSuggestionsLoading}
								<div class="flex h-11 items-center gap-2 px-3 text-xs text-slate-500">
									<LoaderCircle size={14} class="animate-spin" aria-hidden="true" />
									검색 중
								</div>
							{:else}
								{#each searchSuggestions as suggestion, index (`${suggestion.kind}-${suggestion.value}`)}
									<button
										type="button"
										id={`${searchSuggestionsId}-${index}`}
										role="option"
										aria-selected={index === highlightedSuggestion}
										class={`grid h-11 w-full grid-cols-[minmax(0,1fr)_auto] items-center gap-3 rounded-sm px-3 text-left ${
											index === highlightedSuggestion
												? 'bg-cyan-400/10 text-cyan-50'
												: 'text-slate-200 hover:bg-[var(--color-app-hover)]'
										}`}
										onmouseenter={() => {
											highlightedSuggestion = index;
										}}
										onmousedown={(event) => {
											event.preventDefault();
										}}
										onclick={() => selectSearchSuggestion(suggestion)}
									>
										<span class="min-w-0">
											<strong class="block truncate text-xs font-semibold"
												>{suggestion.label}</strong
											>
											{#if suggestion.description}
												<small class="mt-0.5 block truncate text-[10px] font-medium text-slate-500"
													>{suggestion.description}</small
												>
											{/if}
										</span>
										<span class="text-[10px] font-semibold text-slate-600">
											{suggestion.kind === 'riotAccount' ? 'Riot' : '길드 멤버'}
										</span>
									</button>
								{/each}
							{/if}
						</div>
					{/if}
				</Tabs.Root>
			</div>
		{/if}
	</header>

	{#if searchOpen}
		<button
			type="button"
			aria-label="검색 닫기"
			transition:fade={{ duration: 140 }}
			onclick={() => {
				searchOpen = false;
				searchInputFocused = false;
			}}
			class="fixed inset-x-0 top-14 bottom-0 z-10 cursor-default border-0 bg-black/30 p-0"
		></button>
	{/if}

	<div class={`flex min-h-0 flex-1 flex-col ${usesViewportShell ? 'overflow-hidden' : ''}`}>
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

		<main
			inert={searchOpen}
			class={`mx-auto flex min-h-0 w-full min-w-0 max-w-[1680px] flex-1 flex-col px-4 pt-5 transition-[filter,opacity] duration-150 lg:px-6 ${
				showQuickActionFab ? 'pb-40 lg:pb-36' : 'pb-5'
			} ${usesViewportShell ? 'overflow-hidden' : ''} ${searchOpen ? 'blur-[2px] opacity-55' : ''}`}
		>
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
					<div class="flex min-h-0 flex-1 flex-col" in:fade={{ duration: 120, easing: cubicOut }}>
						{@render children()}
					</div>
				{/key}
			{/if}
		</main>

		<AppFooter reserveFabSpace={showQuickActionFab} />
	</div>

	{#if showQuickActionFab && !searchOpen}
		<QuickActionFab />
	{/if}
</div>
