import { MessageFlags } from 'discord.js';
import { describe, expect, it } from 'vitest';

import {
	BOT_COLORS,
	botContainer,
	botDeferOptions,
	botEditReply,
	botEphemeralReply,
	botNotice,
	botReply,
	botSeparator,
	botText
} from './message-v2.js';

describe('Discord Components V2 message helpers', () => {
	it('builds a container reply with a heading hierarchy and no legacy payload fields', () => {
		const reply = botReply(
			botContainer({
				color: BOT_COLORS.info,
				children: [botText('## 모집'), botSeparator(), botText('참가자를 기다립니다.')]
			})
		);

		expect(reply.flags).toBe(MessageFlags.IsComponentsV2);
		expect(reply.allowedMentions).toEqual({ parse: [] });
		expect(reply).not.toHaveProperty('content');
		expect(reply).not.toHaveProperty('embeds');
		expect(JSON.stringify(reply)).toContain('## 모집');
		expect(JSON.stringify(reply)).toContain('참가자를 기다립니다.');
	});

	it('keeps notice, ephemeral, defer, and edit payloads on Components V2 flags', () => {
		const notice = botNotice({
			tone: 'warning',
			title: '확인이 필요합니다.',
			body: '모집을 마감하기 전에 참가자를 확인해 주세요.',
			details: ['정원 10명', '현재 9명']
		});

		expect(JSON.stringify(notice)).toContain('## 확인이 필요합니다.');
		expect(JSON.stringify(notice)).toContain('정원 10명');
		expect(botEphemeralReply(notice).flags).toBe(
			MessageFlags.IsComponentsV2 | MessageFlags.Ephemeral
		);
		expect(botEphemeralReply(notice).allowedMentions).toEqual({ parse: [] });
		expect(botDeferOptions().flags).toBe(MessageFlags.IsComponentsV2);
		expect(botDeferOptions(true).flags).toBe(MessageFlags.IsComponentsV2 | MessageFlags.Ephemeral);
		expect(botEditReply(notice).flags).toBe(MessageFlags.IsComponentsV2);
	});
});
