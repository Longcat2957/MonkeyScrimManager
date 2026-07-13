import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';
import type { GuildAdminMember } from '$lib/contracts/workflow';
import GuildMemberManagementPanel from './GuildMemberManagementPanel.svelte';

const member: GuildAdminMember = {
	id: '1525847677542207548',
	identityKind: 'DISCORD',
	discordName: 'Longcat',
	riotId: null,
	role: 'OPERATOR',
	mainLane: null,
	laneMmr: { TOP: 1500, JUNGLE: 1618, MID: 1500, BOTTOM: 1500, SUPPORT: 1500 },
	registered: true,
	status: 'ACTIVE',
	lastActiveAt: null
};

const otherMember: GuildAdminMember = {
	...member,
	id: '1525847677542207549',
	discordName: 'Other member',
	role: 'MEMBER'
};

const dummyMember: GuildAdminMember = {
	...otherMember,
	id: 'dmy_01900000-0000-7000-8000-000000000001',
	identityKind: 'DUMMY',
	discordName: 'Dummy top'
};

describe('GuildMemberManagementPanel', () => {
	it('renders canonical roles and nullable member fields without fabricating a lane', () => {
		const { body } = render(GuildMemberManagementPanel, {
			props: {
				members: [member, otherMember, dummyMember],
				revision: 0,
				currentUserId: member.id,
				onAdjustMmr: vi.fn(),
				onSetStatus: vi.fn()
			}
		});

		expect(body).toContain('운영자');
		expect(body).toContain('Riot 계정 미연결');
		expect(body).toContain('미설정');
		expect(body).not.toContain('undefined');
		expect(body).toContain('자기 자신은 비활성화할 수 없습니다');
		expect(body).toContain('멤버 비활성화');
		expect(body).toContain('더미');
		expect(body.match(/disabled/g)?.length).toBeGreaterThanOrEqual(2);
	});
});
