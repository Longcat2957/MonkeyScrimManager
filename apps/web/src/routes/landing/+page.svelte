<script lang="ts">
	import { resolve } from '$app/paths';
	import { ArrowRight, Bot, LogIn } from 'lucide-svelte';
	import LandingAuctionDraftPreview from '$lib/components/landing/LandingAuctionDraftPreview.svelte';
	import LandingScrimPreview from '$lib/components/landing/LandingScrimPreview.svelte';

	const loginHref = `${resolve('/oauth/[...path]', { path: 'discord/start' })}?${new globalThis.URLSearchParams({ next: '/' })}`;
</script>

<svelte:head>
	<title>MonkeyBot | Discord 내전 운영</title>
	<meta
		name="description"
		content="Discord에서 모집하고 웹에서 League of Legends 내전을 운영하는 MonkeyBot"
	/>
</svelte:head>

<div class="min-h-[100dvh] bg-[var(--color-app-bg)] text-slate-100">
	<header class="border-b border-[var(--color-app-border-muted)] bg-[#0b1018]">
		<div
			class="mx-auto flex min-h-16 max-w-[1240px] items-center justify-between gap-4 px-4 lg:px-6"
		>
			<a
				href={resolve('/landing')}
				class="flex min-w-0 items-center gap-3"
				aria-label="MonkeyBot 소개"
			>
				<img src="/brand/logo.png?v=2" alt="" class="size-10 shrink-0 object-contain" />
				<span class="text-sm font-semibold text-slate-50">MonkeyBot</span>
			</a>

			<nav
				class="hidden items-center gap-5 text-xs font-semibold text-slate-400 sm:flex"
				aria-label="소개 메뉴"
			>
				<a href={resolve('/install')} class="hover:text-cyan-100">설치 안내</a>
				<a href={resolve('/docs')} class="hover:text-cyan-100">문서</a>
			</nav>

			<a
				href={loginHref}
				class="inline-flex h-9 shrink-0 items-center gap-2 rounded-md border border-cyan-300/45 bg-cyan-300/10 px-3 text-xs font-semibold text-cyan-50 hover:border-cyan-200 hover:bg-cyan-300/18"
			>
				<LogIn size={15} aria-hidden="true" />
				<span class="hidden sm:inline">Discord로 시작</span>
				<span class="sm:hidden">시작</span>
			</a>
		</div>
	</header>

	<main>
		<section
			class="relative isolate overflow-hidden border-b border-[var(--color-app-border-muted)]"
		>
			<img
				src="/brand/logo.png?v=2"
				alt=""
				aria-hidden="true"
				class="pointer-events-none absolute -right-36 top-8 size-[25rem] select-none opacity-[0.09] sm:-right-20 sm:size-[31rem] lg:right-[6%] lg:top-10 lg:size-[34rem]"
			/>
			<div class="relative mx-auto max-w-[1240px] px-4 py-14 lg:px-6 lg:py-20">
				<div class="max-w-2xl">
					<p class="text-xs font-semibold text-cyan-200">LEAGUE OF LEGENDS SCRIMS</p>
					<h1 class="mt-4 text-5xl font-semibold text-slate-50 sm:text-6xl">MonkeyBot</h1>
					<p class="mt-5 text-xl font-medium text-slate-200 sm:text-2xl">
						Discord 내전을 한 흐름으로 운영하세요.
					</p>
					<p class="mt-4 max-w-xl text-sm leading-7 text-slate-400 sm:text-base">
						모집, 팀 편성, 픽밴, 결과 기록까지. 필요한 순간에 필요한 화면을 엽니다.
					</p>
					<div class="mt-8 flex flex-wrap items-center gap-3">
						<a
							href={loginHref}
							class="inline-flex h-11 items-center gap-2 rounded-md bg-cyan-300 px-4 text-sm font-semibold text-slate-950 hover:bg-cyan-200"
						>
							Discord로 시작
							<ArrowRight size={17} aria-hidden="true" />
						</a>
						<a
							href={resolve('/install')}
							class="inline-flex h-11 items-center gap-2 rounded-md border border-[var(--color-app-border)] bg-[#111a26] px-4 text-sm font-semibold text-slate-200 hover:border-slate-500 hover:text-slate-50"
						>
							서버에 설치하기
							<Bot size={16} aria-hidden="true" />
						</a>
					</div>
				</div>
			</div>
		</section>

		<section class="border-b border-[var(--color-app-border-muted)] bg-[#0b1018]">
			<div class="mx-auto max-w-[1240px] px-4 py-12 lg:px-6 lg:py-16">
				<div class="mb-7 max-w-2xl">
					<p class="text-xs font-semibold text-cyan-200">WORKSPACE PREVIEW</p>
					<h2 class="mt-3 text-2xl font-semibold text-slate-50">
						내전이 열리면, 이 화면에서 이어갑니다.
					</h2>
					<p class="mt-3 text-sm leading-6 text-slate-400">
						참가자 확정부터 팀 편성, 세트 진행까지 운영에 필요한 상태를 한곳에서 확인합니다.
					</p>
				</div>
				<LandingScrimPreview />
			</div>
		</section>

		<section class="border-b border-[var(--color-app-border-muted)] bg-[#0e1520]">
			<div class="mx-auto max-w-[1240px] px-4 py-12 lg:px-6 lg:py-16">
				<div class="mb-7 max-w-2xl">
					<p class="text-xs font-semibold text-amber-200">AUCTION WORKSPACE PREVIEW</p>
					<h2 class="mt-3 text-2xl font-semibold text-slate-50">
						경매 내전은 후보와 팀을 이 화면에서 관리합니다.
					</h2>
					<p class="mt-3 text-sm leading-6 text-slate-400">
						현재 매물 확인부터 입찰, 유찰 처리와 팀 배치까지 같은 작업대에서 이어집니다.
					</p>
				</div>
				<LandingAuctionDraftPreview />
			</div>
		</section>

		<section class="border-b border-[var(--color-app-border-muted)] bg-[#0e1520]">
			<div
				class="mx-auto flex max-w-[1240px] flex-col gap-6 px-4 py-12 sm:flex-row sm:items-center sm:justify-between lg:px-6"
			>
				<div class="max-w-xl">
					<p class="text-xs font-semibold text-emerald-200">FOR GUILD OPERATORS</p>
					<h2 class="mt-3 text-2xl font-semibold text-slate-50">처음 도입하는 운영자라면</h2>
					<p class="mt-3 text-sm leading-6 text-slate-400">
						Discord 서버에 MonkeyBot을 설치하고 `/초기설정`으로 모집 채널과 운영 역할을 정하세요.
					</p>
				</div>
				<a
					href={resolve('/install')}
					class="inline-flex h-11 shrink-0 items-center justify-center gap-2 rounded-md border border-cyan-300/35 bg-cyan-300/10 px-4 text-sm font-semibold text-cyan-50 hover:border-cyan-200 hover:bg-cyan-300/18"
				>
					설치 안내 보기
					<ArrowRight size={17} aria-hidden="true" />
				</a>
			</div>
		</section>
	</main>

	<footer class="bg-[#0b1018]">
		<div
			class="mx-auto flex min-h-16 max-w-[1240px] flex-wrap items-center justify-between gap-3 px-4 py-4 text-xs lg:px-6"
		>
			<span class="font-semibold text-slate-300">MonkeyBot</span>
			<nav class="flex items-center gap-4 text-slate-500" aria-label="하단 메뉴">
				<a href={resolve('/install')} class="hover:text-cyan-100">설치 안내</a>
				<a href={resolve('/docs')} class="hover:text-cyan-100">문서</a>
				<a href={resolve('/terms')} class="hover:text-cyan-100">이용약관</a>
				<a href={resolve('/privacy')} class="hover:text-cyan-100">개인정보 처리방침</a>
			</nav>
		</div>
	</footer>
</div>
