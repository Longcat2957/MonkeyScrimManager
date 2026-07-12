import { recruitCommand } from './recruit.js';
import { setupCommand } from './setup.js';

export const commands = [setupCommand, recruitCommand] as const;

export function commandByName(name: string) {
	return commands.find((command) => command.data.name === name);
}
