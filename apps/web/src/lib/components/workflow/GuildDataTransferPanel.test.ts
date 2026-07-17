import { readFileSync } from 'node:fs';
import { render } from 'svelte/server';
import { describe, expect, it } from 'vitest';
import GuildDataTransferPanel from './GuildDataTransferPanel.svelte';

const source = readFileSync(new URL('./GuildDataTransferPanel.svelte', import.meta.url), 'utf8');

describe('GuildDataTransferPanel', () => {
	it('renders the complete guild history transfer workflow', () => {
		const { body } = render(GuildDataTransferPanel, {
			props: {
				guildId: '1525851181157580861',
				guildName: 'MonkeyBot Test',
				activeMemberCount: 12,
				currentSeasonName: '2026 시즌 1'
			}
		});
		const normalizedBody = body.replace(/\s+/g, ' ');

		expect(normalizedBody).toContain('전체 데이터 내보내기');
		expect(normalizedBody).toContain('멤버·시즌·전적·픽/밴·경매·MMR 원장·감사 기록');
		expect(normalizedBody).toContain('전체 데이터 가져오기');
		expect(normalizedBody).toContain('길드 ID가 달라도 적용 가능한 GUILD_FULL_SNAPSHOT v1 JSON');
		expect(normalizedBody).toContain(
			'미리보기는 PostgreSQL 제약을 실제로 검사한 뒤 모두 롤백합니다.'
		);
		expect(normalizedBody).toContain('12명');
		expect(normalizedBody).toContain('2026 시즌 1');
		expect(normalizedBody).not.toContain('경기·전적·승패 수는 옮기지 않습니다');
	});

	it('requires server preview and explicit guild-name confirmation before apply', () => {
		expect(source).toContain('previewGuildFullSnapshotImport(guildId, selectedArchive)');
		expect(source).toContain('applyGuildFullSnapshotImport(guildId');
		expect(source).toContain('confirmationValue.trim() === guildName');
		expect(source).toContain('onInteractOutside={preventCloseWhileApplying}');
		expect(source).toContain('onEscapeKeydown={preventCloseWhileApplying}');
		expect(source).toContain('guildFullSnapshotArchiveSchema.safeParse(payload)');
		expect(source).toContain('serializeGuildFullSnapshotArchive(archive)');
		expect(source).toContain('expectedTargetDigest: preview.targetDigest');
		expect(source).toContain('preview.usesTargetActiveSeason');
		expect(source).toContain('preview.remapsSourceGuild');
		expect(source).toContain('preview.rebasesDisplayNumbers');
		expect(source).toContain('원본 길드의');
		expect(source).toContain('채널/메시지 연결은 제거합니다');
		expect(source).toContain('result.remappedSourceGuild');
		expect(source).toContain('result.rebasedDisplayNumbers');
		expect(source).toContain('백업의 활성 시즌 데이터는 현재 활성 시즌인');
		expect(source).toContain('globalThis.crypto.randomUUID()');
		expect(source).toContain('const isPreviewExpired = $derived(');
		expect(source).toContain('!isPreviewExpired');
		expect(source).not.toContain('expiresAt - previewClock');
	});
});
