import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const appCss = readFileSync(new URL('../../../app.css', import.meta.url), 'utf8');
const coinSource = readFileSync(new URL('./CoinTossGame.svelte', import.meta.url), 'utf8');
const ladderSource = readFileSync(new URL('./LadderGame.svelte', import.meta.url), 'utf8');
const rouletteSource = readFileSync(new URL('./RouletteGame.svelte', import.meta.url), 'utf8');

describe('FAB minigame animation production fallback', () => {
	it('keeps animation-critical rules in the eager app stylesheet', () => {
		expect(appCss).toContain('.fab-minigame-coin-flip');
		expect(appCss).toContain('.fab-minigame-roulette-wheel');
		expect(appCss).toContain('.fab-minigame-ladder-bridge');
		expect(appCss).toContain('.fab-minigame-ladder-trace');
		expect(appCss).toContain('@keyframes fab-minigame-ladder-bridge-flash');
	});

	it('wires every minigame to the eager animation classes', () => {
		expect(coinSource).toContain('fab-minigame-coin-flip');
		expect(rouletteSource).toContain('fab-minigame-roulette-wheel');
		expect(ladderSource).toContain('fab-minigame-ladder-bridge');
		expect(ladderSource).toContain('fab-minigame-ladder-trace');
		expect(ladderSource).toContain('fab-minigame-ladder-dot');
		expect(ladderSource).toContain('fab-minigame-ladder-output-pin');
	});

	it('does not publish coin results until the toss animation phase completes', () => {
		expect(coinSource).toContain('let pendingResult = $state<CoinSide | null>(null)');
		expect(coinSource).toContain('const nextResult = randomSide();');
		expect(coinSource).toContain('result = null;');
		expect(coinSource).toContain('pendingResult = nextResult;');
		expect(coinSource).toContain('result = pendingResult;');
		expect(coinSource).not.toContain('result = randomSide();');
	});

	it('starts ladder trace animations after the hidden starting state can paint', () => {
		expect(ladderSource).toContain('function scheduleAfterPaint');
		expect(ladderSource).toContain('requestNextFrame(() => requestNextFrame(callback))');
		expect(ladderSource).toContain('activeEntryIds = [...activeEntryIds, trace.entryId];');
	});
});
