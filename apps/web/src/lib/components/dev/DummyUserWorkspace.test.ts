import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import { createChampionCatalog } from '$lib/datadragon';
import { dummyGroupFixture, dummyUserFixture } from '$lib/contracts/dummyUsers.testFixtures';
import DummyUserWorkspace from './DummyUserWorkspace.svelte';

const source = readFileSync(new URL('./DummyUserWorkspace.svelte', import.meta.url), 'utf8');

const championCatalog = createChampionCatalog({
	type: 'champion',
	format: 'standAloneComplex',
	version: '16.13.1',
	data: {
		Ahri: {
			version: '16.13.1',
			id: 'Ahri',
			key: '103',
			name: '아리',
			image: { full: 'Ahri.png' },
			tags: ['Mage']
		}
	}
});

describe('DummyUserWorkspace', () => {
	it('keeps the reusable dummy identity management actions and detail areas visible', () => {
		const { body } = render(DummyUserWorkspace, {
			props: {
				initialUsers: [dummyUserFixture],
				initialTotal: 1,
				initialGroups: [dummyGroupFixture],
				guilds: [
					{
						guildId: dummyUserFixture.guildMemberships[0]!.guildId,
						displayName: dummyUserFixture.guildMemberships[0]!.guildName,
						status: 'ACTIVE',
						activeSeasonId: '01900000-0000-7000-8000-000000000020'
					}
				],
				championCatalog
			}
		});

		for (const expected of [
			'사용자 생성',
			'그룹 관리',
			'길드 배치',
			'가상 Riot 계정 편집',
			'선호 챔피언 편집',
			'사용자 보관',
			'사용자 삭제 검토'
		]) {
			expect(body).toContain(expected);
		}
	});

	it('passes operation-scoped idempotency keys to UI mutations', () => {
		expect(source).toContain('createOperationIdempotencyKeys()');
		expect(source.match(/mutationKeys\.keyFor\(/g)?.length).toBeGreaterThanOrEqual(12);
		expect(source.match(/mutationKeys\.complete\(/g)?.length).toBeGreaterThanOrEqual(12);
	});

	it('refreshes authoritative groups when member replacement fails after metadata update', () => {
		expect(source).toContain('await recoverGroupAfterMemberFailure(');
		expect(source).toContain('const refreshedGroups = (await fetchDummyGroups()).groups;');
		expect(source).toContain('selectGroup(refreshed);');
		expect(source).toContain('서버 최신 상태로 복구했습니다.');
		expect(source).toContain('구성원 저장: ${memberFailureText} / 상태 갱신:');
	});
});
