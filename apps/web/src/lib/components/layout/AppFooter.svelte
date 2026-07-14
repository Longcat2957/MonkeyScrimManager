<script lang="ts">
	import { resolve } from '$app/paths';

	interface Props {
		reserveFabSpace?: boolean;
	}

	let { reserveFabSpace = true }: Props = $props();
</script>

<footer class="shrink-0 border-t border-[var(--color-app-border)] bg-[var(--color-app-header)]">
	<div
		class={`mx-auto flex min-h-14 max-w-[1680px] flex-wrap items-center justify-between gap-3 py-3 pl-4 lg:pl-6 ${
			reserveFabSpace ? 'pr-24' : 'pr-4 lg:pr-6'
		}`}
	>
		<a href={resolve('/')} class="grid min-w-0" aria-label="MonkeyBot 홈">
			<strong class="truncate text-xs font-semibold text-slate-300">MonkeyBot</strong>
			<small class="text-[10px] font-medium text-slate-600">monkeybot.lol</small>
		</a>
		<nav
			class="flex flex-wrap items-center justify-end gap-x-3 gap-y-2 text-[11px] font-medium"
			aria-label="Footer 메뉴"
		>
			<a href={resolve('/docs')} class="text-slate-500 hover:text-cyan-100">문서</a>
			<a href={resolve('/terms')} class="text-slate-500 hover:text-cyan-100">이용약관</a>
			<a href={resolve('/privacy')} class="text-slate-500 hover:text-cyan-100">개인정보 처리방침</a>
		</nav>
	</div>
</footer>
