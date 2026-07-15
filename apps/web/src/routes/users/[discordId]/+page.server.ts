import { error } from '@sveltejs/kit';
import { fetchUserProfile, userProfileErrorText, UserProfileApiError } from '$lib/api/userProfile';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = async ({ fetch, params }) => {
	try {
		return { profile: await fetchUserProfile(fetch, params.discordId) };
	} catch (cause) {
		if (!(cause instanceof UserProfileApiError)) throw cause;
		const status = cause.status >= 400 && cause.status <= 599 ? cause.status : 503;
		error(status, userProfileErrorText(cause));
	}
};
