import { render } from 'svelte/server';
import { describe, expect, it, vi } from 'vitest';
import type { GuildAdminSettingsSnapshot } from '$lib/contracts/workflow';
import GuildSettingsAdminPanel from './GuildSettingsAdminPanel.svelte';

const snapshot: GuildAdminSettingsSnapshot = {
	settings: {
		displayName: 'MonkeyBotTest',
		operatorRoleId: '1525889769761149148',
		recruitmentChannelId: null,
		entryChannelId: '1525900000000000002',
		logChannelId: '1525900000000000003',
		isExternallyPublic: false
	},
	revision: 3,
	updatedAt: '2026-07-13T08:00:00Z',
	updatedBy: '1525847677542207548',
	catalogUpdatedAt: '2026-07-13T07:59:00Z',
	roles: [{ id: '1525889769761149148', name: '내전 운영진' }],
	channels: [
		{ id: '1525900000000000001', name: '내전-모집', parentName: '내전' },
		{ id: '1525900000000000002', name: '내전-엔트리', parentName: '내전' },
		{ id: '1525900000000000003', name: '운영-로그', parentName: null }
	]
};

describe('GuildSettingsAdminPanel', () => {
	it('renders authoritative Discord options and nullable channel choices without season settings', () => {
		const { body } = render(GuildSettingsAdminPanel, {
			props: {
				snapshot,
				onSave: vi.fn()
			}
		});

		expect(body).toContain('MonkeyBotTest');
		expect(body).toContain('내전 운영진');
		expect(body).toContain('내전 / #내전-엔트리');
		expect(body).toContain('설정 안 함');
		expect(body).toContain('모집 채널');
		expect(body).toContain('엔트리 채널');
		expect(body).toContain('운영 로그 채널');
		expect(body.indexOf('길드 표시 이름')).toBeLessThan(body.indexOf('운영자 역할'));
		expect(body.indexOf('운영자 역할')).toBeLessThan(body.indexOf('모집 채널'));
		expect(body.indexOf('모집 채널')).toBeLessThan(body.indexOf('엔트리 채널'));
		expect(body.indexOf('엔트리 채널')).toBeLessThan(body.indexOf('운영 로그 채널'));
		expect(body).not.toContain('현재 시즌');
		expect(body).not.toContain('전적과 길드 MMR이 집계되는 시즌');
	});

	it('preserves configured Discord ids that are absent from the latest catalog', () => {
		const missingCatalog: GuildAdminSettingsSnapshot = {
			...snapshot,
			settings: {
				...snapshot.settings,
				operatorRoleId: '1525889769761149999',
				entryChannelId: '1525900000000000999'
			}
		};
		const { body } = render(GuildSettingsAdminPanel, {
			props: {
				snapshot: missingCatalog,
				onSave: vi.fn()
			}
		});

		expect(body).toContain('Discord에서 확인 불가');
		expect(body).toContain('1525889769761149999');
		expect(body).toContain('1525900000000000999');
	});
});
