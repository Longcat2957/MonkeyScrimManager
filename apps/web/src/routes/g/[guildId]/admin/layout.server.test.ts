import { describe, expect, it, vi } from 'vitest';
import { load } from './+layout.server';

function run(currentUserRole: 'MEMBER' | 'OPERATOR') {
	return load({
		parent: vi.fn().mockResolvedValue({ guild: { currentUserRole } })
	} as never);
}

describe('guild admin layout server load', () => {
	it('allows operators to access every nested admin route', async () => {
		await expect(run('OPERATOR')).resolves.toEqual({});
	});

	it('rejects non-operators before rendering an admin route', async () => {
		await expect(run('MEMBER')).rejects.toMatchObject({
			status: 403,
			body: { message: '길드 관리자 권한이 필요합니다.' }
		});
	});
});
