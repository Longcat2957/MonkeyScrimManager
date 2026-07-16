<script lang="ts">
	import { resolve } from '$app/paths';
	import {
		ArrowRight,
		Bot,
		Check,
		ChevronRight,
		LogIn,
		Settings2,
		ShieldCheck,
		UserRoundPlus
	} from 'lucide-svelte';

	const loginHref = `${resolve('/oauth/[...path]', { path: 'discord/start' })}?${new globalThis.URLSearchParams({ next: '/' })}`;
	const installHref = 'https://discord.com/oauth2/authorize?client_id=1525847677542207548';

	const setupSteps = [
		{
			label: 'Bot 추가',
			title: '운영할 Discord 서버에 MonkeyBot을 추가하세요.',
			description:
				'서버 관리 권한이 있는 Discord 계정으로 설치 화면에서 대상 서버를 선택합니다. 설치 후에는 해당 서버의 채널에서 명령을 실행합니다.',
			icon: Bot,
			command: undefined,
			points: [
				'한 Discord 서버가 하나의 길드 운영 공간이 됩니다.',
				'다른 서버의 내전 전적과 MMR은 서로 섞이지 않습니다.'
			]
		},
		{
			label: '초기 설정',
			title: 'Discord에서 `/초기설정`을 한 번 실행하세요.',
			description:
				'운영에 사용할 역할과 모집 채널을 지정하면 길드 운영 공간과 기본 시즌이 준비됩니다. 이미 설정된 서버에는 다시 실행할 수 없습니다.',
			icon: Settings2,
			command:
				'/초기설정 운영자역할 모집채널 [길드이름] [엔트리채널] [결과알림채널] [운영로그채널]',
			points: [
				'필수: 운영자역할, 모집채널',
				'선택: 길드이름, 엔트리채널, 결과알림채널, 운영로그채널'
			]
		},
		{
			label: '멤버 등록',
			title: '참가자는 `/등록`으로 Riot 계정을 연결합니다.',
			description:
				'현재 Discord 서버의 멤버로 등록되며, 입력한 Riot 계정은 내전 전적과 선호 챔피언 설정에 사용됩니다.',
			icon: UserRoundPlus,
			command: '/등록 GameName#TagLine',
			points: [
				'예시: `/등록 Longcat#KR1`',
				'등록 후 웹의 내 정보에서 선호 챔피언과 개인정보 설정을 관리합니다.'
			]
		}
	] as const;

	let selectedStep = $state(0);
	const activeStep = $derived(setupSteps[selectedStep]);
</script>

<svelte:head>
	<title>MonkeyBot 설치 안내</title>
	<meta
		name="description"
		content="Discord 서버에 MonkeyBot을 설치하고 내전 운영을 시작하는 방법"
	/>
</svelte:head>

<div class="min-h-[100dvh] bg-[var(--color-app-bg)] text-slate-100">
	<header class="border-b border-[var(--color-app-border-muted)] bg-[#0b1018]">
		<div
			class="mx-auto flex min-h-16 max-w-[1120px] items-center justify-between gap-4 px-4 lg:px-6"
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
				<a href={resolve('/landing')} class="hover:text-cyan-100">소개</a>
				<a href={resolve('/docs')} class="hover:text-cyan-100">문서</a>
			</nav>

			<a
				href={loginHref}
				class="inline-flex h-9 shrink-0 items-center gap-2 rounded-md border border-cyan-300/45 bg-cyan-300/10 px-3 text-xs font-semibold text-cyan-50 hover:border-cyan-200 hover:bg-cyan-300/18"
			>
				<LogIn size={15} aria-hidden="true" />
				<span>Discord 로그인</span>
			</a>
		</div>
	</header>

	<main>
		<section class="border-b border-[var(--color-app-border-muted)]">
			<div class="mx-auto max-w-[1120px] px-4 py-14 lg:px-6 lg:py-20">
				<p class="text-xs font-semibold text-cyan-200">GETTING STARTED</p>
				<h1 class="mt-4 max-w-2xl text-3xl font-semibold text-slate-50 sm:text-4xl">
					서버에 설치하고, 바로 내전을 시작하세요.
				</h1>
				<p class="mt-4 max-w-2xl text-sm leading-7 text-slate-400 sm:text-base">
					MonkeyBot은 Discord에서 참가자를 모으고 웹에서 내전을 운영합니다. 서버 운영자는 아래 세
					단계만 완료하면 됩니다.
				</p>
			</div>
		</section>

		<section class="mx-auto max-w-[1120px] px-4 py-10 lg:px-6 lg:py-14">
			<div class="grid gap-8 lg:grid-cols-[15rem_minmax(0,1fr)] lg:gap-12">
				<div class="lg:pt-1">
					<p class="text-xs font-semibold text-slate-500">설치 순서</p>
					<div class="mt-4 flex gap-2 overflow-x-auto pb-1 lg:flex-col lg:overflow-visible">
						{#each setupSteps as step, index (step.label)}
							<button
								type="button"
								class={`flex h-11 shrink-0 items-center gap-3 border px-3 text-left text-sm font-semibold transition-colors lg:w-full ${
									selectedStep === index
										? 'border-cyan-300/45 bg-cyan-300/10 text-cyan-50'
										: 'border-[var(--color-app-border-muted)] bg-[#0e1520] text-slate-400 hover:border-slate-600 hover:text-slate-200'
								}`}
								onclick={() => (selectedStep = index)}
								aria-pressed={selectedStep === index}
							>
								<span
									class={`grid size-5 shrink-0 place-items-center text-xs ${
										selectedStep === index
											? 'bg-cyan-300 text-slate-950'
											: 'border border-current/35 text-slate-500'
									}`}
								>
									{index + 1}
								</span>
								{step.label}
							</button>
						{/each}
					</div>
				</div>

				<section class="border border-[var(--color-app-border)] bg-[#0e1520]" aria-live="polite">
					<div
						class="flex items-start gap-4 border-b border-[var(--color-app-border-muted)] px-5 py-5 sm:px-6"
					>
						<div
							class="grid size-10 shrink-0 place-items-center border border-cyan-300/25 bg-cyan-300/10 text-cyan-200"
						>
							<activeStep.icon size={20} aria-hidden="true" />
						</div>
						<div class="min-w-0">
							<p class="text-xs font-semibold text-cyan-200">STEP {selectedStep + 1}</p>
							<h2 class="mt-1 text-lg font-semibold text-slate-50">{activeStep.title}</h2>
							<p class="mt-3 text-sm leading-6 text-slate-400">{activeStep.description}</p>
							{#if selectedStep === 0}
								<a
									href={installHref}
									class="mt-4 inline-flex h-10 items-center justify-center gap-2 rounded-md bg-cyan-300 px-4 text-sm font-semibold text-slate-950 hover:bg-cyan-200"
								>
									Discord 서버에 설치
									<ArrowRight size={16} aria-hidden="true" />
								</a>
							{/if}
						</div>
					</div>

					<div class="px-5 py-5 sm:px-6">
						{#if activeStep.command}
							<div class="border border-[var(--color-app-border-muted)] bg-[#0b1018] px-4 py-3">
								<p class="text-xs font-semibold text-slate-500">DISCORD COMMAND</p>
								<code
									class="mt-2 block overflow-x-auto whitespace-nowrap text-sm font-medium text-cyan-100"
								>
									{activeStep.command}
								</code>
							</div>
						{/if}

						<ul class:mt-5={activeStep.command} class="space-y-3">
							{#each activeStep.points as point (point)}
								<li class="flex gap-3 text-sm leading-6 text-slate-300">
									<Check size={16} class="mt-1 shrink-0 text-emerald-300" aria-hidden="true" />
									<span>{point}</span>
								</li>
							{/each}
						</ul>
					</div>

					<div
						class="flex items-center justify-between border-t border-[var(--color-app-border-muted)] px-5 py-4 sm:px-6"
					>
						{#if selectedStep > 0}
							<button
								type="button"
								class="text-xs font-semibold text-slate-400 hover:text-slate-100"
								onclick={() => (selectedStep -= 1)}
							>
								이전 단계
							</button>
						{:else}
							<span class="text-xs text-slate-600">1 / {setupSteps.length}</span>
						{/if}

						{#if selectedStep < setupSteps.length - 1}
							<button
								type="button"
								class="inline-flex items-center gap-1.5 text-xs font-semibold text-cyan-100 hover:text-cyan-50"
								onclick={() => (selectedStep += 1)}
							>
								다음 단계
								<ChevronRight size={15} aria-hidden="true" />
							</button>
						{:else}
							<span class="text-xs text-slate-600">{setupSteps.length} / {setupSteps.length}</span>
						{/if}
					</div>
				</section>
			</div>
		</section>

		<section class="border-t border-[var(--color-app-border-muted)] bg-[#0e1520]">
			<div
				class="mx-auto flex max-w-[1120px] flex-col gap-6 px-4 py-10 sm:flex-row sm:items-center sm:justify-between lg:px-6"
			>
				<div class="flex min-w-0 items-start gap-3">
					<ShieldCheck size={20} class="mt-0.5 shrink-0 text-emerald-200" aria-hidden="true" />
					<div>
						<h2 class="text-base font-semibold text-slate-50">
							초기 설정 후에는 웹에서 운영합니다.
						</h2>
						<p class="mt-1 text-sm leading-6 text-slate-400">
							Discord 계정으로 로그인하면 길드별 내전, 경매, 전적과 설정을 확인할 수 있습니다.
						</p>
					</div>
				</div>
				<a
					href={loginHref}
					class="inline-flex h-10 shrink-0 items-center justify-center gap-2 rounded-md bg-cyan-300 px-4 text-sm font-semibold text-slate-950 hover:bg-cyan-200"
				>
					Discord 로그인
					<ArrowRight size={16} aria-hidden="true" />
				</a>
			</div>
		</section>
	</main>

	<footer class="border-t border-[var(--color-app-border-muted)] bg-[#0b1018]">
		<div
			class="mx-auto flex min-h-16 max-w-[1120px] flex-wrap items-center justify-between gap-3 px-4 py-4 text-xs lg:px-6"
		>
			<span class="font-semibold text-slate-300">MonkeyBot</span>
			<nav class="flex items-center gap-4 text-slate-500" aria-label="하단 메뉴">
				<a href={resolve('/landing')} class="hover:text-cyan-100">소개</a>
				<a href={resolve('/docs')} class="hover:text-cyan-100">문서</a>
				<a href={resolve('/terms')} class="hover:text-cyan-100">이용약관</a>
				<a href={resolve('/privacy')} class="hover:text-cyan-100">개인정보 처리방침</a>
			</nav>
		</div>
	</footer>
</div>
