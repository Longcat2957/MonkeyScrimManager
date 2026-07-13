import { DATA_DRAGON_PROXY_ROOT } from './champions';

export const DEFAULT_PROFILE_ICON_VERSION = '15.14.1';

export function riotProfileIconUrl(
	profileIconId: number | string,
	version = DEFAULT_PROFILE_ICON_VERSION,
	proxyRoot = DATA_DRAGON_PROXY_ROOT
): string {
	return `${proxyRoot}/cdn/${encodeURIComponent(version)}/img/profileicon/${encodeURIComponent(String(profileIconId))}.png`;
}
