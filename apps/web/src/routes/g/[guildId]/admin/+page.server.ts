import { error } from '@sveltejs/kit';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ params, parent }) => {
	const { guild } = await parent();

	if (guild.currentUserRole !== 'OPERATOR') {
		error(403, '길드 관리자 권한이 필요합니다.');
	}

	return { guildId: params.guildId };
};
