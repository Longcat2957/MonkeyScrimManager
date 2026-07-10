<script lang="ts">
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
</script>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="경매 내전"
		title="컴포넌트 검토 대기"
		description="다음 경매내전 컴포넌트 범위를 정리한 뒤 이 페이지에 하나씩 배치합니다."
		metrics={[
			{ label: '상태', value: '대기', tone: 'warning' },
			{ label: '범위', value: '재정리', tone: 'info' }
		]}
	/>
</div>
