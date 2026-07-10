<script lang="ts">
	interface Props {
		tone?: 'neutral' | 'info' | 'success' | 'warning' | 'danger';
		label: string;
	}

	let { tone = 'neutral', label }: Props = $props();

	const toneClass = {
		neutral: 'border-slate-700 bg-slate-900/80 text-slate-200',
		info: 'border-cyan-400/35 bg-cyan-400/10 text-cyan-100',
		success: 'border-emerald-400/35 bg-emerald-400/10 text-emerald-100',
		warning: 'border-amber-300/35 bg-amber-300/10 text-amber-100',
		danger: 'border-red-400/35 bg-red-500/10 text-red-100'
	};
</script>

<span
	class={`inline-flex h-7 items-center border px-2 text-[11px] font-semibold ${toneClass[tone]}`}
>
	{label}
</span>
