<script lang="ts">
	import StaticMarkdownDocument from '$lib/components/layout/StaticMarkdownDocument.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const sections = [
		{ id: 'scope', label: '목적과 적용' },
		{ id: 'service', label: '서비스 내용' },
		{ id: 'account', label: '계정과 권한' },
		{ id: 'responsibilities', label: '이용자 책임' },
		{ id: 'guild-data', label: '길드 운영 데이터' },
		{ id: 'external-services', label: '외부 서비스' },
		{ id: 'availability', label: '변경과 중단' },
		{ id: 'rights', label: '권리와 표시' },
		{ id: 'liability', label: '책임 범위' },
		{ id: 'termination', label: '이용 종료' },
		{ id: 'changes', label: '약관 변경' },
		{ id: 'contact', label: '문의와 준거' }
	];
</script>

<svelte:head>
	<title>이용약관 | MonkeyBot</title>
	<meta name="description" content="MonkeyBot Discord 봇 및 길드 내전 관리 서비스 이용약관" />
</svelte:head>

<StaticMarkdownDocument
	kicker="Legal"
	title="이용약관"
	description="MonkeyBot Discord 봇과 웹 콘솔 이용에 적용되는 기본 조건"
	content={data.content}
	{sections}
	navigationLabel="이용약관 목차"
	dateLabel="시행일"
	dateValue="2026년 7월 13일"
/>
