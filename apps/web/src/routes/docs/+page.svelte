<script lang="ts">
	import StaticMarkdownDocument from '$lib/components/layout/StaticMarkdownDocument.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const sections = [
		{ id: 'overview', label: '기본 구조' },
		{ id: 'getting-started', label: '시작하기' },
		{ id: 'series', label: '일반 내전' },
		{ id: 'auction', label: '경매 내전' },
		{ id: 'records', label: '전적 검색' },
		{ id: 'leaderboard', label: '랭킹' },
		{ id: 'profile', label: '내 정보' },
		{ id: 'admin', label: '길드 관리' },
		{ id: 'realtime', label: '실시간 편집' },
		{ id: 'tools', label: '빠른 도구' }
	];
</script>

<svelte:head>
	<title>사용 문서 | MonkeyBot</title>
	<meta name="description" content="MonkeyBot 길드 내전 운영 기능 사용 문서" />
</svelte:head>

<StaticMarkdownDocument
	kicker="Docs"
	title="MonkeyBot 사용 문서"
	description="길드 내전 운영 기능과 화면별 흐름"
	content={data.content}
	{sections}
	navigationLabel="문서 목차"
/>
