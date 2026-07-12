<script lang="ts">
	import StaticMarkdownDocument from '$lib/components/layout/StaticMarkdownDocument.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const sections = [
		{ id: 'overview', label: '처리 원칙' },
		{ id: 'data', label: '처리 항목' },
		{ id: 'purpose', label: '처리 목적' },
		{ id: 'retention', label: '보유 기간' },
		{ id: 'visibility', label: '길드 내 공개' },
		{ id: 'providers', label: '외부 서비스' },
		{ id: 'cookies', label: '쿠키와 세션' },
		{ id: 'rights', label: '이용자 권리' },
		{ id: 'destruction', label: '파기 절차' },
		{ id: 'security', label: '보호 조치' },
		{ id: 'contact', label: '문의' },
		{ id: 'changes', label: '방침 변경' }
	];
</script>

<svelte:head>
	<title>개인정보 처리방침 | MonkeyBot</title>
	<meta
		name="description"
		content="MonkeyBot Discord 봇 및 길드 내전 관리 서비스 개인정보 처리방침"
	/>
</svelte:head>

<StaticMarkdownDocument
	kicker="Privacy"
	title="개인정보 처리방침"
	description="MonkeyBot이 처리하는 정보와 이용자가 선택할 수 있는 공개 범위"
	content={data.content}
	{sections}
	navigationLabel="개인정보 처리방침 목차"
	dateLabel="시행일"
	dateValue="2026년 7월 13일"
/>
