import { error } from '@sveltejs/kit';
import type { LayoutServerLoad } from './$types';

export const load: LayoutServerLoad = async ({ parent }) => {
	const { guild } = await parent();

	if (guild.currentUserRole !== 'OPERATOR') {
		error(403, '길드 관리자 권한이 필요합니다.');
	}

	return {};
};
