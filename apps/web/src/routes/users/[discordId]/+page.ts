import type { PageLoad } from './$types';

export const load: PageLoad = ({ params }) => {
	return {
		discordId: params.discordId,
		displayName: params.discordId === 'user-tamsol' ? '탐솔' : params.discordId
	};
};
