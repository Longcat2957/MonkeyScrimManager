import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';
import { scrimPickBanResultInputChampions } from '$lib/fixtures/scrimPickBanResultInputFixtures';
import ScrimPickBanResultInputChampionHints from './ScrimPickBanResultInputChampionHints.svelte';

const source = readFileSync(
	new URL('./ScrimPickBanResultInputChampionHints.svelte', import.meta.url),
	'utf8'
);
const champions = scrimPickBanResultInputChampions.slice(0, 10);

describe('ScrimPickBanResultInputChampionHints', () => {
	it('keeps all ten allowed preferences in a single horizontal rail', () => {
		const { body } = render(ScrimPickBanResultInputChampionHints, {
			props: {
				label: '선호',
				championIds: champions.map((champion) => champion.id),
				champions,
				version: '16.13.1',
				onPick: vi.fn()
			}
		});

		expect(body).toContain('선호 챔피언 목록');
		for (const champion of champions) {
			expect(body).toContain(champion.name);
		}
		expect(source).toContain('flex h-9 min-w-0 items-start gap-1 overflow-x-auto');
		expect(source).toContain('pb-2 pr-1');
		expect(source).toContain('shrink-0');
		expect(source).not.toContain('grid-cols-3');
	});
});
