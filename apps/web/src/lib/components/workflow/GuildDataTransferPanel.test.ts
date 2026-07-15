import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import GuildDataTransferPanel from './GuildDataTransferPanel.svelte';

const source = readFileSync(new URL('./GuildDataTransferPanel.svelte', import.meta.url), 'utf8');

describe('GuildDataTransferPanel', () => {
	it('renders only the member-season MMR transfer workflow', () => {
		const { body } = render(GuildDataTransferPanel, {
			props: {
				guildId: '1525851181157580861',
				guildName: 'MonkeyBot Test',
				activeMemberCount: 12,
				currentSeasonName: '2026 시즌 1'
			}
		});

		expect(body).toContain('데이터 내보내기');
		expect(body).toContain('현재 활성 멤버의 시즌 레인 MMR 백업');
		expect(body).toContain('암호화하지 않은 읽고 편집할 수 있는 JSON 파일입니다.');
		expect(body).toContain('데이터 가져오기');
		expect(body).toContain('MonkeyBot 멤버·시즌 MMR 백업 JSON만 지원');
		expect(body).toContain('12명');
		expect(body).toContain('2026 시즌 1');
		expect(body).not.toContain('완료 내전');
		expect(body).not.toContain('경매 기록');
	});

	it('requires server preview and explicit guild-name confirmation before apply', () => {
		expect(source).toContain('previewGuildMemberSeasonMmrSeedImport(guildId, selectedArchive)');
		expect(source).toContain('applyGuildMemberSeasonMmrSeedImport(guildId');
		expect(source).toContain('confirmationValue.trim() === guildName');
		expect(source).toContain('onInteractOutside={preventCloseWhileApplying}');
		expect(source).toContain('onEscapeKeydown={preventCloseWhileApplying}');
		expect(source).toContain('memberSeasonMmrSeedArchiveSchema.safeParse(payload)');
		expect(source).toContain('serializeGuildMemberSeasonMmrSeedArchive(archive)');
		expect(source).toContain('globalThis.crypto.randomUUID()');
		expect(source).toContain('const isPreviewExpired = $derived(');
		expect(source).toContain('!isPreviewExpired');
		expect(source).not.toContain('expiresAt - previewClock');
	});
});
