import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import Page from './+page.svelte';

const source = readFileSync(new URL('./+page.svelte', import.meta.url), 'utf8');
const guildId = '1525851181157580861';

function renderPage() {
	return render(Page, {
		props: {
			data: {
				guildId,
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
		expect(source).toContain('createRecruitment(data.guildId, idempotencyKey)');
		expect(source).toContain("resolve('/g/[guildId]/recruitments/[id]'");
		expect(source).toContain('id: created.recruitment.id');
		expect(source).not.toContain('await goto(created.links.entry)');
		expect(source).not.toContain("['guilds', data.guildId, 'recruitments', 'manual']");
	});
});
