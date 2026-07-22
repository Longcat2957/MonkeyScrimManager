import { readFileSync, statSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const rankNames = [
	'bronze',
	'challenger',
	'diamond',
	'emerald',
	'gold',
	'grandmaster',
	'iron',
	'master',
	'platinum',
	'silver'
];

function staticAsset(relativePath: string): URL {
	return new URL(`../../static/${relativePath}`, import.meta.url);
}

function pngDimensions(path: URL): { width: number; height: number } {
	const header = readFileSync(path).subarray(0, 24);
	return {
		width: header.readUInt32BE(16),
		height: header.readUInt32BE(20)
	};
}

describe('optimized static UI assets', () => {
	it('keeps rank artwork at the 3x display-size budget', () => {
		let totalBytes = 0;
		for (const rank of rankNames) {
			const path = staticAsset(`riot/ranks/${rank}.png`);
			expect(pngDimensions(path)).toEqual({ width: 256, height: 256 });
			totalBytes += statSync(path).size;
		}

		expect(totalBytes).toBeLessThan(1_000_000);
	});

	it('keeps the shared logo and favicon within their delivery budgets', () => {
		const logo = staticAsset('brand/logo.png');
		const favicon = staticAsset('brand/favicon.png');

		expect(pngDimensions(logo)).toEqual({ width: 512, height: 512 });
		expect(statSync(logo).size).toBeLessThan(70_000);
		expect(pngDimensions(favicon)).toEqual({ width: 128, height: 128 });
		expect(statSync(favicon).size).toBeLessThan(25_000);
	});
});
