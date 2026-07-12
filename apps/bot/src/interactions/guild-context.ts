export const SERVER_ONLY_MESSAGE = 'Discord 서버에서만 사용할 수 있습니다.';
export const BOT_NOT_INSTALLED_MESSAGE =
	'MonkeyBot 계정이 이 서버에 설치되어 있지 않습니다. Guild Install에 bot scope를 추가한 뒤 다시 설치해 주세요.';

interface GuildInteractionShape {
	guildId: string | null;
	guild: unknown | null;
	member: unknown | null;
}

export function guildInteractionError(interaction: GuildInteractionShape): string | null {
	if (!interaction.guildId) return SERVER_ONLY_MESSAGE;
	if (!interaction.guild || !interaction.member) return BOT_NOT_INSTALLED_MESSAGE;
	return null;
}
