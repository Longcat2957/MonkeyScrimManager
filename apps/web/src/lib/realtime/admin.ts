import type { GuildAdminSettings, GuildMemberStatus } from '$lib/fixtures/guildAdmin';
import type { Lane } from '$lib/fixtures/workflow';

export interface GuildMmrAdjustmentCommand {
	kind: 'ADJUST_MMR';
	memberId: string;
	lane: Lane;
	delta: number;
	note: string;
	expectedRevision: number;
}

export interface GuildMemberStatusCommand {
	kind: 'SET_MEMBER_STATUS';
	memberId: string;
	status: GuildMemberStatus;
	expectedRevision: number;
}

export interface GuildRevertCommand {
	kind: 'APPLY_REVERT';
	checkpointId: string;
	reason: string;
	expectedRevision: number;
}

export interface GuildSettingsUpdateCommand {
	kind: 'UPDATE_GUILD_SETTINGS';
	settings: GuildAdminSettings;
	expectedRevision: number;
}
