import { describe, expect, it } from 'vitest';
import { commands } from './index.js';

describe('Discord command definitions', () => {
	it('places required options before optional options', () => {
		for (const command of commands) {
			const options = command.data.toJSON().options ?? [];
			const firstOptional = options.findIndex((option) => option.required !== true);

			if (firstOptional === -1) continue;

			expect(options.slice(firstOptional).every((option) => option.required !== true)).toBe(true);
		}
	});
});
