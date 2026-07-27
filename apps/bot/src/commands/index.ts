import { auctionRecruitTenCommand, auctionRecruitTwentyCommand } from './auction-recruit.js';
import {
	auctionRecruitmentAddMemberCommand,
	auctionRecruitmentRemoveMemberCommand
} from './auction-recruitment-members.js';
import { auctionRecruitmentForceCancelCommand } from './auction-recruitment-force-cancel.js';
import { auctionTournamentCancelCommand } from './auction-tournament-cancel.js';
import { auctionTournamentListCommand } from './auction-tournament-list.js';
import { recruitCommand } from './recruit.js';
import { registerCommand } from './register.js';
import { setupCommand } from './setup.js';

export const commands = [
	setupCommand,
	registerCommand,
	recruitCommand,
	auctionRecruitTenCommand,
	auctionRecruitTwentyCommand,
	auctionRecruitmentAddMemberCommand,
	auctionRecruitmentRemoveMemberCommand,
	auctionTournamentListCommand,
	auctionRecruitmentForceCancelCommand,
	auctionTournamentCancelCommand
] as const;

export function commandByName(name: string) {
	return commands.find((command) => command.data.name === name);
}
