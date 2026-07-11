<script lang="ts">
	import RecruitmentEntryQueue from '$lib/components/workflow/RecruitmentEntryQueue.svelte';
	import WorkspaceHeader from '$lib/components/layout/WorkspaceHeader.svelte';
	import { mockRecruitmentQueue } from '$lib/fixtures/recruitments';
</script>

<div class="space-y-5">
	<WorkspaceHeader
		kicker="일반 내전"
		title="모집 목록"
		description="Discord 모집이 마감된 뒤 웹에서 편성할 대상을 확인합니다."
	/>

	<RecruitmentEntryQueue items={mockRecruitmentQueue} />
</div>
