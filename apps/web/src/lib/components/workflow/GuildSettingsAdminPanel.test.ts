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
		resultChannelId: null,
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
	it('requires a recruitment channel while keeping feedback channels nullable', () => {
		const { body } = render(GuildSettingsAdminPanel, {
			props: {
				snapshot,
				onSave: vi.fn()
			}
		});

		expect(body).toContain('MonkeyBotTest');
		expect(body).toContain('내전 운영진');
		expect(body).toContain('내전 / #내전-엔트리');
		expect(body).toContain('모집 채널');
		expect(body).toContain('내전과 경매 모집 메시지를 게시할 필수 채널');
		expect(body).toContain('모집 채널을 선택해 주세요');
		expect(body).toContain('모집 채널을 선택해야 길드 설정을 저장할 수 있습니다.');
		expect(body).toContain('엔트리 채널');
		expect(body).toContain('결과 알림 채널');
		expect(body).toContain('설정하지 않으면 Discord 알림을 보내지 않음');
		expect(body).toContain('운영 로그 채널');
		expect(body.match(/설정 안 함/g)).toHaveLength(3);
		expect(body.indexOf('길드 표시 이름')).toBeLessThan(body.indexOf('운영자 역할'));
		expect(body.indexOf('운영자 역할')).toBeLessThan(body.indexOf('모집 채널'));
		expect(body.indexOf('모집 채널')).toBeLessThan(body.indexOf('엔트리 채널'));
		expect(body.indexOf('엔트리 채널')).toBeLessThan(body.indexOf('결과 알림 채널'));
		expect(body.indexOf('결과 알림 채널')).toBeLessThan(body.indexOf('운영 로그 채널'));
		expect(body).not.toContain('현재 시즌');
		expect(body).not.toContain('전적과 길드 MMR이 집계되는 시즌');
	});

	it('preserves configured Discord ids that are absent from the latest catalog', () => {
		const missingCatalog: GuildAdminSettingsSnapshot = {
			...snapshot,
			settings: {
				...snapshot.settings,
				operatorRoleId: '1525889769761149999',
				entryChannelId: '1525900000000000999',
				resultChannelId: '1525900000000000998'
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
		expect(body).toContain('1525900000000000998');
	});
});
