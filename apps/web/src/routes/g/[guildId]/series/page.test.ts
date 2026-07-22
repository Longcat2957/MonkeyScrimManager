import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import Page from './+page.svelte';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');
const guildId = '1525851181157580861';

function renderPage(openCreateDialog = false) {
	return render(Page, {
		props: {
			data: {
				guildId,
				openCreateDialog,
				guild: { currentUserRole: 'MEMBER' },
				view: { activeWork: [], completedSeries: [] }
			}
		} as never
	});
}

describe('general series page', () => {
	it('offers every active guild member a direct recruitment creation action', () => {
		const { body } = renderPage();

		expect(body).toContain('내전 모집');
		expect(body).toContain('진행 중인 내전이 없습니다.');
		expect(source).toContain('createRecruitment(');
		expect(source).toContain('let createDialogOpen = $derived(Boolean(data.openCreateDialog));');
		expect(source).toContain('{ bestOf: createBestOf, hardFearless: createHardFearless }');
		expect(source).toContain('<Dialog.Root bind:open={createDialogOpen}>');
		expect(source).toContain('BO{bestOf}');
		expect(source).toContain("resolve('/g/[guildId]/recruitments/[id]'");
		expect(source).toContain('id: created.recruitment.id');
		expect(source).not.toContain('await goto(created.links.entry)');
		expect(source).not.toContain("['guilds', data.guildId, 'recruitments', 'manual']");
	});

	it('uses the presentation-provided next action instead of a generic work link', () => {
		expect(source).toContain('{item.actionLabel}');
		expect(source).not.toContain('작업 열기');
	});
});
