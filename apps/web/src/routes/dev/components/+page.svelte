<script lang="ts">
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
</script>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="개발"
		title="컴포넌트 개발"
		description="현재 단독 개발 중인 컴포넌트가 없습니다."
		metrics={[
			{ label: '상태', value: '비어 있음', tone: 'neutral' },
			{ label: '대상', value: '없음', tone: 'neutral' }
		]}
	/>
</div>
