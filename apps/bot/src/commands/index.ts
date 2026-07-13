import { recruitCommand } from './recruit.js';
import { registerCommand } from './register.js';
import { setupCommand } from './setup.js';

export const commands = [setupCommand, registerCommand, recruitCommand] as const;

export function commandByName(name: string) {
	return commands.find((command) => command.data.name === name);
}
